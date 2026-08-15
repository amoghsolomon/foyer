use std::fmt::{Debug, Formatter};
use std::time::Duration;

use super::error::DavError;
use super::path::HttpUrl;

pub const MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_XML_BYTES: usize = 1024 * 1024;
pub const MAX_XML_DEPTH: usize = 32;
pub const MAX_XML_ELEMENTS: usize = 4_096;
pub const MAX_RESOURCE_BYTES: usize = 256 * 1024;
pub const MAX_MULTIGET: usize = 64;
pub const MAX_HEADER_BYTES: usize = 64 * 1024;
pub const MAX_DISPLAY_NAME: usize = 200;
pub const MAX_REDIRECTS: usize = 3;
pub const USER_AGENT: &str = "foyer-server-dav/0.1";

#[derive(Clone)]
pub struct Secret(String);

impl Secret {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub(super) fn expose(&self) -> &str {
        &self.0
    }

    pub fn contains_in(&self, haystack: &str) -> bool {
        !self.0.is_empty() && haystack.contains(&self.0)
    }
}

impl Debug for Secret {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str("[redacted]")
    }
}

#[derive(Clone, Debug)]
pub struct DavConfig {
    pub base_url: String,
    pub username: String,
    pub password: Secret,
    pub connect_timeout: Duration,
    pub request_timeout: Duration,
    pub max_response_bytes: usize,
    pub max_xml_bytes: usize,
    pub max_xml_depth: usize,
    pub max_xml_elements: usize,
    pub max_multiget: usize,
    pub max_resource_bytes: usize,
}

impl DavConfig {
    pub fn new(
        base_url: impl Into<String>,
        username: impl Into<String>,
        password: impl Into<String>,
    ) -> Result<Self, DavError> {
        let config = Self {
            base_url: base_url.into(),
            username: username.into(),
            password: Secret::new(password),
            connect_timeout: Duration::from_secs(5),
            request_timeout: Duration::from_secs(30),
            max_response_bytes: MAX_RESPONSE_BYTES,
            max_xml_bytes: MAX_XML_BYTES,
            max_xml_depth: MAX_XML_DEPTH,
            max_xml_elements: MAX_XML_ELEMENTS,
            max_multiget: MAX_MULTIGET,
            max_resource_bytes: MAX_RESOURCE_BYTES,
        };
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), DavError> {
        let url = HttpUrl::parse(&self.base_url)?;
        if url.path != "/" {
            return Err(DavError::UnsafeUrl(
                "DAV base URL must be an origin with an empty path".into(),
            ));
        }
        if self.username.is_empty() || self.username.len() > 128 {
            return Err(DavError::InvalidRequest(
                "DAV service username is missing or too long".into(),
            ));
        }
        if self.username.chars().any(|ch| ch == ':' || ch.is_control()) {
            return Err(DavError::InvalidRequest(
                "DAV service username contains reserved characters".into(),
            ));
        }
        if self.password.is_empty() {
            return Err(DavError::InvalidRequest(
                "DAV service password is required".into(),
            ));
        }
        if self.max_response_bytes == 0 || self.max_xml_bytes == 0 {
            return Err(DavError::InvalidRequest(
                "DAV size bounds must be positive".into(),
            ));
        }
        if self.max_multiget == 0 || self.max_multiget > 256 {
            return Err(DavError::InvalidRequest(
                "DAV multiget bound must be between 1 and 256".into(),
            ));
        }
        Ok(())
    }

    pub fn origin(&self) -> Result<HttpUrl, DavError> {
        HttpUrl::parse(&self.base_url)
    }

    pub fn redact(&self, text: &str) -> String {
        let mut redacted = text.to_string();
        if self.password.contains_in(&redacted) {
            redacted = redacted.replace(self.password.expose(), "[redacted]");
        }
        if let Ok(header) = basic_auth_header(&self.username, self.password.expose())
            && let Some((_, token)) = header.split_once(' ')
            && redacted.contains(token)
        {
            redacted = redacted.replace(token, "[redacted]");
        }
        redacted
    }
}

pub fn basic_auth_header(username: &str, password: &str) -> Result<String, DavError> {
    use base64::Engine as _;
    if username.contains(':') {
        return Err(DavError::InvalidRequest(
            "DAV username cannot contain ':'".into(),
        ));
    }
    let token = base64::engine::general_purpose::STANDARD.encode(format!("{username}:{password}"));
    Ok(format!("Basic {token}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_is_redacted_in_debug() {
        let config = DavConfig::new("http://127.0.0.1:5232", "foyer", "super-secret").unwrap();
        let rendered = format!("{config:?}");
        assert!(!rendered.contains("super-secret"));
        assert!(rendered.contains("[redacted]"));
    }

    #[test]
    fn https_base_url_is_rejected() {
        let error = DavConfig::new("https://dav.example", "foyer", "secret").unwrap_err();
        assert!(matches!(error, DavError::UnsafeUrl(_)));
    }

    #[test]
    fn redact_removes_password_and_basic_token() {
        let config = DavConfig::new("http://127.0.0.1:5232", "foyer", "super-secret").unwrap();
        let header = basic_auth_header("foyer", "super-secret").unwrap();
        let leaked = format!("Authorization: {header} password=super-secret");
        let redacted = config.redact(&leaked);
        assert!(!redacted.contains("super-secret"));
        assert!(!redacted.contains(header.split(' ').nth(1).unwrap()));
    }
}
