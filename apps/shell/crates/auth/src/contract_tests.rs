use std::fs;
use std::path::PathBuf;

use p256::ecdsa::{Signature, SigningKey};
use serde_json::Value;

use crate::jwk::PublicJwk;
use crate::keystore::DeviceSigningKey;
use crate::{decode_base64url, encode_base64url};

const RFC7638_X: &str = "MKBCTNIcKUSDii11ySs3526iDZ8AiTo7Tu6KPAqv7D4";
const RFC7638_Y: &str = "4Etl6SRW2YiLUrN5vfvVHuhp7x8PxltmWWlbbM4IFyM";
const RFC7638_THUMBPRINT: &str = "cn-I_WNMClehiVp51i_0VpOENW1upEerA8sEam5hn-s";

#[test]
fn rfc7638_thumbprint_matches_android_and_adr() {
    let jwk = PublicJwk::p256(RFC7638_X, RFC7638_Y).expect("jwk");
    assert_eq!(jwk.device_key_id(), RFC7638_THUMBPRINT);
}

#[test]
fn contract_canonical_jwk_and_thumbprint_fixtures_match() {
    let Some(canonical) = load_contract_text("canonical-jwk.json") else {
        return;
    };
    let canonical = canonical.trim().to_string();
    let jwk: PublicJwk = serde_json::from_str(&canonical).expect("canonical jwk");
    assert_eq!(jwk.canonical_json(), canonical);
    assert_eq!(jwk.device_key_id(), RFC7638_THUMBPRINT);
    if let Some(thumbprint) = load_contract_text("thumbprint.txt") {
        assert_eq!(thumbprint.trim(), jwk.device_key_id());
    }
    if let Some(public) = load_contract_json("rfc7517-public.jwk.json") {
        let published = PublicJwk::p256(required_str(&public, "x"), required_str(&public, "y"))
            .expect("rfc7517 public");
        assert_eq!(published, jwk);
        assert_eq!(published.device_key_id(), RFC7638_THUMBPRINT);
    }
}

#[test]
fn contract_rfc7517_private_key_matches_public_jwk() {
    let Some(private) = load_contract_json("rfc7517-private.jwk.json") else {
        return;
    };
    let secret = decode_secret(&required_str(&private, "d"));
    let key = DeviceSigningKey::from_secret_bytes(&secret).expect("rfc7517 private");
    assert_eq!(key.material().jwk.x, required_str(&private, "x"));
    assert_eq!(key.material().jwk.y, required_str(&private, "y"));
    assert_eq!(key.material().device_key_id, RFC7638_THUMBPRINT);
}

#[test]
fn contract_signing_payload_matches_documented_construction() {
    let Some(meta) = load_contract_json("signing-payload-meta.json") else {
        return;
    };
    let device_key_id = required_str(&meta, "deviceKeyId");
    let audience = required_str(&meta, "apiAudience");
    let expires_at = required_str(&meta, "expiresAt");
    let nonce = hex(&required_str(&meta, "nonceHex"));
    let mut payload = b"FOYER-AUTH-CHALLENGE-V1".to_vec();
    payload.push(0);
    payload.extend_from_slice(device_key_id.as_bytes());
    payload.push(0);
    payload.extend_from_slice(audience.as_bytes());
    payload.push(0);
    payload.extend_from_slice(expires_at.as_bytes());
    payload.push(0);
    payload.extend_from_slice(&nonce);

    if let Some(example) = load_contract_json("challenge-response.json") {
        let encoded = required_str(&example, "signingPayload");
        assert_eq!(
            decode_base64url(&encoded).expect("example payload"),
            payload
        );
        assert!(!encoded.contains('='));
    }
    if let Some(encoded) = load_contract_text("signing-payload.b64") {
        assert_eq!(
            decode_base64url(encoded.trim()).expect("fixture payload"),
            payload
        );
    }
    if let Some(private) = load_contract_json("rfc7517-private.jwk.json") {
        let key = DeviceSigningKey::from_secret_bytes(&decode_secret(&required_str(&private, "d")))
            .expect("rfc7517 private");
        let signature = key.sign_sha256(&payload);
        assert_eq!(signature.len(), 64);
        if let Some(expected) = load_contract_text("signature.b64") {
            assert_eq!(
                signature.as_slice(),
                decode_base64url(expected.trim())
                    .expect("fixture signature")
                    .as_slice()
            );
        }
        if let Some(example) = load_contract_json("session-request.json") {
            let encoded = required_str(&example, "signature");
            if encoded != "REPLACE_WITH_FIXTURE_SIGNATURE" {
                assert_eq!(
                    signature.as_slice(),
                    decode_base64url(&encoded)
                        .expect("example signature")
                        .as_slice()
                );
            }
        }
    }
}

#[test]
fn contract_jwk_thumbprint_matches_when_present() {
    if let Some(canonical) = load_contract_text("fixtures/canonical-jwk.json") {
        let expected = PublicJwk::p256(RFC7638_X, RFC7638_Y)
            .expect("rfc jwk")
            .canonical_json();
        assert_eq!(canonical.trim(), expected);
    }
    if let Some(public) = load_contract_json("fixtures/rfc7517-public.jwk.json") {
        let jwk = PublicJwk::p256(required_str(&public, "x"), required_str(&public, "y"))
            .expect("contract jwk");
        assert_eq!(jwk.device_key_id(), RFC7638_THUMBPRINT);
        if let Some(thumbprint) = load_contract_text("fixtures/thumbprint.txt") {
            assert_eq!(jwk.device_key_id(), thumbprint.trim());
        }
    }
    if let Some(legacy) = load_contract_json("jwk-thumbprint.json") {
        let jwk_value = legacy.get("jwk").cloned().unwrap_or(legacy.clone());
        let jwk = PublicJwk::p256(required_str(&jwk_value, "x"), required_str(&jwk_value, "y"))
            .expect("legacy contract jwk");
        if let Some(expected) = legacy
            .get("deviceKeyId")
            .or_else(|| legacy.get("thumbprint"))
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
        {
            assert_eq!(jwk.device_key_id(), expected);
        }
    }
}

#[test]
fn rfc6979_p256_sample_signature_is_fixed_p1363() {
    let secret = hex("C9AFA9D845BA75166B5C215767B1D6934E50C3DB36E89B127B8A622B120F6721");
    let key = DeviceSigningKey::from_secret_bytes(&secret).expect("rfc6979 key");
    let signature = key.sign_sha256(b"sample");
    assert_eq!(signature.len(), 64);
    let expected = hex(concat!(
        "EFD48B2AACB6A8FD1140DD9CD45E81D69D2C877B56AAF991C34D0EA84EAF3716",
        "F7CB1C942D657C41D436C7A1B6E29F65F3E900DBB9AFF4064DC4AB2F843ACDA8"
    ));
    assert_eq!(signature.as_slice(), expected.as_slice());
    assert!(!encode_base64url(&signature).contains('='));
}

#[test]
fn contract_signature_fixture_matches_when_present() {
    if let (Some(payload_b64), Some(signature_b64), Some(private)) = (
        load_contract_text("fixtures/signing-payload.b64"),
        load_contract_text("fixtures/signature.b64"),
        load_contract_json("fixtures/rfc7517-private.jwk.json"),
    ) {
        let secret = decode_base64url(&required_str(&private, "d")).expect("private d");
        let key = DeviceSigningKey::from_secret_bytes(&secret).expect("rfc7517 key");
        assert_eq!(key.material().device_key_id, RFC7638_THUMBPRINT);
        let payload = decode_base64url(payload_b64.trim()).expect("payload");
        let expected = decode_base64url(signature_b64.trim()).expect("signature");
        assert_eq!(key.sign_sha256(&payload).as_slice(), expected.as_slice());
    }
    if let (Some(challenge), Some(private)) = (
        load_contract_json("examples/challenge-response.json"),
        load_contract_json("fixtures/rfc7517-private.jwk.json"),
    ) {
        let secret = decode_base64url(&required_str(&private, "d")).expect("private d");
        let key = DeviceSigningKey::from_secret_bytes(&secret).expect("rfc7517 key");
        assert_eq!(key.material().device_key_id, RFC7638_THUMBPRINT);
        let payload =
            decode_base64url(&required_str(&challenge, "signingPayload")).expect("example payload");
        let signature = key.sign_sha256(&payload);
        assert_eq!(signature.len(), 64);
        assert!(!encode_base64url(&signature).contains('='));
    }
    if let Some(fixture) = load_contract_json("challenge-signature.json")
        .or_else(|| load_contract_json("ecdsa-p256-sha256.json"))
    {
        if let Some(cases) = fixture.get("cases").and_then(Value::as_array) {
            for case in cases {
                assert_contract_signature_case(case);
            }
        } else {
            assert_contract_signature_case(&fixture);
        }
    }
}

#[test]
fn signing_key_public_jwk_round_trips() {
    let key = DeviceSigningKey::generate().expect("generate");
    let encoded = key.secret_bytes();
    let again = DeviceSigningKey::from_secret_bytes(encoded.as_ref()).expect("reload");
    assert_eq!(key.material().jwk, again.material().jwk);
    assert_eq!(key.material().device_key_id, again.material().device_key_id);
}

#[test]
fn generated_signature_is_64_byte_p1363() {
    let key = DeviceSigningKey::generate().expect("generate");
    let signature = key.sign_sha256(b"foyer-device-auth-fixture");
    assert_eq!(signature.len(), 64);
    let verifying = *SigningKey::from_slice(key.secret_bytes().as_ref())
        .expect("signing key")
        .verifying_key();
    let parsed = Signature::from_slice(&signature).expect("p1363");
    p256::ecdsa::signature::Verifier::verify(&verifying, b"foyer-device-auth-fixture", &parsed)
        .expect("verify");
}

fn assert_contract_signature_case(case: &Value) {
    if let (Some(d), Some(payload)) = (
        case.get("d").and_then(Value::as_str),
        case.get("payload")
            .or_else(|| case.get("signingPayload"))
            .and_then(Value::as_str),
    ) {
        let secret = decode_secret(d);
        let key = DeviceSigningKey::from_secret_bytes(&secret).expect("fixture key");
        let payload_bytes = decode_payload(payload);
        let signature = key.sign_sha256(&payload_bytes);
        if let Some(expected) = case
            .get("signature")
            .or_else(|| case.get("p1363"))
            .and_then(Value::as_str)
        {
            let expected = decode_payload(expected);
            assert_eq!(signature.as_slice(), expected.as_slice());
        }
        if let Some(jwk) = case.get("jwk") {
            let expected = PublicJwk::p256(required_str(jwk, "x"), required_str(jwk, "y"))
                .expect("fixture jwk");
            assert_eq!(key.material().jwk, expected);
        }
        if let Some(device_key_id) = case.get("deviceKeyId").and_then(Value::as_str) {
            assert_eq!(key.material().device_key_id, device_key_id);
        }
    }
}

fn decode_secret(value: &str) -> Vec<u8> {
    if value.chars().all(|ch| ch.is_ascii_hexdigit()) && value.len() == 64 {
        return hex(value);
    }
    decode_base64url(value).unwrap_or_else(|_| hex(value))
}

fn decode_payload(value: &str) -> Vec<u8> {
    decode_base64url(value).unwrap_or_else(|_| value.as_bytes().to_vec())
}

fn required_str(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("fixture missing {key}"))
        .to_string()
}

fn hex(value: &str) -> Vec<u8> {
    let cleaned: String = value.chars().filter(|ch| !ch.is_whitespace()).collect();
    (0..cleaned.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&cleaned[index..index + 2], 16).expect("hex"))
        .collect()
}

fn load_contract_text(name: &str) -> Option<String> {
    contract_file(name).and_then(|path| fs::read_to_string(path).ok())
}

fn load_contract_json(name: &str) -> Option<Value> {
    load_contract_text(name).and_then(|text| serde_json::from_str(&text).ok())
}

fn contract_file(name: &str) -> Option<PathBuf> {
    let mut roots = Vec::new();
    let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for _ in 0..8 {
        roots.push(dir.join("contracts/auth/v1"));
        if !dir.pop() {
            break;
        }
    }
    roots.push(PathBuf::from(
        "/home/user/Projects/amazity/foyer/contracts/auth/v1",
    ));
    for root in roots {
        for candidate in [
            root.join(name),
            root.join("fixtures").join(name),
            root.join("examples").join(name),
        ] {
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}
