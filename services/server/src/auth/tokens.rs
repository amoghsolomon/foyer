use std::fmt;
use std::fs;
use std::path::Path;

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Duration, Utc};
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode};
use p256::SecretKey;
use p256::elliptic_curve::sec1::ToEncodedPoint;
use p256::pkcs8::{DecodePrivateKey, EncodePrivateKey, EncodePublicKey, LineEnding};
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::config::{Config, RuntimeEnv};
use crate::error::{ApiError, ApiResult};

use super::Principal;

pub const ACCESS_TOKEN_TTL: Duration = Duration::minutes(5);

#[derive(Clone)]
pub struct TokenSigner {
    pub key_id: String,
    pub issuer: String,
    pub api_audience: String,
    pub powersync_audience: String,
    encoding: EncodingKey,
    decoding: DecodingKey,
    pub public_jwk: serde_json::Value,
}

impl fmt::Debug for TokenSigner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TokenSigner")
            .field("key_id", &self.key_id)
            .field("issuer", &self.issuer)
            .field("api_audience", &self.api_audience)
            .field("powersync_audience", &self.powersync_audience)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct IssuedClaims {
    pub sub: String,
    pub aud: String,
    pub iss: String,
    pub iat: i64,
    pub exp: i64,
    pub jti: String,
    #[serde(rename = "deviceKeyId", skip_serializing_if = "Option::is_none")]
    pub device_key_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct AccessSession {
    pub access_token: String,
    pub expires_at: DateTime<Utc>,
}

impl TokenSigner {
    pub fn from_config(config: &Config) -> Result<Self, String> {
        match config.auth_signing_key_path.as_deref() {
            Some(path) => Self::from_pem_path(
                path,
                config.auth_key_id.clone(),
                config.auth_issuer.clone(),
                config.auth_api_audience.clone(),
                config.powersync_audience.clone(),
            ),
            None if config.runtime_env == RuntimeEnv::Development => Self::ephemeral(
                config.auth_key_id.clone(),
                config.auth_issuer.clone(),
                config.auth_api_audience.clone(),
                config.powersync_audience.clone(),
            ),
            None => Err(
                "FOYER_AUTH_SIGNING_KEY_PATH is required when FOYER_SERVER_ENV is not development"
                    .into(),
            ),
        }
    }

    pub fn from_pem_path(
        path: impl AsRef<Path>,
        key_id: String,
        issuer: String,
        api_audience: String,
        powersync_audience: String,
    ) -> Result<Self, String> {
        let path = path.as_ref();
        let pem = fs::read_to_string(path).map_err(|error| {
            format!(
                "failed to read FOYER_AUTH_SIGNING_KEY_PATH {}: {error}",
                path.display()
            )
        })?;
        Self::from_pem(&pem, key_id, issuer, api_audience, powersync_audience).map_err(|error| {
            format!(
                "invalid FOYER_AUTH_SIGNING_KEY_PATH {}: {error}",
                path.display()
            )
        })
    }

    pub fn from_pem(
        pem: &str,
        key_id: String,
        issuer: String,
        api_audience: String,
        powersync_audience: String,
    ) -> Result<Self, String> {
        let secret = parse_p256_pem(pem)?;
        let encoding = EncodingKey::from_ec_pem(pem.as_bytes())
            .or_else(|_| {
                let pkcs8 = secret
                    .to_pkcs8_pem(LineEnding::LF)
                    .map_err(|_| "failed to encode the signing key as PKCS#8")?;
                EncodingKey::from_ec_pem(pkcs8.as_bytes())
                    .map_err(|_| "PEM is not a usable ES256 signing key".to_string())
            })
            .map_err(|_| "PEM is not a usable ES256 signing key".to_string())?;
        let public_pem = secret
            .public_key()
            .to_public_key_pem(LineEnding::LF)
            .map_err(|_| "failed to encode the ES256 public key".to_string())?;
        let decoding = DecodingKey::from_ec_pem(public_pem.as_bytes())
            .map_err(|_| "PEM is not a usable ES256 verification key".to_string())?;
        if key_id.trim().is_empty() {
            return Err("signing key id must not be empty".into());
        }
        if issuer.trim().is_empty() {
            return Err("issuer must not be empty".into());
        }
        if api_audience.trim().is_empty() {
            return Err("API audience must not be empty".into());
        }
        if powersync_audience.trim().is_empty() {
            return Err("PowerSync audience must not be empty".into());
        }
        if api_audience == powersync_audience {
            return Err(
                "FOYER_AUTH_API_AUDIENCE and FOYER_POWERSYNC_AUDIENCE must be distinct".into(),
            );
        }
        Ok(Self {
            key_id,
            issuer,
            api_audience,
            powersync_audience,
            encoding,
            decoding,
            public_jwk: public_jwk_from_secret(&secret),
        })
    }

    pub fn ephemeral(
        key_id: String,
        issuer: String,
        api_audience: String,
        powersync_audience: String,
    ) -> Result<Self, String> {
        let secret = SecretKey::random(&mut OsRng);
        let pem = secret
            .to_pkcs8_pem(LineEnding::LF)
            .map_err(|_| "failed to encode the development signing key".to_string())?;
        Self::from_pem(
            pem.as_str(),
            key_id,
            issuer,
            api_audience,
            powersync_audience,
        )
    }

    pub fn jwks(&self) -> serde_json::Value {
        let mut key = self.public_jwk.clone();
        if let Some(object) = key.as_object_mut() {
            object.insert("kid".into(), serde_json::Value::String(self.key_id.clone()));
            object.insert("alg".into(), serde_json::Value::String("ES256".into()));
            object.insert("use".into(), serde_json::Value::String("sig".into()));
        }
        serde_json::json!({ "keys": [key] })
    }

    pub fn issue_access(
        &self,
        user_id: &str,
        device_key_id: Option<&str>,
    ) -> ApiResult<AccessSession> {
        self.issue(user_id, device_key_id, &self.api_audience)
    }

    pub fn issue_powersync(
        &self,
        user_id: &str,
        device_key_id: Option<&str>,
    ) -> ApiResult<AccessSession> {
        self.issue(user_id, device_key_id, &self.powersync_audience)
    }

    fn issue(
        &self,
        user_id: &str,
        device_key_id: Option<&str>,
        audience: &str,
    ) -> ApiResult<AccessSession> {
        let now = Utc::now();
        let expires_at = now + ACCESS_TOKEN_TTL;
        let claims = IssuedClaims {
            sub: user_id.to_string(),
            aud: audience.to_string(),
            iss: self.issuer.clone(),
            iat: now.timestamp(),
            exp: expires_at.timestamp(),
            jti: Uuid::new_v4().to_string(),
            device_key_id: device_key_id.map(str::to_string),
        };
        let mut header = Header::new(Algorithm::ES256);
        header.kid = Some(self.key_id.clone());
        let access_token = encode(&header, &claims, &self.encoding)
            .map_err(|_| ApiError::unavailable("failed to sign an access token"))?;
        Ok(AccessSession {
            access_token,
            expires_at,
        })
    }

    pub fn authenticate_access(&self, token: &str) -> ApiResult<Principal> {
        self.decode(token, &self.api_audience)
            .map(|claims| Principal {
                user_id: claims.sub,
                device_key_id: claims.device_key_id,
            })
    }

    pub fn decode(&self, token: &str, audience: &str) -> ApiResult<IssuedClaims> {
        let header =
            jsonwebtoken::decode_header(token).map_err(|_| ApiError::authentication_failed())?;
        if header.alg != Algorithm::ES256 {
            return Err(ApiError::authentication_failed());
        }
        if header.kid.as_deref() != Some(self.key_id.as_str()) {
            return Err(ApiError::authentication_failed());
        }
        let mut validation = Validation::new(Algorithm::ES256);
        validation.set_audience(&[audience]);
        validation.set_issuer(&[&self.issuer]);
        validation.leeway = 5;
        validation.validate_nbf = false;
        decode::<IssuedClaims>(token, &self.decoding, &validation)
            .map(|data| data.claims)
            .map_err(|_| ApiError::authentication_failed())
    }
}

fn parse_p256_pem(pem: &str) -> Result<SecretKey, String> {
    SecretKey::from_pkcs8_pem(pem)
        .or_else(|_| SecretKey::from_sec1_pem(pem))
        .map_err(|_| "signing key must be a P-256 PKCS#8 or SEC1 PEM".to_string())
}

fn public_jwk_from_secret(secret: &SecretKey) -> serde_json::Value {
    let point = secret.public_key().to_encoded_point(false);
    let x = URL_SAFE_NO_PAD.encode(point.x().expect("uncompressed X"));
    let y = URL_SAFE_NO_PAD.encode(point.y().expect("uncompressed Y"));
    serde_json::json!({
        "kty": "EC",
        "crv": "P-256",
        "x": x,
        "y": y,
    })
}

pub fn looks_like_jwt(token: &str) -> bool {
    token.bytes().filter(|byte| *byte == b'.').count() == 2
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signer() -> TokenSigner {
        TokenSigner::ephemeral(
            "foyer-dev".into(),
            "foyer-server".into(),
            "foyer-api".into(),
            "foyer-powersync".into(),
        )
        .expect("ephemeral signer")
    }

    #[test]
    fn access_and_powersync_audiences_are_distinct() {
        let signer = signer();
        let access = signer
            .issue_access("owner", Some("device-1"))
            .expect("access");
        let sync = signer
            .issue_powersync("owner", Some("device-1"))
            .expect("sync");
        let access_claims = signer
            .decode(&access.access_token, "foyer-api")
            .expect("access claims");
        let sync_claims = signer
            .decode(&sync.access_token, "foyer-powersync")
            .expect("sync claims");
        assert_eq!(access_claims.aud, "foyer-api");
        assert_eq!(sync_claims.aud, "foyer-powersync");
        assert!(signer.decode(&sync.access_token, "foyer-api").is_err());
        assert!(
            signer
                .decode(&access.access_token, "foyer-powersync")
                .is_err()
        );
    }

    #[test]
    fn hs256_tokens_are_rejected() {
        let signer = signer();
        let mut header = Header::new(Algorithm::HS256);
        header.kid = Some(signer.key_id.clone());
        let token = encode(
            &header,
            &IssuedClaims {
                sub: "owner".into(),
                aud: signer.api_audience.clone(),
                iss: signer.issuer.clone(),
                iat: Utc::now().timestamp(),
                exp: (Utc::now() + ACCESS_TOKEN_TTL).timestamp(),
                jti: "jti".into(),
                device_key_id: Some("device-1".into()),
            },
            &EncodingKey::from_secret(b"not-the-server-key"),
        )
        .expect("hs256");
        assert!(signer.authenticate_access(&token).is_err());
    }

    #[test]
    fn missing_or_wrong_kid_is_rejected() {
        let signer = signer();
        let issued = signer.issue_access("owner", Some("device-1")).expect("jwt");
        let header = jsonwebtoken::decode_header(&issued.access_token).expect("header");
        assert_eq!(header.kid.as_deref(), Some("foyer-dev"));
        assert_eq!(header.alg, Algorithm::ES256);
        let other = TokenSigner::ephemeral(
            "other".into(),
            signer.issuer.clone(),
            signer.api_audience.clone(),
            signer.powersync_audience.clone(),
        )
        .expect("other");
        assert!(other.authenticate_access(&issued.access_token).is_err());

        let now = Utc::now();
        let claims = IssuedClaims {
            sub: "owner".into(),
            aud: signer.api_audience.clone(),
            iss: signer.issuer.clone(),
            iat: now.timestamp(),
            exp: (now + ACCESS_TOKEN_TTL).timestamp(),
            jti: "missing-kid".into(),
            device_key_id: Some("device-1".into()),
        };
        let without_kid = encode(&Header::new(Algorithm::ES256), &claims, &signer.encoding)
            .expect("sign token without kid");
        assert!(signer.authenticate_access(&without_kid).is_err());

        let mut wrong_header = Header::new(Algorithm::ES256);
        wrong_header.kid = Some("wrong".into());
        let wrong_kid =
            encode(&wrong_header, &claims, &signer.encoding).expect("sign token with wrong kid");
        assert!(signer.authenticate_access(&wrong_kid).is_err());
    }

    #[test]
    fn loads_pkcs8_pem_from_a_file() {
        let secret = SecretKey::random(&mut OsRng);
        let pem = secret.to_pkcs8_pem(LineEnding::LF).expect("pkcs8");
        let path = std::env::temp_dir().join(format!("foyer-auth-{}.pem", Uuid::new_v4()));
        std::fs::write(&path, pem.as_str()).expect("write pem");
        let loaded = TokenSigner::from_pem_path(
            &path,
            "prod-key".into(),
            "https://foyer.example".into(),
            "foyer-api".into(),
            "foyer-powersync".into(),
        );
        let _ = std::fs::remove_file(&path);
        let loaded = loaded.expect("load pem");
        let issued = loaded
            .issue_access("owner", Some("device-1"))
            .expect("issue");
        let principal = loaded
            .authenticate_access(&issued.access_token)
            .expect("validate");
        assert_eq!(principal.user_id, "owner");
        assert_eq!(principal.device_key_id.as_deref(), Some("device-1"));
        assert_eq!(loaded.key_id, "prod-key");
    }
}
