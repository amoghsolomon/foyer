use std::fmt::{Display, Formatter};

use p256::ecdsa::VerifyingKey;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::base64_url;

const COORDINATE: &str = r"^[A-Za-z0-9_-]+$";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicJwk {
    pub kty: String,
    pub crv: String,
    pub x: String,
    pub y: String,
}

impl PublicJwk {
    pub fn p256(x: impl Into<String>, y: impl Into<String>) -> Result<Self, String> {
        let jwk = Self {
            kty: "EC".into(),
            crv: "P-256".into(),
            x: x.into(),
            y: y.into(),
        };
        jwk.validate()?;
        Ok(jwk)
    }

    pub fn from_verifying_key(key: &VerifyingKey) -> Result<Self, String> {
        let point = key.to_encoded_point(false);
        let x = point
            .x()
            .ok_or_else(|| "P-256 public key is missing x".to_string())?;
        let y = point
            .y()
            .ok_or_else(|| "P-256 public key is missing y".to_string())?;
        Self::p256(base64_url::encode(x), base64_url::encode(y))
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.kty != "EC" {
            return Err("only EC public keys are supported".into());
        }
        if self.crv != "P-256" {
            return Err("only P-256 public keys are supported".into());
        }
        if self.x.is_empty() || self.y.is_empty() {
            return Err("JWK coordinates are required".into());
        }
        let coordinate = regex_is_match(COORDINATE, &self.x) && regex_is_match(COORDINATE, &self.y);
        if !coordinate {
            return Err("JWK coordinates must be unpadded base64url".into());
        }
        Ok(())
    }

    pub fn canonical_json(&self) -> String {
        format!(
            r#"{{"crv":"{}","kty":"{}","x":"{}","y":"{}"}}"#,
            self.crv, self.kty, self.x, self.y
        )
    }

    pub fn device_key_id(&self) -> String {
        let digest = Sha256::digest(self.canonical_json().as_bytes());
        base64_url::encode(&digest)
    }

    pub fn normalized(&self) -> Self {
        Self {
            kty: "EC".into(),
            crv: "P-256".into(),
            x: self.x.clone(),
            y: self.y.clone(),
        }
    }
}

fn regex_is_match(pattern: &str, value: &str) -> bool {
    // Coordinates are unpadded base64url: keep this check allocation-free and
    // dependency-free rather than pulling in the regex crate.
    let _ = pattern;
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnrollmentMaterial {
    pub jwk: PublicJwk,
    pub device_key_id: String,
}

impl EnrollmentMaterial {
    pub fn new(jwk: PublicJwk) -> Self {
        let jwk = jwk.normalized();
        let device_key_id = jwk.device_key_id();
        Self { jwk, device_key_id }
    }

    pub fn enrollment_json(&self) -> String {
        format!(
            "{{\n  \"kty\": \"{}\",\n  \"crv\": \"{}\",\n  \"x\": \"{}\",\n  \"y\": \"{}\",\n  \"deviceKeyId\": \"{}\"\n}}",
            self.jwk.kty, self.jwk.crv, self.jwk.x, self.jwk.y, self.device_key_id
        )
    }

    pub fn fingerprint(&self) -> &str {
        &self.device_key_id
    }

    pub fn short_fingerprint(&self) -> &str {
        let end = self.device_key_id.len().min(12);
        &self.device_key_id[..end]
    }
}

impl Display for EnrollmentMaterial {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.device_key_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc7638_example_matches_canonical_thumbprint() {
        let jwk = PublicJwk::p256(
            "MKBCTNIcKUSDii11ySs3526iDZ8AiTo7Tu6KPAqv7D4",
            "4Etl6SRW2YiLUrN5vfvVHuhp7x8PxltmWWlbbM4IFyM",
        )
        .expect("fixture jwk");
        assert_eq!(
            jwk.canonical_json(),
            r#"{"crv":"P-256","kty":"EC","x":"MKBCTNIcKUSDii11ySs3526iDZ8AiTo7Tu6KPAqv7D4","y":"4Etl6SRW2YiLUrN5vfvVHuhp7x8PxltmWWlbbM4IFyM"}"#
        );
        assert_eq!(
            jwk.device_key_id(),
            "cn-I_WNMClehiVp51i_0VpOENW1upEerA8sEam5hn-s"
        );
    }

    #[test]
    fn enrollment_json_is_public_only() {
        let material = EnrollmentMaterial::new(
            PublicJwk::p256(
                "MKBCTNIcKUSDii11ySs3526iDZ8AiTo7Tu6KPAqv7D4",
                "4Etl6SRW2YiLUrN5vfvVHuhp7x8PxltmWWlbbM4IFyM",
            )
            .expect("fixture jwk"),
        );
        let json = material.enrollment_json();
        assert!(json.contains(r#""kty": "EC""#));
        assert!(json.contains(r#""deviceKeyId""#));
        assert!(!json.contains(r#""d""#));
        assert!(!json.to_ascii_lowercase().contains("private"));
    }
}
