pub const MAX_FOLDER_NAME: usize = 80;
pub const MAX_BOOKMARK_TITLE: usize = 200;
pub const MAX_DESCRIPTION_BYTES: usize = 64 * 1024;
pub const MAX_URL_BYTES: usize = 2048;
pub const MAX_TAGS: usize = 16;
pub const MAX_TAG_LENGTH: usize = 32;

pub fn required_title(value: String, label: &str) -> Result<String, String> {
    let value = value.trim().to_string();
    if value.is_empty() {
        return Err(format!("{label} is required"));
    }
    if value.chars().count() > MAX_BOOKMARK_TITLE && label.contains("title") {
        return Err(format!("{label} is too long"));
    }
    if value.chars().count() > MAX_FOLDER_NAME && label.contains("Folder") {
        return Err(format!("{label} is too long"));
    }
    if value.contains('\0') || value.chars().any(char::is_control) {
        return Err(format!("{label} is invalid"));
    }
    Ok(value)
}

pub fn lossless_description(value: String) -> Result<String, String> {
    if value.len() > MAX_DESCRIPTION_BYTES {
        return Err("Description is too long".into());
    }
    if value.contains('\0') {
        return Err("Description cannot contain NUL bytes".into());
    }
    Ok(value)
}

pub fn validate_bookmark_url(value: &str) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err("URL is required".into());
    }
    if trimmed.len() > MAX_URL_BYTES {
        return Err("URL is too long".into());
    }
    if trimmed.contains('\0')
        || trimmed
            .chars()
            .any(|ch| ch.is_control() || ch.is_whitespace())
    {
        return Err("URL cannot contain whitespace or control characters".into());
    }
    let (scheme, rest) = if let Some(rest) = strip_prefix_ignore_ascii_case(trimmed, "https://") {
        ("https", rest)
    } else if let Some(rest) = strip_prefix_ignore_ascii_case(trimmed, "http://") {
        ("http", rest)
    } else {
        return Err("Only HTTP and HTTPS URLs are accepted".into());
    };
    if rest.is_empty() {
        return Err("URL must include a host".into());
    }
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    if authority.is_empty() {
        return Err("URL must include a host".into());
    }
    let hostport = authority
        .rsplit_once('@')
        .map(|(_, host)| host)
        .unwrap_or(authority);
    if hostport.is_empty() {
        return Err("URL must include a host".into());
    }
    let host = if let Some(inner) = hostport.strip_prefix('[') {
        let end = inner
            .find(']')
            .ok_or_else(|| "URL host is invalid".to_string())?;
        if inner[..end].is_empty() {
            return Err("URL must include a host".into());
        }
        &inner[..end]
    } else {
        hostport
            .split_once(':')
            .map(|(host, _)| host)
            .unwrap_or(hostport)
    };
    if host.is_empty() || host == "." || host == ".." {
        return Err("URL must include a host".into());
    }
    Ok(format!("{scheme}://{rest}"))
}

pub fn normalize_tags(values: &[String]) -> Result<Vec<String>, String> {
    let mut tags = Vec::new();
    for value in values {
        let tag = normalize_tag(value)?;
        if !tags.iter().any(|existing| existing == &tag) {
            tags.push(tag);
        }
    }
    if tags.len() > MAX_TAGS {
        return Err(format!("A bookmark may have at most {MAX_TAGS} tags"));
    }
    Ok(tags)
}

pub fn normalize_tag(value: &str) -> Result<String, String> {
    let collapsed = value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    if collapsed.is_empty() {
        return Err("Tags cannot be empty".into());
    }
    if collapsed.chars().count() > MAX_TAG_LENGTH {
        return Err(format!(
            "Each tag must be at most {MAX_TAG_LENGTH} characters"
        ));
    }
    if collapsed.contains('\0') || collapsed.chars().any(char::is_control) {
        return Err("Tags cannot contain control characters".into());
    }
    Ok(collapsed)
}

fn strip_prefix_ignore_ascii_case<'a>(value: &'a str, prefix: &str) -> Option<&'a str> {
    if value.len() >= prefix.len() && value[..prefix.len()].eq_ignore_ascii_case(prefix) {
        Some(&value[prefix.len()..])
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_accepts_only_http_and_https() {
        assert_eq!(
            validate_bookmark_url("  HTTPS://Example.COM/path?q=1  ").unwrap(),
            "https://Example.COM/path?q=1"
        );
        assert!(validate_bookmark_url("javascript:alert(1)").is_err());
        assert!(validate_bookmark_url("ftp://example.com").is_err());
        assert!(validate_bookmark_url("https:///no-host").is_err());
    }

    #[test]
    fn tags_are_normalized_and_deduplicated() {
        assert_eq!(
            normalize_tags(&["  Work  ".into(), "WORK".into(), "docs".into()]).unwrap(),
            vec!["work", "docs"]
        );
    }

    #[test]
    fn description_is_lossless() {
        let description = "Keep <em>html</em>\n";
        assert_eq!(
            lossless_description(description.into()).unwrap(),
            description
        );
    }
}
