use super::config::DavConfig;
use super::error::{DavError, classify_http};
use super::path::{CollectionKind, DavHref, HttpUrl};
use super::xml::{XmlElement, XmlLimits, escape_xml, parse_xml};

pub const NS_DAV: &str = "DAV:";
pub const NS_CALDAV: &str = "urn:ietf:params:xml:ns:caldav";
pub const NS_CARDDAV: &str = "urn:ietf:params:xml:ns:carddav";
pub const XML_CONTENT_TYPE: &str = "application/xml; charset=utf-8";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ETag(String);

impl ETag {
    pub fn parse(raw: &str) -> Result<Self, DavError> {
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.len() > 256 {
            return Err(DavError::MalformedRemote(
                "ETag is empty or too long".into(),
            ));
        }
        if trimmed.chars().any(|ch| matches!(ch, '\r' | '\n' | '\0')) {
            return Err(DavError::MalformedRemote(
                "ETag contains control characters".into(),
            ));
        }
        Ok(Self(trimmed.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn header_value(&self) -> String {
        if self.0.starts_with('"') || self.0.starts_with("W/") {
            self.0.clone()
        } else {
            format!("\"{}\"", self.0.replace('"', "\\\""))
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SyncToken(String);

impl SyncToken {
    pub fn parse(raw: &str) -> Result<Self, DavError> {
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.len() > 1024 {
            return Err(DavError::MalformedRemote(
                "sync-token is empty or too long".into(),
            ));
        }
        if trimmed.chars().any(|ch| matches!(ch, '\r' | '\n' | '\0')) {
            return Err(DavError::MalformedRemote(
                "sync-token contains control characters".into(),
            ));
        }
        Ok(Self(trimmed.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MultiStatus {
    pub responses: Vec<PropResponse>,
    pub sync_token: Option<SyncToken>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PropResponse {
    pub href: DavHref,
    pub status: u16,
    pub etag: Option<ETag>,
    pub display_name: Option<String>,
    pub resource_type: ResourceType,
    pub content_type: Option<String>,
    pub calendar_data: Option<String>,
    pub address_data: Option<String>,
    pub current_user_principal: Option<DavHref>,
    pub calendar_home_set: Option<DavHref>,
    pub addressbook_home_set: Option<DavHref>,
    pub supported_components: Vec<String>,
    pub sync_token: Option<SyncToken>,
    pub error_name: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResourceType {
    Principal,
    Calendar,
    AddressBook,
    Collection,
    Resource,
}

impl ResourceType {
    pub fn inferred_kind(self, components: &[String], href: &DavHref) -> Option<CollectionKind> {
        match self {
            Self::AddressBook => Some(CollectionKind::AddressBook),
            Self::Calendar => {
                if href.as_str().contains("/tasks/")
                    || (components.iter().any(|c| c.eq_ignore_ascii_case("VTODO"))
                        && !components.iter().any(|c| c.eq_ignore_ascii_case("VEVENT")))
                {
                    Some(CollectionKind::TaskList)
                } else {
                    Some(CollectionKind::Calendar)
                }
            }
            _ => None,
        }
    }
}

pub fn propfind_discovery() -> String {
    concat!(
        r#"<?xml version="1.0" encoding="utf-8"?>"#,
        r#"<d:propfind xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav" xmlns:cr="urn:ietf:params:xml:ns:carddav">"#,
        r#"<d:prop>"#,
        r#"<d:current-user-principal/>"#,
        r#"<d:resourcetype/>"#,
        r#"<d:displayname/>"#,
        r#"<d:getetag/>"#,
        r#"<d:sync-token/>"#,
        r#"<c:calendar-home-set/>"#,
        r#"<cr:addressbook-home-set/>"#,
        r#"<c:supported-calendar-component-set/>"#,
        r#"</d:prop>"#,
        r#"</d:propfind>"#
    )
    .into()
}

pub fn propfind_collection() -> String {
    concat!(
        r#"<?xml version="1.0" encoding="utf-8"?>"#,
        r#"<d:propfind xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav" xmlns:cr="urn:ietf:params:xml:ns:carddav">"#,
        r#"<d:prop>"#,
        r#"<d:resourcetype/>"#,
        r#"<d:displayname/>"#,
        r#"<d:getetag/>"#,
        r#"<d:sync-token/>"#,
        r#"<d:getcontenttype/>"#,
        r#"<c:supported-calendar-component-set/>"#,
        r#"</d:prop>"#,
        r#"</d:propfind>"#
    )
    .into()
}

pub fn mkcalendar_xml(display_name: &str, components: &[&str]) -> String {
    let comps = components
        .iter()
        .map(|name| format!(r#"<c:comp name="{}"/>"#, escape_xml(name)))
        .collect::<String>();
    format!(
        concat!(
            r#"<?xml version="1.0" encoding="utf-8"?>"#,
            r#"<c:mkcalendar xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">"#,
            r#"<d:set><d:prop>"#,
            r#"<d:displayname>{}</d:displayname>"#,
            r#"<c:supported-calendar-component-set>{}</c:supported-calendar-component-set>"#,
            r#"</d:prop></d:set></c:mkcalendar>"#
        ),
        escape_xml(display_name),
        comps
    )
}

pub fn mkcol_addressbook_xml(display_name: &str) -> String {
    format!(
        concat!(
            r#"<?xml version="1.0" encoding="utf-8"?>"#,
            r#"<d:mkcol xmlns:d="DAV:" xmlns:cr="urn:ietf:params:xml:ns:carddav">"#,
            r#"<d:set><d:prop>"#,
            r#"<d:resourcetype><d:collection/><cr:addressbook/></d:resourcetype>"#,
            r#"<d:displayname>{}</d:displayname>"#,
            r#"</d:prop></d:set></d:mkcol>"#
        ),
        escape_xml(display_name)
    )
}

pub fn mkcol_xml() -> String {
    concat!(
        r#"<?xml version="1.0" encoding="utf-8"?>"#,
        r#"<d:mkcol xmlns:d="DAV:"><d:set><d:prop>"#,
        r#"<d:resourcetype><d:collection/></d:resourcetype>"#,
        r#"</d:prop></d:set></d:mkcol>"#
    )
    .into()
}

pub fn sync_collection_xml(token: Option<&SyncToken>) -> String {
    let token_xml = match token {
        Some(token) => format!(
            "<d:sync-token>{}</d:sync-token>",
            escape_xml(token.as_str())
        ),
        None => "<d:sync-token/>".into(),
    };
    format!(
        concat!(
            r#"<?xml version="1.0" encoding="utf-8"?>"#,
            r#"<d:sync-collection xmlns:d="DAV:">"#,
            "{}",
            r#"<d:sync-level>1</d:sync-level>"#,
            r#"<d:prop><d:getetag/><d:getcontenttype/><d:resourcetype/></d:prop>"#,
            r#"</d:sync-collection>"#
        ),
        token_xml
    )
}

pub fn calendar_multiget_xml(hrefs: &[DavHref]) -> String {
    multiget_xml(
        "c:calendar-multiget",
        r#"xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav""#,
        "<c:calendar-data/>",
        hrefs,
    )
}

pub fn addressbook_multiget_xml(hrefs: &[DavHref]) -> String {
    multiget_xml(
        "cr:addressbook-multiget",
        r#"xmlns:d="DAV:" xmlns:cr="urn:ietf:params:xml:ns:carddav""#,
        "<cr:address-data/>",
        hrefs,
    )
}

pub fn proppatch_xml(display_name: &str, calendar_order: Option<i32>) -> String {
    let order = match calendar_order {
        Some(order) => format!(
            r#"<a:calendar-order xmlns:a="http://apple.com/ns/ical/">{order}</a:calendar-order>"#
        ),
        None => String::new(),
    };
    format!(
        concat!(
            r#"<?xml version="1.0" encoding="utf-8"?>"#,
            r#"<d:propertyupdate xmlns:d="DAV:">"#,
            r#"<d:set><d:prop>"#,
            r#"<d:displayname>{}</d:displayname>"#,
            "{}",
            r#"</d:prop></d:set></d:propertyupdate>"#
        ),
        escape_xml(display_name),
        order
    )
}

fn multiget_xml(root: &str, xmlns: &str, data_prop: &str, hrefs: &[DavHref]) -> String {
    let href_xml = hrefs
        .iter()
        .map(|href| format!("<d:href>{}</d:href>", escape_xml(href.as_str())))
        .collect::<String>();
    format!(
        concat!(
            r#"<?xml version="1.0" encoding="utf-8"?>"#,
            "<{root} {xmlns}>",
            "<d:prop><d:getetag/><d:getcontenttype/>{data}</d:prop>",
            "{hrefs}",
            "</{root}>"
        ),
        root = root,
        xmlns = xmlns,
        data = data_prop,
        hrefs = href_xml
    )
}

pub fn parse_multistatus(
    body: &[u8],
    config: &DavConfig,
    base: &HttpUrl,
) -> Result<MultiStatus, DavError> {
    let root = parse_xml(body, xml_limits(config))?;
    if !root.name.is(NS_DAV, "multistatus") {
        return Err(DavError::MalformedRemote(
            "DAV response root is not D:multistatus".into(),
        ));
    }
    let mut responses = Vec::new();
    for response in root.children(NS_DAV, "response") {
        responses.push(parse_response(response, base)?);
    }
    let sync_token = match root.child(NS_DAV, "sync-token") {
        Some(node) => Some(SyncToken::parse(&node.text())?),
        None => None,
    };
    Ok(MultiStatus {
        responses,
        sync_token,
    })
}

pub fn extract_dav_error(body: &[u8], config: &DavConfig) -> Option<String> {
    let root = parse_xml(body, xml_limits(config)).ok()?;
    let error = if root.name.is(NS_DAV, "error") {
        &root
    } else {
        root.child(NS_DAV, "error")?
    };
    error.children.iter().find_map(|node| match node {
        super::xml::XmlNode::Element(element) => Some(element.name.local.clone()),
        _ => None,
    })
}

pub fn classify_response(status: u16, body: &[u8], config: &DavConfig) -> DavError {
    let token = extract_dav_error(body, config);
    let hint = std::str::from_utf8(body).unwrap_or("").trim();
    classify_http(status, token.as_deref(), hint)
}

fn parse_response(response: &XmlElement, base: &HttpUrl) -> Result<PropResponse, DavError> {
    let href_text = response
        .child(NS_DAV, "href")
        .map(XmlElement::text)
        .ok_or_else(|| DavError::MalformedRemote("DAV response is missing href".into()))?;
    let href = DavHref::from_dav(base, href_text.trim())?;
    let mut parsed = PropResponse {
        href,
        status: response
            .child(NS_DAV, "status")
            .map(|node| parse_status(&node.text()))
            .transpose()?
            .unwrap_or(207),
        etag: None,
        display_name: None,
        resource_type: ResourceType::Resource,
        content_type: None,
        calendar_data: None,
        address_data: None,
        current_user_principal: None,
        calendar_home_set: None,
        addressbook_home_set: None,
        supported_components: Vec::new(),
        sync_token: None,
        error_name: None,
    };
    if let Some(error) = response.child(NS_DAV, "error") {
        parsed.error_name = error.children.iter().find_map(|node| match node {
            super::xml::XmlNode::Element(element) => Some(element.name.local.clone()),
            _ => None,
        });
    }
    for propstat in response.children(NS_DAV, "propstat") {
        let status = propstat
            .child(NS_DAV, "status")
            .map(|node| parse_status(&node.text()))
            .transpose()?
            .unwrap_or(200);
        if !(200..300).contains(&status) {
            if parsed.status == 207 {
                parsed.status = status;
            }
            if let Some(error) = propstat.child(NS_DAV, "error") {
                parsed.error_name = error.children.iter().find_map(|node| match node {
                    super::xml::XmlNode::Element(element) => Some(element.name.local.clone()),
                    _ => None,
                });
            }
            continue;
        }
        parsed.status = status;
        let Some(prop) = propstat.child(NS_DAV, "prop") else {
            continue;
        };
        if let Some(etag) = prop.child(NS_DAV, "getetag") {
            let text = etag.text();
            if !text.trim().is_empty() {
                parsed.etag = Some(ETag::parse(&text)?);
            }
        }
        if let Some(name) = prop.child(NS_DAV, "displayname") {
            let text = name.text();
            if !text.trim().is_empty() {
                parsed.display_name = Some(text);
            }
        }
        if let Some(content_type) = prop.child(NS_DAV, "getcontenttype") {
            parsed.content_type = Some(content_type.text());
        }
        if let Some(token) = prop.child(NS_DAV, "sync-token") {
            let text = token.text();
            if !text.trim().is_empty() {
                parsed.sync_token = Some(SyncToken::parse(&text)?);
            }
        }
        if let Some(data) = prop.child(NS_CALDAV, "calendar-data") {
            parsed.calendar_data = Some(data.text());
        }
        if let Some(data) = prop.child(NS_CARDDAV, "address-data") {
            parsed.address_data = Some(data.text());
        }
        if let Some(set) = prop.child(NS_CALDAV, "calendar-home-set")
            && let Some(href) = set.child(NS_DAV, "href")
        {
            parsed.calendar_home_set = Some(DavHref::from_dav(base, href.text().trim())?);
        }
        if let Some(set) = prop.child(NS_CARDDAV, "addressbook-home-set")
            && let Some(href) = set.child(NS_DAV, "href")
        {
            parsed.addressbook_home_set = Some(DavHref::from_dav(base, href.text().trim())?);
        }
        if let Some(principal) = prop.child(NS_DAV, "current-user-principal")
            && let Some(href) = principal.child(NS_DAV, "href")
        {
            parsed.current_user_principal = Some(DavHref::from_dav(base, href.text().trim())?);
        }
        if let Some(components) = prop.child(NS_CALDAV, "supported-calendar-component-set") {
            parsed.supported_components = components
                .children(NS_CALDAV, "comp")
                .filter_map(|comp| comp.attr("", "name").map(ToOwned::to_owned))
                .collect();
        }
        if let Some(resource_type) = prop.child(NS_DAV, "resourcetype") {
            parsed.resource_type = classify_resource_type(resource_type);
        }
    }
    Ok(parsed)
}

fn classify_resource_type(resource_type: &XmlElement) -> ResourceType {
    let mut collection = false;
    let mut calendar = false;
    let mut addressbook = false;
    let mut principal = false;
    for child in &resource_type.children {
        let super::xml::XmlNode::Element(element) = child else {
            continue;
        };
        if element.name.is(NS_DAV, "collection") {
            collection = true;
        } else if element.name.is(NS_DAV, "principal") {
            principal = true;
        } else if element.name.is(NS_CALDAV, "calendar") {
            calendar = true;
        } else if element.name.is(NS_CARDDAV, "addressbook") {
            addressbook = true;
        }
    }
    if addressbook {
        ResourceType::AddressBook
    } else if calendar {
        ResourceType::Calendar
    } else if principal {
        ResourceType::Principal
    } else if collection {
        ResourceType::Collection
    } else {
        ResourceType::Resource
    }
}

fn parse_status(value: &str) -> Result<u16, DavError> {
    value
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .or_else(|| value.trim().parse().ok())
        .ok_or_else(|| DavError::MalformedRemote(format!("invalid DAV status {value:?}")))
}

fn xml_limits(config: &DavConfig) -> XmlLimits {
    XmlLimits {
        max_bytes: config.max_xml_bytes,
        max_depth: config.max_xml_depth,
        max_elements: config.max_xml_elements,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> DavConfig {
        DavConfig::new("http://127.0.0.1:5232", "foyer", "secret").unwrap()
    }

    #[test]
    fn parses_sync_collection_multistatus() {
        let xml = br#"<?xml version="1.0" encoding="utf-8"?>
        <d:multistatus xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">
          <d:response>
            <d:href>/alice/calendars/home/event.ics</d:href>
            <d:propstat>
              <d:prop>
                <d:getetag>"etag-1"</d:getetag>
                <d:resourcetype/>
              </d:prop>
              <d:status>HTTP/1.1 200 OK</d:status>
            </d:propstat>
          </d:response>
          <d:response>
            <d:href>/alice/calendars/home/gone.ics</d:href>
            <d:status>HTTP/1.1 404 Not Found</d:status>
          </d:response>
          <d:sync-token>http://radicale.org/ns/sync/abc</d:sync-token>
        </d:multistatus>"#;
        let parsed = parse_multistatus(
            xml,
            &config(),
            &HttpUrl::parse("http://127.0.0.1:5232").unwrap(),
        )
        .unwrap();
        assert_eq!(parsed.responses.len(), 2);
        assert_eq!(
            parsed.responses[0].etag.as_ref().unwrap().as_str(),
            "\"etag-1\""
        );
        assert_eq!(parsed.responses[1].status, 404);
        assert_eq!(
            parsed.sync_token.unwrap().as_str(),
            "http://radicale.org/ns/sync/abc"
        );
    }

    #[test]
    fn mkcalendar_escapes_display_name() {
        let xml = mkcalendar_xml("Work <Home>", &["VEVENT"]);
        assert!(xml.contains("Work &lt;Home&gt;"));
        assert!(xml.contains(r#"<c:comp name="VEVENT"/>"#));
    }
}
