use super::config::MAX_RESOURCE_BYTES;
use super::error::DavError;
use super::path::CollectionKind;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DavMediaType {
    ICalendar,
    VCard,
}

impl DavMediaType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ICalendar => "text/calendar; charset=utf-8",
            Self::VCard => "text/vcard; charset=utf-8",
        }
    }

    pub fn from_content_type(value: Option<&str>, kind: Option<CollectionKind>) -> Self {
        let lowered = value.unwrap_or("").to_ascii_lowercase();
        if lowered.contains("vcard") {
            Self::VCard
        } else if lowered.contains("calendar") {
            Self::ICalendar
        } else if kind == Some(CollectionKind::AddressBook) {
            Self::VCard
        } else {
            Self::ICalendar
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ComponentKind {
    Event,
    Todo,
    Journal,
    Card,
}

impl ComponentKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Event => "VEVENT",
            Self::Todo => "VTODO",
            Self::Journal => "VJOURNAL",
            Self::Card => "VCARD",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PropertyUpdate {
    pub name: String,
    pub value: Option<String>,
}

impl PropertyUpdate {
    pub fn set(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: Some(value.into()),
        }
    }

    pub fn remove(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DavPayload {
    media_type: DavMediaType,
    raw: String,
}

impl DavPayload {
    pub fn from_raw(media_type: DavMediaType, raw: impl Into<String>) -> Result<Self, DavError> {
        let raw = raw.into();
        validate_raw(&raw)?;
        match media_type {
            DavMediaType::ICalendar if !contains_begin(&raw, "VCALENDAR") => {
                return Err(DavError::InvalidRequest(
                    "iCalendar payload must contain BEGIN:VCALENDAR".into(),
                ));
            }
            DavMediaType::VCard if !contains_begin(&raw, "VCARD") => {
                return Err(DavError::InvalidRequest(
                    "vCard payload must contain BEGIN:VCARD".into(),
                ));
            }
            _ => {}
        }
        Ok(Self { media_type, raw })
    }

    pub fn media_type(&self) -> DavMediaType {
        self.media_type
    }

    pub fn raw(&self) -> &str {
        &self.raw
    }

    pub fn into_raw(self) -> String {
        self.raw
    }

    pub fn uid(&self) -> Option<String> {
        first_property(&unfold(&self.raw), "UID")
    }

    pub fn component_kind(&self) -> Option<ComponentKind> {
        let upper = self.raw.to_ascii_uppercase();
        if upper.contains("BEGIN:VEVENT") {
            Some(ComponentKind::Event)
        } else if upper.contains("BEGIN:VTODO") {
            Some(ComponentKind::Todo)
        } else if upper.contains("BEGIN:VJOURNAL") {
            Some(ComponentKind::Journal)
        } else if upper.contains("BEGIN:VCARD") {
            Some(ComponentKind::Card)
        } else {
            None
        }
    }

    pub fn patch(&self, updates: &[PropertyUpdate]) -> Result<Self, DavError> {
        if updates.is_empty() {
            return Ok(self.clone());
        }
        for update in updates {
            validate_property_name(&update.name)?;
            if let Some(value) = &update.value {
                validate_property_value(value)?;
            }
        }
        let lines = unfold(&self.raw);
        let (start, end) = target_component_range(&lines, self.media_type)?;
        let mut rebuilt = lines[..start].to_vec();
        let mut component = lines[start..end].to_vec();
        for update in updates {
            apply_update(&mut component, update);
        }
        rebuilt.extend(component);
        rebuilt.extend(lines[end..].iter().cloned());
        Self::from_raw(self.media_type, fold(&rebuilt))
    }

    pub fn semantically_eq(&self, other: &Self) -> bool {
        self.media_type == other.media_type && unfold(&self.raw) == unfold(&other.raw)
    }
}

fn validate_raw(raw: &str) -> Result<(), DavError> {
    if raw.len() > MAX_RESOURCE_BYTES {
        return Err(DavError::InvalidRequest(format!(
            "DAV payload exceeds {MAX_RESOURCE_BYTES} bytes"
        )));
    }
    if raw.contains('\0') {
        return Err(DavError::InvalidRequest(
            "DAV payload cannot contain NUL bytes".into(),
        ));
    }
    Ok(())
}

fn validate_property_name(name: &str) -> Result<(), DavError> {
    if name.is_empty()
        || name.len() > 64
        || !name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
    {
        return Err(DavError::InvalidRequest(
            "DAV property name is invalid".into(),
        ));
    }
    Ok(())
}

fn validate_property_value(value: &str) -> Result<(), DavError> {
    if value.len() > MAX_RESOURCE_BYTES {
        return Err(DavError::InvalidRequest(
            "DAV property value exceeds the payload bound".into(),
        ));
    }
    if value.contains('\0') {
        return Err(DavError::InvalidRequest(
            "DAV property value cannot contain NUL bytes".into(),
        ));
    }
    Ok(())
}

fn contains_begin(raw: &str, name: &str) -> bool {
    unfold(raw)
        .iter()
        .any(|line| line.eq_ignore_ascii_case(&format!("BEGIN:{name}")))
}

fn unfold(raw: &str) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for line in raw.replace("\r\n", "\n").replace('\r', "\n").lines() {
        if let Some(rest) = line.strip_prefix([' ', '\t'])
            && let Some(last) = lines.last_mut()
        {
            last.push_str(rest);
            continue;
        }
        lines.push(line.to_string());
    }
    lines
}

fn fold(lines: &[String]) -> String {
    let mut out = String::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let bytes = line.as_bytes();
        if bytes.len() <= 75 {
            out.push_str(line);
            out.push_str("\r\n");
            continue;
        }
        let mut start = 0;
        let mut first = true;
        while start < bytes.len() {
            let budget = if first { 75 } else { 74 };
            let mut end = (start + budget).min(bytes.len());
            while end > start && !line.is_char_boundary(end) {
                end -= 1;
            }
            if end == start {
                end = (start + 1).min(bytes.len());
                while end < bytes.len() && !line.is_char_boundary(end) {
                    end += 1;
                }
            }
            if !first {
                out.push(' ');
            }
            out.push_str(&line[start..end]);
            out.push_str("\r\n");
            first = false;
            start = end;
        }
    }
    out
}

fn target_component_range(
    lines: &[String],
    media_type: DavMediaType,
) -> Result<(usize, usize), DavError> {
    let names = match media_type {
        DavMediaType::ICalendar => ["VEVENT", "VTODO", "VJOURNAL"].as_slice(),
        DavMediaType::VCard => ["VCARD"].as_slice(),
    };
    let start = lines.iter().position(|line| {
        names
            .iter()
            .any(|name| line.eq_ignore_ascii_case(&format!("BEGIN:{name}")))
    });
    let Some(start) = start else {
        return Err(DavError::InvalidRequest(
            "payload does not contain a patchable component".into(),
        ));
    };
    let begin = &lines[start];
    let kind = begin.split_once(':').map(|(_, name)| name).unwrap_or("");
    let end = lines[start + 1..]
        .iter()
        .position(|line| line.eq_ignore_ascii_case(&format!("END:{kind}")))
        .map(|offset| start + 1 + offset + 1)
        .ok_or_else(|| DavError::InvalidRequest("payload component is not terminated".into()))?;
    Ok((start, end))
}

fn apply_update(component: &mut Vec<String>, update: &PropertyUpdate) {
    let name = update.name.as_str();
    component.retain(|line| !property_matches(line, name));
    if let Some(value) = &update.value {
        let insert_at = component
            .iter()
            .rposition(|line| line.to_ascii_uppercase().starts_with("END:"))
            .unwrap_or(component.len());
        component.insert(insert_at, format!("{name}:{value}"));
    }
}

fn property_matches(line: &str, name: &str) -> bool {
    let head = line.split_once(':').map(|(head, _)| head).unwrap_or(line);
    let prop = head.split_once(';').map(|(prop, _)| prop).unwrap_or(head);
    prop.eq_ignore_ascii_case(name)
}

fn first_property(lines: &[String], name: &str) -> Option<String> {
    lines.iter().find_map(|line| {
        if !property_matches(line, name) {
            return None;
        }
        line.split_once(':').map(|(_, value)| value.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Foyer//DAV//EN\r\nBEGIN:VEVENT\r\nUID:event-1\r\nSUMMARY:Lunch\r\nX-UNKNOWN-PROP;FOO=1:keep-me\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

    #[test]
    fn patch_preserves_unknown_properties() {
        let payload = DavPayload::from_raw(DavMediaType::ICalendar, SAMPLE).unwrap();
        let patched = payload
            .patch(&[
                PropertyUpdate::set("SUMMARY", "Dinner"),
                PropertyUpdate::set("DESCRIPTION", "with notes"),
            ])
            .unwrap();
        assert!(patched.raw().contains("SUMMARY:Dinner"));
        assert!(patched.raw().contains("DESCRIPTION:with notes"));
        assert!(patched.raw().contains("X-UNKNOWN-PROP;FOO=1:keep-me"));
        assert_eq!(patched.uid().as_deref(), Some("event-1"));
        assert_eq!(patched.component_kind(), Some(ComponentKind::Event));
    }

    #[test]
    fn remove_does_not_drop_unrelated_lines() {
        let payload = DavPayload::from_raw(DavMediaType::ICalendar, SAMPLE).unwrap();
        let patched = payload.patch(&[PropertyUpdate::remove("SUMMARY")]).unwrap();
        assert!(!patched.raw().contains("SUMMARY:"));
        assert!(patched.raw().contains("X-UNKNOWN-PROP;FOO=1:keep-me"));
    }

    #[test]
    fn unfolded_equality_ignores_folding() {
        let folded = "BEGIN:VCARD\r\nVERSION:4.0\r\nFN:Ada Lovelace\r\nNOTE:This is a long note that should be folded by the writer later on without losing the original unknown property.\r\nX-FOYER:1\r\nEND:VCARD\r\n";
        let payload = DavPayload::from_raw(DavMediaType::VCard, folded).unwrap();
        assert!(payload.semantically_eq(&payload));
        assert_eq!(payload.component_kind(), Some(ComponentKind::Card));
    }
}
