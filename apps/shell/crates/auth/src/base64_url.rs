use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};

pub fn encode(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

pub fn decode(value: &str) -> Result<Vec<u8>, String> {
    let normalized = value.trim();
    if normalized.is_empty() {
        return Err("base64url value is empty".into());
    }
    URL_SAFE_NO_PAD
        .decode(normalized)
        .map_err(|_| "invalid base64url value".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_without_padding() {
        let encoded = encode(&[1, 2, 3, 4]);
        assert!(!encoded.contains('='));
        assert_eq!(decode(&encoded).expect("decode"), [1, 2, 3, 4]);
    }
}
