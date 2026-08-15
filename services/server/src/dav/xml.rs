use super::error::DavError;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct XmlName {
    pub namespace: String,
    pub local: String,
}

impl XmlName {
    pub fn new(namespace: impl Into<String>, local: impl Into<String>) -> Self {
        Self {
            namespace: namespace.into(),
            local: local.into(),
        }
    }

    pub fn is(&self, namespace: &str, local: &str) -> bool {
        self.namespace == namespace && self.local == local
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum XmlNode {
    Element(XmlElement),
    Text(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct XmlElement {
    pub name: XmlName,
    pub attributes: Vec<(XmlName, String)>,
    pub children: Vec<XmlNode>,
}

impl XmlElement {
    pub fn new(namespace: &str, local: &str) -> Self {
        Self {
            name: XmlName::new(namespace, local),
            attributes: Vec::new(),
            children: Vec::new(),
        }
    }

    pub fn child(&self, namespace: &str, local: &str) -> Option<&XmlElement> {
        self.children.iter().find_map(|node| match node {
            XmlNode::Element(element) if element.name.is(namespace, local) => Some(element),
            _ => None,
        })
    }

    pub fn children<'a>(
        &'a self,
        namespace: &'a str,
        local: &'a str,
    ) -> impl Iterator<Item = &'a XmlElement> + 'a {
        self.children.iter().filter_map(move |node| match node {
            XmlNode::Element(element) if element.name.is(namespace, local) => Some(element),
            _ => None,
        })
    }

    pub fn text(&self) -> String {
        let mut out = String::new();
        collect_text(self, &mut out);
        out
    }

    pub fn attr(&self, namespace: &str, local: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|(name, _)| name.is(namespace, local))
            .map(|(_, value)| value.as_str())
    }
}

#[derive(Clone, Copy, Debug)]
pub struct XmlLimits {
    pub max_bytes: usize,
    pub max_depth: usize,
    pub max_elements: usize,
}

pub fn parse_xml(input: &[u8], limits: XmlLimits) -> Result<XmlElement, DavError> {
    if input.len() > limits.max_bytes {
        return Err(DavError::XmlBound {
            detail: format!("document is {} bytes", input.len()),
        });
    }
    let text = std::str::from_utf8(input)
        .map_err(|_| DavError::MalformedRemote("DAV XML is not valid UTF-8".into()))?;
    let mut parser = Parser {
        input: text,
        pos: 0,
        limits,
        elements: 0,
    };
    parser.skip_bom();
    parser.skip_misc(true)?;
    let root = parser.parse_element(0, &NsScope::default())?;
    parser.skip_misc(false)?;
    if !parser.eof() {
        return Err(DavError::MalformedRemote(
            "DAV XML has trailing data after the root element".into(),
        ));
    }
    Ok(root)
}

pub fn escape_xml(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            other => out.push(other),
        }
    }
    out
}

struct Parser<'a> {
    input: &'a str,
    pos: usize,
    limits: XmlLimits,
    elements: usize,
}

#[derive(Clone, Default)]
struct NsScope {
    default: String,
    prefixes: Vec<(String, String)>,
}

impl NsScope {
    fn resolve(&self, prefix: Option<&str>) -> String {
        match prefix {
            None => self.default.clone(),
            Some("xml") => "http://www.w3.org/XML/1998/namespace".into(),
            Some("xmlns") => "http://www.w3.org/2000/xmlns/".into(),
            Some(prefix) => self
                .prefixes
                .iter()
                .rev()
                .find(|(name, _)| name == prefix)
                .map(|(_, uri)| uri.clone())
                .unwrap_or_default(),
        }
    }

    fn with_declarations(&self, attrs: &[(String, String)]) -> Self {
        let mut next = self.clone();
        for (name, value) in attrs {
            if name == "xmlns" {
                next.default = value.clone();
            } else if let Some(prefix) = name.strip_prefix("xmlns:") {
                next.prefixes.push((prefix.to_string(), value.clone()));
            }
        }
        next
    }
}

impl<'a> Parser<'a> {
    fn eof(&self) -> bool {
        self.pos >= self.input.len()
    }

    fn rest(&self) -> &'a str {
        &self.input[self.pos..]
    }

    fn skip_bom(&mut self) {
        if self.rest().starts_with('\u{feff}') {
            self.pos += '\u{feff}'.len_utf8();
        }
    }

    fn skip_ws(&mut self) {
        while let Some(ch) = self.rest().chars().next() {
            if ch.is_ascii_whitespace() {
                self.pos += ch.len_utf8();
            } else {
                break;
            }
        }
    }

    fn starts_with(&self, token: &str) -> bool {
        self.rest().starts_with(token)
    }

    fn bump(&mut self, n: usize) {
        self.pos += n;
    }

    fn skip_misc(&mut self, allow_declaration: bool) -> Result<(), DavError> {
        loop {
            self.skip_ws();
            if allow_declaration && self.starts_with("<?xml") {
                self.skip_until("?>")?;
                continue;
            }
            if self.starts_with("<?") {
                return Err(DavError::MalformedRemote(
                    "DAV XML processing instructions are not accepted".into(),
                ));
            }
            if self.starts_with("<!--") {
                self.skip_until("-->")?;
                continue;
            }
            if self.starts_with("<!DOCTYPE")
                || self.starts_with("<!ENTITY")
                || self.starts_with("<!")
            {
                return Err(DavError::MalformedRemote(
                    "DAV XML must not contain a DTD or entity declaration".into(),
                ));
            }
            break;
        }
        Ok(())
    }

    fn skip_until(&mut self, end: &str) -> Result<(), DavError> {
        if let Some(index) = self.rest().find(end) {
            self.pos += index + end.len();
            Ok(())
        } else {
            Err(DavError::MalformedRemote(
                "unterminated DAV XML construct".into(),
            ))
        }
    }

    fn parse_element(&mut self, depth: usize, parent_ns: &NsScope) -> Result<XmlElement, DavError> {
        if depth > self.limits.max_depth {
            return Err(DavError::XmlBound {
                detail: format!("element depth exceeded {}", self.limits.max_depth),
            });
        }
        self.elements += 1;
        if self.elements > self.limits.max_elements {
            return Err(DavError::XmlBound {
                detail: format!("element count exceeded {}", self.limits.max_elements),
            });
        }
        self.expect('<')?;
        if self.starts_with("/") || self.starts_with("!") || self.starts_with("?") {
            return Err(DavError::MalformedRemote(
                "DAV XML element name is invalid".into(),
            ));
        }
        let raw_name = self.parse_name()?;
        let raw_attrs = self.parse_raw_attributes()?;
        let ns = parent_ns.with_declarations(&raw_attrs);
        let name = qualify(&raw_name, &ns, false);
        let mut attributes = Vec::new();
        for (raw, value) in &raw_attrs {
            if raw == "xmlns" || raw.starts_with("xmlns:") {
                continue;
            }
            attributes.push((qualify(raw, &ns, true), value.clone()));
        }
        self.skip_ws();
        if self.starts_with("/>") {
            self.bump(2);
            return Ok(XmlElement {
                name,
                attributes,
                children: Vec::new(),
            });
        }
        self.expect('>')?;
        let mut children = Vec::new();
        loop {
            if self.eof() {
                return Err(DavError::MalformedRemote(
                    "unterminated DAV XML element".into(),
                ));
            }
            if self.starts_with("</") {
                break;
            }
            if self.starts_with("<!--") {
                self.skip_until("-->")?;
                continue;
            }
            if self.starts_with("<![CDATA[") {
                self.bump("<![CDATA[".len());
                let Some(end) = self.rest().find("]]>") else {
                    return Err(DavError::MalformedRemote("unterminated CDATA".into()));
                };
                children.push(XmlNode::Text(self.rest()[..end].to_string()));
                self.pos += end + 3;
                continue;
            }
            if self.starts_with("<") {
                children.push(XmlNode::Element(self.parse_element(depth + 1, &ns)?));
                continue;
            }
            let Some(next_lt) = self.rest().find('<') else {
                return Err(DavError::MalformedRemote(
                    "unterminated DAV XML element".into(),
                ));
            };
            let text = unescape(&self.rest()[..next_lt])?;
            self.pos += next_lt;
            if !text.is_empty() {
                children.push(XmlNode::Text(text));
            }
        }
        self.bump(2);
        let closing = self.parse_name()?;
        if closing != raw_name {
            return Err(DavError::MalformedRemote(format!(
                "DAV XML closing tag {closing} does not match {raw_name}"
            )));
        }
        self.skip_ws();
        self.expect('>')?;
        Ok(XmlElement {
            name,
            attributes,
            children,
        })
    }

    fn parse_name(&mut self) -> Result<String, DavError> {
        let rest = self.rest();
        let len = rest
            .chars()
            .take_while(|ch| is_name_char(*ch))
            .map(char::len_utf8)
            .sum();
        if len == 0 {
            return Err(DavError::MalformedRemote("DAV XML name is missing".into()));
        }
        let name = rest[..len].to_string();
        self.pos += len;
        Ok(name)
    }

    fn parse_raw_attributes(&mut self) -> Result<Vec<(String, String)>, DavError> {
        let mut attrs = Vec::new();
        loop {
            self.skip_ws();
            if self.starts_with("/>") || self.starts_with(">") || self.eof() {
                break;
            }
            if attrs.len() >= 64 {
                return Err(DavError::XmlBound {
                    detail: "attribute count exceeded 64".into(),
                });
            }
            let name = self.parse_name()?;
            self.skip_ws();
            self.expect('=')?;
            self.skip_ws();
            let value = self.parse_quoted()?;
            attrs.push((name, value));
        }
        Ok(attrs)
    }

    fn parse_quoted(&mut self) -> Result<String, DavError> {
        let quote = self
            .rest()
            .chars()
            .next()
            .ok_or_else(|| DavError::MalformedRemote("unterminated attribute value".into()))?;
        if quote != '"' && quote != '\'' {
            return Err(DavError::MalformedRemote(
                "attribute value must be quoted".into(),
            ));
        }
        self.bump(quote.len_utf8());
        let rest = self.rest();
        let Some(end) = rest.find(quote) else {
            return Err(DavError::MalformedRemote(
                "unterminated attribute value".into(),
            ));
        };
        let value = unescape(&rest[..end])?;
        self.pos += end + quote.len_utf8();
        Ok(value)
    }

    fn expect(&mut self, expected: char) -> Result<(), DavError> {
        if self.rest().starts_with(expected) {
            self.pos += expected.len_utf8();
            Ok(())
        } else {
            Err(DavError::MalformedRemote(format!(
                "expected {expected:?} in DAV XML"
            )))
        }
    }
}

fn qualify(raw: &str, ns: &NsScope, is_attr: bool) -> XmlName {
    if let Some((prefix, local)) = raw.split_once(':') {
        XmlName::new(ns.resolve(Some(prefix)), local)
    } else if is_attr {
        XmlName::new("", raw)
    } else {
        XmlName::new(ns.resolve(None), raw)
    }
}

fn collect_text(element: &XmlElement, out: &mut String) {
    for child in &element.children {
        match child {
            XmlNode::Text(text) => out.push_str(text),
            XmlNode::Element(child) => collect_text(child, out),
        }
    }
}

fn is_name_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, ':' | '_' | '-' | '.')
}

fn unescape(input: &str) -> Result<String, DavError> {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(start) = rest.find('&') {
        out.push_str(&rest[..start]);
        rest = &rest[start + 1..];
        let Some(end) = rest.find(';') else {
            return Err(DavError::MalformedRemote("unterminated XML entity".into()));
        };
        let entity = &rest[..end];
        rest = &rest[end + 1..];
        match entity {
            "amp" => out.push('&'),
            "lt" => out.push('<'),
            "gt" => out.push('>'),
            "quot" => out.push('"'),
            "apos" => out.push('\''),
            other
                if let Some(digits) = other
                    .strip_prefix("#x")
                    .or_else(|| other.strip_prefix("#X")) =>
            {
                push_codepoint(&mut out, u32::from_str_radix(digits, 16).ok())?;
            }
            other if let Some(digits) = other.strip_prefix('#') => {
                push_codepoint(&mut out, digits.parse().ok())?;
            }
            _ => {
                return Err(DavError::MalformedRemote(
                    "DAV XML contains an unsupported entity".into(),
                ));
            }
        }
    }
    out.push_str(rest);
    Ok(out)
}

fn push_codepoint(out: &mut String, code: Option<u32>) -> Result<(), DavError> {
    let Some(code) = code else {
        return Err(DavError::MalformedRemote(
            "DAV XML numeric entity is invalid".into(),
        ));
    };
    if code == 0 {
        return Err(DavError::MalformedRemote(
            "DAV XML numeric entity cannot be NUL".into(),
        ));
    }
    let Some(ch) = char::from_u32(code) else {
        return Err(DavError::MalformedRemote(
            "DAV XML numeric entity is not a Unicode scalar".into(),
        ));
    };
    out.push(ch);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limits() -> XmlLimits {
        XmlLimits {
            max_bytes: 8_192,
            max_depth: 8,
            max_elements: 64,
        }
    }

    #[test]
    fn parses_namespaced_multistatus_fragment() {
        let xml = br#"<d:prop xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">
            <d:getetag>&quot;abc&quot;</d:getetag>
            <c:calendar-data>BEGIN:VCALENDAR</c:calendar-data>
        </d:prop>"#;
        let root = parse_xml(xml, limits()).unwrap();
        assert!(root.name.is("DAV:", "prop"));
        assert_eq!(root.child("DAV:", "getetag").unwrap().text(), "\"abc\"");
        assert_eq!(
            root.child("urn:ietf:params:xml:ns:caldav", "calendar-data")
                .unwrap()
                .text(),
            "BEGIN:VCALENDAR"
        );
    }

    #[test]
    fn rejects_doctype_and_external_entities() {
        let xml = br#"<!DOCTYPE foo [<!ENTITY xxe SYSTEM "file:///etc/passwd">]><foo>&xxe;</foo>"#;
        assert!(parse_xml(xml, limits()).is_err());
    }

    #[test]
    fn enforces_depth_and_size_bounds() {
        let deep = "<a>".repeat(10) + &"</a>".repeat(10);
        assert!(matches!(
            parse_xml(deep.as_bytes(), limits()),
            Err(DavError::XmlBound { .. })
        ));
        let huge = "a".repeat(9_000);
        assert!(matches!(
            parse_xml(huge.as_bytes(), limits()),
            Err(DavError::XmlBound { .. })
        ));
    }
}
