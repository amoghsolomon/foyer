use base64::{
    Engine as _,
    engine::general_purpose::{URL_SAFE, URL_SAFE_NO_PAD},
};
use p256::ecdsa::signature::Verifier;
use p256::ecdsa::{Signature, VerifyingKey};
use serde_json::Value;
use sha2::{Digest, Sha256};

pub const THUMBPRINT_LEN: usize = 43;
pub const P1363_LEN: usize = 64;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublicJwk {
    pub x: String,
    pub y: String,
}

impl PublicJwk {
    pub fn to_value(&self) -> Value {
        serde_json::json!({
            "kty": "EC",
            "crv": "P-256",
            "x": self.x,
            "y": self.y,
        })
    }

    pub fn canonical_json(&self) -> String {
        canonical_public_jwk_json(&self.x, &self.y)
    }

    pub fn device_key_id(&self) -> String {
        let digest = Sha256::digest(self.canonical_json().as_bytes());
        URL_SAFE_NO_PAD.encode(digest)
    }

    pub fn verifying_key(&self) -> Result<VerifyingKey, String> {
        let x = decode_b64url(&self.x)?;
        let y = decode_b64url(&self.y)?;
        if x.len() != 32 || y.len() != 32 {
            return Err("P-256 coordinates must be 32 bytes".into());
        }
        let mut sec1 = Vec::with_capacity(65);
        sec1.push(0x04);
        sec1.extend_from_slice(&x);
        sec1.extend_from_slice(&y);
        VerifyingKey::from_sec1_bytes(&sec1)
            .map_err(|_| "JWK is not a valid P-256 public point".into())
    }
}

pub fn canonical_public_jwk_json(x: &str, y: &str) -> String {
    format!(r#"{{"crv":"P-256","kty":"EC","x":"{x}","y":"{y}"}}"#)
}

pub fn parse_public_jwk(value: &Value) -> Result<PublicJwk, String> {
    let object = value
        .as_object()
        .ok_or_else(|| "JWK must be a JSON object".to_string())?;
    if object.contains_key("d") {
        return Err("public JWK must not include a private scalar".into());
    }
    let kty = object
        .get("kty")
        .and_then(Value::as_str)
        .ok_or_else(|| "JWK kty is required".to_string())?;
    let crv = object
        .get("crv")
        .and_then(Value::as_str)
        .ok_or_else(|| "JWK crv is required".to_string())?;
    if kty != "EC" || crv != "P-256" {
        return Err("only EC P-256 public keys are accepted".into());
    }
    let x = normalize_coordinate(object.get("x"), "x")?;
    let y = normalize_coordinate(object.get("y"), "y")?;
    let jwk = PublicJwk { x, y };
    jwk.verifying_key()?;
    Ok(jwk)
}

pub fn parse_public_jwk_bytes(bytes: &[u8]) -> Result<PublicJwk, String> {
    let value: Value =
        serde_json::from_slice(bytes).map_err(|error| format!("JWK is not valid JSON: {error}"))?;
    parse_public_jwk(&value)
}

pub fn is_device_key_id(value: &str) -> bool {
    value.len() == THUMBPRINT_LEN
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

pub fn decode_p1363(signature: &str) -> Result<[u8; P1363_LEN], String> {
    if signature.is_empty()
        || signature.contains('=')
        || !signature
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        return Err("signature must be unpadded base64url".into());
    }
    let bytes = URL_SAFE_NO_PAD
        .decode(signature)
        .map_err(|_| "invalid base64url signature".to_string())?;
    if bytes.len() != P1363_LEN {
        return Err("signature must be 64-byte IEEE P1363 r||s".into());
    }
    let mut out = [0_u8; P1363_LEN];
    out.copy_from_slice(&bytes);
    Ok(out)
}

pub fn verify_p1363(jwk: &PublicJwk, payload: &[u8], signature: &str) -> Result<(), String> {
    let bytes = decode_p1363(signature)?;
    let signature = Signature::from_slice(&bytes)
        .map_err(|_| "signature must be 64-byte IEEE P1363 r||s".to_string())?;
    let key = jwk.verifying_key()?;
    key.verify(payload, &signature)
        .map_err(|_| "signature verification failed".to_string())
}

pub fn decode_b64url(value: &str) -> Result<Vec<u8>, String> {
    let trimmed = value.trim();
    URL_SAFE_NO_PAD
        .decode(trimmed)
        .or_else(|_| URL_SAFE.decode(trimmed))
        .map_err(|_| "invalid base64url".to_string())
}

fn normalize_coordinate(value: Option<&Value>, name: &str) -> Result<String, String> {
    let raw = value
        .and_then(Value::as_str)
        .ok_or_else(|| format!("JWK {name} is required"))?;
    let bytes = decode_b64url(raw)?;
    if bytes.len() != 32 {
        return Err(format!("JWK {name} must decode to 32 bytes"));
    }
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const RFC_X: &str = "MKBCTNIcKUSDii11ySs3526iDZ8AiTo7Tu6KPAqv7D4";
    const RFC_Y: &str = "4Etl6SRW2YiLUrN5vfvVHuhp7x8PxltmWWlbbM4IFyM";
    const RFC_THUMBPRINT: &str = "cn-I_WNMClehiVp51i_0VpOENW1upEerA8sEam5hn-s";

    #[test]
    fn rfc7638_thumbprint_matches_appendix_a() {
        let jwk = parse_public_jwk(&json!({
            "kty": "EC",
            "crv": "P-256",
            "x": RFC_X,
            "y": RFC_Y,
        }))
        .expect("rfc jwk");
        assert_eq!(
            jwk.canonical_json(),
            format!(r#"{{"crv":"P-256","kty":"EC","x":"{RFC_X}","y":"{RFC_Y}"}}"#)
        );
        assert_eq!(jwk.device_key_id(), RFC_THUMBPRINT);
    }

    #[test]
    fn padded_coordinates_normalize_to_the_same_thumbprint() {
        let jwk = parse_public_jwk(&json!({
            "kty": "EC",
            "crv": "P-256",
            "x": format!("{RFC_X}="),
            "y": format!("{RFC_Y}="),
        }))
        .expect("padded jwk");
        assert_eq!(jwk.device_key_id(), RFC_THUMBPRINT);
    }

    #[test]
    fn private_scalar_is_rejected() {
        let error = parse_public_jwk(&json!({
            "kty": "EC",
            "crv": "P-256",
            "x": RFC_X,
            "y": RFC_Y,
            "d": "870MB6gfuTJ4HtUnUvYMyJpr5eUZNP4Bk43bVdj3eAE",
        }))
        .expect_err("private jwk");
        assert!(error.contains("private"));
    }

    #[test]
    fn rsa_jwk_is_rejected() {
        assert!(
            parse_public_jwk(&json!({
                "kty": "RSA",
                "n": "sXch",
                "e": "AQAB",
            }))
            .is_err()
        );
    }

    #[test]
    fn off_curve_point_is_rejected() {
        assert!(
            parse_public_jwk(&json!({
                "kty": "EC",
                "crv": "P-256",
                "x": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
                "y": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
            }))
            .is_err()
        );
    }
}
