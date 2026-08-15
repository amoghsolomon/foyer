use std::fmt::{Display, Formatter};

use super::config::MAX_DISPLAY_NAME;
use super::error::DavError;

pub const CALENDARS_SEGMENT: &str = "calendars";
pub const TASKS_SEGMENT: &str = "tasks";
pub const ADDRESSBOOKS_SEGMENT: &str = "addressbooks";
const MAX_SEGMENT: usize = 128;
const MAX_HREF: usize = 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CollectionKind {
    Calendar,
    TaskList,
    AddressBook,
}

impl CollectionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Calendar => "calendar",
            Self::TaskList => "task_list",
            Self::AddressBook => "address_book",
        }
    }

    pub fn parse(value: &str) -> Result<Self, DavError> {
        match value {
            "calendar" => Ok(Self::Calendar),
            "task_list" => Ok(Self::TaskList),
            "address_book" => Ok(Self::AddressBook),
            _ => Err(DavError::InvalidRequest(format!(
                "unknown DAV collection kind {value:?}"
            ))),
        }
    }

    pub fn home_segment(self) -> &'static str {
        match self {
            Self::Calendar => CALENDARS_SEGMENT,
            Self::TaskList => TASKS_SEGMENT,
            Self::AddressBook => ADDRESSBOOKS_SEGMENT,
        }
    }

    pub fn resource_extension(self) -> &'static str {
        match self {
            Self::Calendar | Self::TaskList => ".ics",
            Self::AddressBook => ".vcf",
        }
    }

    pub fn media_type(self) -> &'static str {
        match self {
            Self::Calendar | Self::TaskList => "text/calendar",
            Self::AddressBook => "text/vcard",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HttpUrl {
    pub host: String,
    pub port: u16,
    pub path: String,
}

impl HttpUrl {
    pub fn parse(raw: &str) -> Result<Self, DavError> {
        let raw = raw.trim();
        let rest = raw.strip_prefix("http://").ok_or_else(|| {
            DavError::UnsafeUrl(
                "only http:// DAV origins are accepted on the private network".into(),
            )
        })?;
        if rest.contains('@') {
            return Err(DavError::UnsafeUrl(
                "DAV URLs must not embed credentials".into(),
            ));
        }
        if rest.contains('#') {
            return Err(DavError::UnsafeUrl(
                "DAV URLs must not include fragments".into(),
            ));
        }
        let (authority, path_and_query) = rest.split_once('/').unwrap_or((rest, ""));
        if authority.is_empty() || authority.contains('\\') {
            return Err(DavError::UnsafeUrl("DAV URL host is missing".into()));
        }
        if path_and_query.contains('?') {
            return Err(DavError::UnsafeUrl(
                "DAV base URLs must not include a query".into(),
            ));
        }
        let (host, port) = split_host_port(authority)?;
        let path = if path_and_query.is_empty() {
            "/".to_string()
        } else {
            normalize_absolute_path(&format!("/{path_and_query}"))?
        };
        Ok(Self { host, port, path })
    }

    pub fn authority(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }

    pub fn origin(&self) -> String {
        format!("http://{}", self.authority())
    }

    pub fn join_href(&self, href: &DavHref) -> String {
        format!("{}{}", self.origin(), href.request_target())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct DavHref(String);

impl DavHref {
    pub fn parse(raw: &str) -> Result<Self, DavError> {
        let decoded = percent_decode(raw.trim())?;
        let path = normalize_absolute_path(&decoded)?;
        Ok(Self(path))
    }

    pub fn from_dav(base: &HttpUrl, value: &str) -> Result<Self, DavError> {
        let value = value.trim();
        if value.is_empty() {
            return Err(DavError::UnsafePath("empty DAV href".into()));
        }
        if let Some(rest) = value.strip_prefix("http://") {
            let parsed = HttpUrl::parse(&format!("http://{rest}"))?;
            if parsed.host != base.host || parsed.port != base.port {
                return Err(DavError::UnsafeUrl(
                    "DAV href escaped the configured origin".into(),
                ));
            }
            return Self::parse(&parsed.path);
        }
        if value.contains("://") {
            return Err(DavError::UnsafeUrl(
                "DAV href uses a disallowed scheme".into(),
            ));
        }
        Self::parse(value)
    }

    pub fn root() -> Self {
        Self("/".into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_collection(&self) -> bool {
        self.0.ends_with('/')
    }

    pub fn as_collection(&self) -> Self {
        if self.is_collection() {
            self.clone()
        } else {
            Self(format!("{}/", self.0))
        }
    }

    pub fn parent(&self) -> Option<Self> {
        if self.0 == "/" {
            return None;
        }
        let trimmed = self.0.trim_end_matches('/');
        let Some((parent, _)) = trimmed.rsplit_once('/') else {
            return Some(Self::root());
        };
        if parent.is_empty() {
            Some(Self::root())
        } else {
            Some(Self(format!("{parent}/")))
        }
    }

    pub fn join_segment(&self, segment: &str) -> Result<Self, DavError> {
        let segment = validate_segment(segment)?;
        Ok(Self(format!("{}{segment}/", self.as_collection().0)))
    }

    pub fn join_resource(&self, name: &str) -> Result<Self, DavError> {
        let name = validate_resource_name(name)?;
        Ok(Self(format!("{}{name}", self.as_collection().0)))
    }

    pub fn starts_with(&self, prefix: &DavHref) -> bool {
        let prefix = prefix.as_collection();
        self.0 == prefix.0.trim_end_matches('/') || self.0.starts_with(&prefix.0)
    }

    pub fn request_target(&self) -> String {
        encode_path(&self.0)
    }
}

impl Display for DavHref {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UserPaths {
    pub user_id: String,
    pub principal: DavHref,
    pub calendar_home: DavHref,
    pub task_home: DavHref,
    pub addressbook_home: DavHref,
}

impl UserPaths {
    pub fn for_user(user_id: &str) -> Result<Self, DavError> {
        let user = validate_segment(user_id)?;
        let principal = DavHref::root().join_segment(&user)?;
        Ok(Self {
            user_id: user.clone(),
            calendar_home: principal.join_segment(CALENDARS_SEGMENT)?,
            task_home: principal.join_segment(TASKS_SEGMENT)?,
            addressbook_home: principal.join_segment(ADDRESSBOOKS_SEGMENT)?,
            principal,
        })
    }

    pub fn home(&self, kind: CollectionKind) -> &DavHref {
        match kind {
            CollectionKind::Calendar => &self.calendar_home,
            CollectionKind::TaskList => &self.task_home,
            CollectionKind::AddressBook => &self.addressbook_home,
        }
    }

    pub fn collection(
        &self,
        kind: CollectionKind,
        collection_id: &str,
    ) -> Result<DavHref, DavError> {
        self.home(kind).join_segment(collection_id)
    }

    pub fn resource(
        &self,
        kind: CollectionKind,
        collection_id: &str,
        resource_id: &str,
    ) -> Result<DavHref, DavError> {
        let filename = resource_filename(kind, resource_id)?;
        self.collection(kind, collection_id)?
            .join_resource(&filename)
    }

    pub fn ensure_owned(&self, href: &DavHref) -> Result<(), DavError> {
        if href.starts_with(&self.principal) {
            Ok(())
        } else {
            Err(DavError::UnsafePath(format!(
                "href {href} is outside user {}",
                self.user_id
            )))
        }
    }
}

pub fn resource_filename(kind: CollectionKind, resource_id: &str) -> Result<String, DavError> {
    let id = validate_segment(resource_id)?;
    Ok(format!("{id}{}", kind.resource_extension()))
}

pub fn validate_display_name(value: &str) -> Result<String, DavError> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.chars().count() > MAX_DISPLAY_NAME {
        return Err(DavError::InvalidRequest(format!(
            "display name must be between 1 and {MAX_DISPLAY_NAME} characters"
        )));
    }
    if trimmed.contains('\0') || trimmed.chars().any(char::is_control) {
        return Err(DavError::InvalidRequest(
            "display name cannot contain control characters".into(),
        ));
    }
    Ok(trimmed.to_string())
}

pub fn validate_segment(value: &str) -> Result<String, DavError> {
    if value.is_empty() || value.len() > MAX_SEGMENT {
        return Err(DavError::UnsafePath(
            "DAV path segment is empty or too long".into(),
        ));
    }
    if value == "." || value == ".." {
        return Err(DavError::UnsafePath(
            "DAV path segment cannot be '.' or '..'".into(),
        ));
    }
    if !value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
    {
        return Err(DavError::UnsafePath(
            "DAV path segment contains disallowed characters".into(),
        ));
    }
    if value.contains("..") {
        return Err(DavError::UnsafePath(
            "DAV path segment cannot contain '..'".into(),
        ));
    }
    Ok(value.to_string())
}

fn validate_resource_name(value: &str) -> Result<String, DavError> {
    let (stem, ext) = value.rsplit_once('.').ok_or_else(|| {
        DavError::UnsafePath("DAV resource name must include .ics or .vcf".into())
    })?;
    let ext = ext.to_ascii_lowercase();
    if ext != "ics" && ext != "vcf" {
        return Err(DavError::UnsafePath(
            "DAV resource name must end in .ics or .vcf".into(),
        ));
    }
    let stem = validate_segment(stem)?;
    Ok(format!("{stem}.{ext}"))
}

fn split_host_port(authority: &str) -> Result<(String, u16), DavError> {
    if authority.starts_with('[') {
        return Err(DavError::UnsafeUrl(
            "IPv6 DAV origins are not accepted".into(),
        ));
    }
    let (host, port) = if let Some((host, port)) = authority.rsplit_once(':') {
        let port = port
            .parse::<u16>()
            .map_err(|_| DavError::UnsafeUrl("DAV URL port is invalid".into()))?;
        (host, port)
    } else {
        (authority, 80)
    };
    if host.is_empty() || host.len() > 253 {
        return Err(DavError::UnsafeUrl("DAV URL host is invalid".into()));
    }
    if !host
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-'))
    {
        return Err(DavError::UnsafeUrl(
            "DAV URL host contains disallowed characters".into(),
        ));
    }
    Ok((host.to_ascii_lowercase(), port))
}

fn normalize_absolute_path(path: &str) -> Result<String, DavError> {
    if !path.starts_with('/') {
        return Err(DavError::UnsafePath(
            "DAV href must be an absolute path".into(),
        ));
    }
    if path.len() > MAX_HREF {
        return Err(DavError::UnsafePath(
            "DAV href exceeds the length bound".into(),
        ));
    }
    if path.contains('\\') || path.contains('\0') {
        return Err(DavError::UnsafePath(
            "DAV href contains disallowed characters".into(),
        ));
    }
    if path.chars().any(|ch| ch.is_control()) {
        return Err(DavError::UnsafePath(
            "DAV href contains control characters".into(),
        ));
    }
    let trailing_slash = path.ends_with('/') && path != "/";
    let mut normalized = Vec::new();
    for segment in path.split('/') {
        if segment.is_empty() || segment == "." {
            continue;
        }
        if segment == ".." {
            return Err(DavError::UnsafePath(
                "DAV href must not contain parent segments".into(),
            ));
        }
        if segment.contains('\\') {
            return Err(DavError::UnsafePath(
                "DAV href segment contains a backslash".into(),
            ));
        }
        normalized.push(segment);
    }
    let mut out = String::from("/");
    out.push_str(&normalized.join("/"));
    if trailing_slash && out != "/" {
        out.push('/');
    }
    if out.len() > MAX_HREF {
        return Err(DavError::UnsafePath(
            "DAV href exceeds the length bound".into(),
        ));
    }
    Ok(out)
}

pub fn percent_decode(input: &str) -> Result<String, DavError> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                if i + 2 >= bytes.len() {
                    return Err(DavError::UnsafePath("truncated percent-encoding".into()));
                }
                let hi = from_hex(bytes[i + 1])?;
                let lo = from_hex(bytes[i + 2])?;
                let value = (hi << 4) | lo;
                if value == 0 {
                    return Err(DavError::UnsafePath(
                        "percent-encoded NUL is not allowed".into(),
                    ));
                }
                out.push(value);
                i += 3;
            }
            byte if byte.is_ascii_control() => {
                return Err(DavError::UnsafePath(
                    "DAV href contains a control character".into(),
                ));
            }
            byte => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8(out).map_err(|_| DavError::UnsafePath("DAV href is not valid UTF-8".into()))
}

fn from_hex(byte: u8) -> Result<u8, DavError> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(DavError::UnsafePath(
            "invalid percent-encoding in DAV href".into(),
        )),
    }
}

pub fn encode_path(path: &str) -> String {
    path.split('/')
        .map(|segment| {
            if segment.is_empty() {
                String::new()
            } else {
                encode_segment(segment)
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn encode_segment(segment: &str) -> String {
    let mut out = String::with_capacity(segment.len());
    for byte in segment.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_paths_are_isolated() {
        let alice = UserPaths::for_user("alice").unwrap();
        let bob = UserPaths::for_user("bob").unwrap();
        let alice_cal = alice.collection(CollectionKind::Calendar, "cal-1").unwrap();
        assert!(bob.ensure_owned(&alice_cal).is_err());
        assert!(alice.ensure_owned(&alice_cal).is_ok());
        assert_eq!(alice_cal.as_str(), "/alice/calendars/cal-1/");
    }

    #[test]
    fn path_traversal_is_rejected() {
        assert!(DavHref::parse("/alice/../bob/").is_err());
        assert!(DavHref::parse("/alice/%2e%2e/bob/").is_err());
        assert!(validate_segment("..").is_err());
        assert!(validate_segment("a/b").is_err());
        assert!(DavHref::parse("/alice/\0hidden").is_err());
    }

    #[test]
    fn absolute_url_must_stay_on_origin() {
        let base = HttpUrl::parse("http://radicale:5232").unwrap();
        assert!(DavHref::from_dav(&base, "http://radicale:5232/alice/").is_ok());
        assert!(DavHref::from_dav(&base, "http://evil.example/alice/").is_err());
        assert!(DavHref::from_dav(&base, "https://radicale:5232/alice/").is_err());
    }

    #[test]
    fn encoded_slash_cannot_escape_segment() {
        assert!(validate_segment("a%2Fb").is_err());
        let href = DavHref::parse("/alice/calendars/id%2F../secret/").unwrap_err();
        assert!(matches!(href, DavError::UnsafePath(_)));
    }
}
