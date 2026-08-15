//! Contacts slice: CardDAV is canonical; PostgreSQL rows are rebuildable projections.
#![allow(
    clippy::collapsible_if,
    clippy::if_same_then_else,
    clippy::manual_strip,
    clippy::map_flatten,
    clippy::needless_borrow,
    clippy::question_mark,
    clippy::too_many_arguments,
    clippy::useless_format
)]
//!
//! This module is self-contained so it can be compiled as `foyer_server::contacts` once
//! wired, or path-included by `tests/contacts_dav.rs` before that wiring exists.

use std::{
    collections::{BTreeMap, HashSet},
    fmt::{self, Write as _},
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
};

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{FromRow, PgPool, Postgres, Transaction, types::Json as SqlJson};
use uuid::Uuid;

use crate::{
    AppState,
    auth::Principal,
    dav::{
        CollectionKind, DavClient, DavHref, DavMediaType, DavPayload, ETag, NewAddressBook,
        PutPrecondition,
    },
};

pub const MAX_BOOK_NAME: usize = 80;
pub const MAX_BOOK_DESCRIPTION: usize = 400;
pub const MAX_DISPLAY_NAME: usize = 200;
pub const MAX_NAME_PART: usize = 100;
pub const MAX_ORGANIZATION: usize = 200;
pub const MAX_JOB_TITLE: usize = 200;
pub const MAX_EMAIL: usize = 254;
pub const MAX_PHONE: usize = 64;
pub const MAX_ADDRESS_LINE: usize = 200;
pub const MAX_NOTE_CHARS: usize = 16_384;
pub const MAX_EMAILS: usize = 16;
pub const MAX_PHONES: usize = 16;
pub const MAX_ADDRESSES: usize = 8;
pub const MAX_ADDRESS_BOOKS: i64 = 64;
pub const MAX_CONTACTS: i64 = 10_000;
pub const MAX_VCARD_BYTES: usize = 256 * 1024;

const KNOWN_PROPERTIES: &[&str] = &[
    "BEGIN", "END", "VERSION", "UID", "FN", "N", "EMAIL", "TEL", "ORG", "TITLE", "ADR", "BDAY",
    "NOTE", "REV", "PRODID", "KIND",
];

/// Shared PowerSync table names. Android and Foyer Shell must use this exact pair.
pub const ADDRESS_BOOKS_TABLE: &str = "contacts_address_books";
pub const CONTACTS_TABLE: &str = "contacts";

#[derive(Debug)]
pub struct ContactsError {
    status: StatusCode,
    code: &'static str,
    message: String,
    details: Option<Value>,
}

impl ContactsError {
    pub fn code(&self) -> &'static str {
        self.code
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn status(&self) -> StatusCode {
        self.status
    }

    fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
            details: None,
        }
    }

    fn with_details(mut self, details: Value) -> Self {
        self.details = Some(details);
        self
    }

    pub fn invalid_request(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, "invalid_request", message)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, "not_found", message)
    }

    pub fn gone(message: impl Into<String>) -> Self {
        Self::new(StatusCode::GONE, "gone", message)
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, "conflict", message)
    }

    pub fn stale_etag(expected: &str, actual: Option<&str>) -> Self {
        Self::new(
            StatusCode::CONFLICT,
            "stale_etag",
            "The expected ETag does not match the current CardDAV ETag.",
        )
        .with_details(json!({
            "expectedEtag": expected,
            "actualEtag": actual,
        }))
    }

    pub fn stale_revision(expected: i64, actual: i64) -> Self {
        Self::new(
            StatusCode::CONFLICT,
            "stale_revision",
            "The expected revision does not match the current revision.",
        )
        .with_details(json!({
            "expectedRevision": expected,
            "actualRevision": actual,
        }))
    }

    pub fn address_book_not_empty(message: impl Into<String>) -> Self {
        Self::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "address_book_not_empty",
            message,
        )
    }

    pub fn invalid_parent(message: impl Into<String>) -> Self {
        Self::new(StatusCode::UNPROCESSABLE_ENTITY, "invalid_parent", message)
    }

    pub fn limit_exceeded(message: impl Into<String>) -> Self {
        Self::new(StatusCode::UNPROCESSABLE_ENTITY, "limit_exceeded", message)
    }

    pub fn unavailable(message: impl Into<String>) -> Self {
        Self::new(StatusCode::SERVICE_UNAVAILABLE, "unavailable", message)
    }
}

impl fmt::Display for ContactsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for ContactsError {}

#[derive(Clone, Debug, Serialize)]
struct ErrorBody {
    error: ErrorObject,
}

#[derive(Clone, Debug, Serialize)]
struct ErrorObject {
    code: &'static str,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    details: Option<Value>,
}

impl IntoResponse for ContactsError {
    fn into_response(self) -> Response {
        let status = self.status;
        let body = ErrorBody {
            error: ErrorObject {
                code: self.code,
                message: self.message,
                details: self.details,
            },
        };
        (status, Json(body)).into_response()
    }
}

pub type ContactsResult<T> = Result<T, ContactsError>;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructuredName {
    #[serde(default, rename = "familyName")]
    pub family_name: String,
    #[serde(default, rename = "givenName")]
    pub given_name: String,
    #[serde(default, rename = "additionalNames")]
    pub additional_names: String,
    #[serde(default, rename = "honorificPrefix")]
    pub honorific_prefix: String,
    #[serde(default, rename = "honorificSuffix")]
    pub honorific_suffix: String,
}

impl StructuredName {
    pub fn is_empty(&self) -> bool {
        self.family_name.is_empty()
            && self.given_name.is_empty()
            && self.additional_names.is_empty()
            && self.honorific_prefix.is_empty()
            && self.honorific_suffix.is_empty()
    }

    fn formatted(&self) -> String {
        let mut parts = Vec::new();
        if !self.honorific_prefix.is_empty() {
            parts.push(self.honorific_prefix.as_str());
        }
        if !self.given_name.is_empty() {
            parts.push(self.given_name.as_str());
        }
        if !self.additional_names.is_empty() {
            parts.push(self.additional_names.as_str());
        }
        if !self.family_name.is_empty() {
            parts.push(self.family_name.as_str());
        }
        if !self.honorific_suffix.is_empty() {
            parts.push(self.honorific_suffix.as_str());
        }
        parts.join(" ")
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TypedEmail {
    pub value: String,
    #[serde(default = "default_type")]
    pub r#type: String,
    #[serde(default)]
    pub pref: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TypedPhone {
    pub value: String,
    #[serde(default = "default_type")]
    pub r#type: String,
    #[serde(default)]
    pub pref: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PostalAddress {
    #[serde(default, rename = "poBox")]
    pub po_box: String,
    #[serde(default)]
    pub extended: String,
    #[serde(default)]
    pub street: String,
    #[serde(default)]
    pub locality: String,
    #[serde(default)]
    pub region: String,
    #[serde(default, rename = "postalCode")]
    pub postal_code: String,
    #[serde(default)]
    pub country: String,
    #[serde(default = "default_type")]
    pub r#type: String,
    #[serde(default)]
    pub pref: bool,
}

fn default_type() -> String {
    "other".into()
}

impl PostalAddress {
    pub fn is_empty(&self) -> bool {
        self.po_box.is_empty()
            && self.extended.is_empty()
            && self.street.is_empty()
            && self.locality.is_empty()
            && self.region.is_empty()
            && self.postal_code.is_empty()
            && self.country.is_empty()
    }

    pub fn one_line(&self) -> String {
        [
            self.street.as_str(),
            self.locality.as_str(),
            self.region.as_str(),
            self.postal_code.as_str(),
            self.country.as_str(),
        ]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(", ")
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, FromRow, PartialEq, Eq)]
pub struct AddressBook {
    pub id: String,
    #[serde(rename = "userId")]
    pub user_id: String,
    pub uid: String,
    pub href: String,
    pub etag: Option<String>,
    #[serde(rename = "displayName")]
    pub display_name: String,
    #[serde(default)]
    pub description: String,
    #[serde(rename = "syncToken", skip_serializing)]
    pub sync_token: Option<String>,
    #[serde(skip_serializing)]
    pub ctag: Option<String>,
    pub revision: i64,
    #[serde(rename = "createdAt")]
    pub created_at: DateTime<Utc>,
    #[serde(rename = "updatedAt")]
    pub updated_at: DateTime<Utc>,
    #[serde(rename = "deletedAt")]
    pub deleted_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Contact {
    pub id: String,
    #[serde(rename = "userId")]
    pub user_id: String,
    #[serde(rename = "addressBookId")]
    pub address_book_id: String,
    pub uid: String,
    pub href: String,
    pub etag: String,
    #[serde(rename = "displayName")]
    pub display_name: String,
    pub name: StructuredName,
    pub emails: Vec<TypedEmail>,
    pub phones: Vec<TypedPhone>,
    pub organization: String,
    #[serde(rename = "jobTitle")]
    pub job_title: String,
    pub addresses: Vec<PostalAddress>,
    pub birthday: Option<String>,
    pub notes: String,
    pub revision: i64,
    #[serde(rename = "createdAt")]
    pub created_at: DateTime<Utc>,
    #[serde(rename = "updatedAt")]
    pub updated_at: DateTime<Utc>,
    #[serde(rename = "deletedAt")]
    pub deleted_at: Option<DateTime<Utc>>,
}

#[derive(FromRow)]
struct ContactRow {
    id: String,
    user_id: String,
    address_book_id: String,
    uid: String,
    href: String,
    etag: String,
    display_name: String,
    given_name: String,
    family_name: String,
    additional_names: String,
    honorific_prefix: String,
    honorific_suffix: String,
    organization: String,
    job_title: String,
    birthday: Option<String>,
    notes: String,
    emails: SqlJson<Vec<TypedEmail>>,
    phones: SqlJson<Vec<TypedPhone>>,
    addresses: SqlJson<Vec<PostalAddress>>,
    revision: i64,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    deleted_at: Option<DateTime<Utc>>,
}

impl From<ContactRow> for Contact {
    fn from(row: ContactRow) -> Self {
        Self {
            id: row.id,
            user_id: row.user_id,
            address_book_id: row.address_book_id,
            uid: row.uid,
            href: row.href,
            etag: row.etag,
            display_name: row.display_name,
            name: StructuredName {
                family_name: row.family_name,
                given_name: row.given_name,
                additional_names: row.additional_names,
                honorific_prefix: row.honorific_prefix,
                honorific_suffix: row.honorific_suffix,
            },
            emails: row.emails.0,
            phones: row.phones.0,
            organization: row.organization,
            job_title: row.job_title,
            addresses: row.addresses.0,
            birthday: row.birthday,
            notes: row.notes,
            revision: row.revision,
            created_at: row.created_at,
            updated_at: row.updated_at,
            deleted_at: row.deleted_at,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct AddressBookList {
    #[serde(rename = "addressBooks")]
    pub address_books: Vec<AddressBook>,
}

#[derive(Debug, Serialize)]
pub struct ContactList {
    pub contacts: Vec<Contact>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateAddressBookRequest {
    #[serde(rename = "operationId")]
    pub operation_id: String,
    pub id: String,
    #[serde(rename = "displayName")]
    pub display_name: String,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateAddressBookRequest {
    #[serde(rename = "operationId")]
    pub operation_id: String,
    #[serde(rename = "expectedEtag")]
    pub expected_etag: Option<String>,
    #[serde(rename = "expectedRevision")]
    pub expected_revision: Option<i64>,
    #[serde(rename = "displayName")]
    pub display_name: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DeleteRequest {
    #[serde(rename = "operationId")]
    pub operation_id: String,
    #[serde(rename = "expectedEtag")]
    pub expected_etag: Option<String>,
    #[serde(rename = "expectedRevision")]
    pub expected_revision: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateContactRequest {
    #[serde(rename = "operationId")]
    pub operation_id: String,
    pub id: String,
    #[serde(rename = "addressBookId")]
    pub address_book_id: String,
    #[serde(default, rename = "displayName")]
    pub display_name: Option<String>,
    #[serde(default)]
    pub name: Option<StructuredName>,
    #[serde(default)]
    pub emails: Vec<TypedEmail>,
    #[serde(default)]
    pub phones: Vec<TypedPhone>,
    #[serde(default)]
    pub organization: Option<String>,
    #[serde(default, rename = "jobTitle")]
    pub job_title: Option<String>,
    #[serde(default)]
    pub addresses: Vec<PostalAddress>,
    #[serde(default)]
    pub birthday: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UpdateContactRequest {
    #[serde(rename = "operationId")]
    pub operation_id: String,
    #[serde(rename = "expectedEtag")]
    pub expected_etag: Option<String>,
    #[serde(rename = "expectedRevision")]
    pub expected_revision: Option<i64>,
    #[serde(default, rename = "displayName")]
    pub display_name: Option<String>,
    #[serde(default)]
    pub name: Option<StructuredName>,
    #[serde(default)]
    pub emails: Option<Vec<TypedEmail>>,
    #[serde(default)]
    pub phones: Option<Vec<TypedPhone>>,
    #[serde(default)]
    pub organization: Option<String>,
    #[serde(default, rename = "jobTitle")]
    pub job_title: Option<String>,
    #[serde(default)]
    pub addresses: Option<Vec<PostalAddress>>,
    #[serde(default)]
    pub birthday: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MoveContactRequest {
    #[serde(rename = "operationId")]
    pub operation_id: String,
    #[serde(rename = "expectedEtag")]
    pub expected_etag: Option<String>,
    #[serde(rename = "expectedRevision")]
    pub expected_revision: Option<i64>,
    #[serde(rename = "addressBookId")]
    pub address_book_id: String,
}

#[derive(Debug, Deserialize)]
pub struct ContactListQuery {
    #[serde(rename = "addressBookId")]
    pub address_book_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VCardProperty {
    pub group: Option<String>,
    pub name: String,
    pub params: Vec<(String, Vec<String>)>,
    pub value: String,
}

impl VCardProperty {
    pub fn new(name: &str, value: impl Into<String>) -> Self {
        Self {
            group: None,
            name: name.to_ascii_uppercase(),
            params: Vec::new(),
            value: value.into(),
        }
    }

    pub fn with_type(mut self, type_value: &str, pref: bool) -> Self {
        let mut types: Vec<String> = type_value
            .split(',')
            .map(|part| part.trim().to_ascii_lowercase())
            .filter(|part| !part.is_empty() && part != "pref")
            .collect();
        if types.is_empty() {
            types.push("other".into());
        }
        self.params.push(("TYPE".into(), types));
        if pref {
            self.params.push(("PREF".into(), vec!["1".into()]));
        }
        self
    }

    pub fn param(&self, name: &str) -> Vec<String> {
        let needle = name.to_ascii_uppercase();
        self.params
            .iter()
            .filter(|(key, _)| key.eq_ignore_ascii_case(&needle))
            .flat_map(|(_, values)| values.iter().cloned())
            .collect()
    }

    pub fn has_pref(&self) -> bool {
        self.param("PREF")
            .iter()
            .any(|value| value != "0" && !value.eq_ignore_ascii_case("false"))
            || self
                .param("TYPE")
                .iter()
                .any(|value| value.eq_ignore_ascii_case("pref"))
    }

    pub fn primary_type(&self) -> String {
        self.param("TYPE")
            .into_iter()
            .find(|value| !value.eq_ignore_ascii_case("pref"))
            .map(|value| normalize_type(&value))
            .unwrap_or_else(|| "other".into())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VCard {
    pub properties: Vec<VCardProperty>,
}

impl VCard {
    pub fn get(&self, name: &str) -> impl Iterator<Item = &VCardProperty> {
        let name = name.to_ascii_uppercase();
        self.properties
            .iter()
            .filter(move |property| property.name == name)
    }

    pub fn first_value(&self, name: &str) -> Option<&str> {
        self.get(name)
            .next()
            .map(|property| property.value.as_str())
    }

    pub fn set_or_insert(&mut self, name: &str, value: String) {
        let name = name.to_ascii_uppercase();
        if let Some(existing) = self
            .properties
            .iter_mut()
            .find(|property| property.name == name)
        {
            existing.value = value;
            return;
        }
        self.insert_after_known(&name, VCardProperty::new(&name, value));
    }

    pub fn remove_all(&mut self, name: &str) -> Vec<VCardProperty> {
        let name = name.to_ascii_uppercase();
        let mut kept = Vec::new();
        let mut removed = Vec::new();
        for property in self.properties.drain(..) {
            if property.name == name {
                removed.push(property);
            } else {
                kept.push(property);
            }
        }
        self.properties = kept;
        removed
    }

    fn insert_after_known(&mut self, name: &str, property: VCardProperty) {
        let insert_at = self
            .properties
            .iter()
            .position(|existing| existing.name == "END")
            .unwrap_or(self.properties.len());
        let _ = name;
        self.properties.insert(insert_at, property);
    }

    pub fn ensure_envelope(&mut self, uid: &str) {
        if !self
            .properties
            .iter()
            .any(|property| property.name == "BEGIN")
        {
            self.properties
                .insert(0, VCardProperty::new("BEGIN", "VCARD"));
        }
        if !self
            .properties
            .iter()
            .any(|property| property.name == "VERSION")
        {
            self.properties.insert(
                1.min(self.properties.len()),
                VCardProperty::new("VERSION", "4.0"),
            );
        }
        if !self
            .properties
            .iter()
            .any(|property| property.name == "UID")
        {
            self.set_or_insert("UID", uid.to_string());
        }
        if !self
            .properties
            .iter()
            .any(|property| property.name == "KIND")
        {
            self.set_or_insert("KIND", "individual".into());
        }
        if !self
            .properties
            .iter()
            .any(|property| property.name == "PRODID")
        {
            self.set_or_insert("PRODID", "-//Foyer//Contacts//EN".into());
        }
        if !self
            .properties
            .iter()
            .any(|property| property.name == "END")
        {
            self.properties.push(VCardProperty::new("END", "VCARD"));
        }
    }
}

pub fn unfold(input: &str) -> String {
    let normalized = input.replace("\r\n", "\n").replace('\r', "\n");
    let mut out = String::with_capacity(normalized.len());
    let mut chars = normalized.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\n' {
            match chars.peek() {
                Some(' ' | '\t') => {
                    chars.next();
                }
                _ => out.push('\n'),
            }
        } else {
            out.push(ch);
        }
    }
    out
}

pub fn fold_line(line: &str) -> String {
    let bytes = line.as_bytes();
    if bytes.len() <= 75 {
        return line.to_string();
    }
    let mut out = String::new();
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
            out.push_str("\r\n ");
        }
        out.push_str(&line[start..end]);
        start = end;
        first = false;
    }
    out
}

pub fn escape_text(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            ',' => out.push_str("\\,"),
            ';' => out.push_str("\\;"),
            '\n' => out.push_str("\\n"),
            '\r' => {}
            _ => out.push(ch),
        }
    }
    out
}

pub fn unescape_text(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('n' | 'N') => out.push('\n'),
                Some('r' | 'R') => {}
                Some(other) => out.push(other),
                None => out.push('\\'),
            }
        } else {
            out.push(ch);
        }
    }
    out
}

fn split_escaped(value: &str, delimiter: char) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut chars = value.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some(next) => {
                    current.push('\\');
                    current.push(next);
                }
                None => current.push('\\'),
            }
        } else if ch == delimiter {
            parts.push(unescape_text(&current));
            current.clear();
        } else {
            current.push(ch);
        }
    }
    parts.push(unescape_text(&current));
    parts
}

pub fn parse_vcard(input: &str) -> ContactsResult<VCard> {
    if input.len() > MAX_VCARD_BYTES {
        return Err(ContactsError::invalid_request(format!(
            "vCard must be at most {MAX_VCARD_BYTES} bytes."
        )));
    }
    if input.contains('\0') {
        return Err(ContactsError::invalid_request(
            "vCard cannot contain NUL bytes.",
        ));
    }
    let unfolded = unfold(input);
    let mut properties = Vec::new();
    for raw_line in unfolded.lines() {
        let line = raw_line.trim_end_matches('\r');
        if line.trim().is_empty() {
            continue;
        }
        properties.push(parse_property(line)?);
    }
    if properties.is_empty() {
        return Err(ContactsError::invalid_request("vCard is empty."));
    }
    let begin = properties.first().is_some_and(|property| {
        property.name == "BEGIN" && property.value.eq_ignore_ascii_case("VCARD")
    });
    let end = properties.last().is_some_and(|property| {
        property.name == "END" && property.value.eq_ignore_ascii_case("VCARD")
    });
    if !begin || !end {
        return Err(ContactsError::invalid_request(
            "vCard must start with BEGIN:VCARD and end with END:VCARD.",
        ));
    }
    Ok(VCard { properties })
}

fn parse_property(line: &str) -> ContactsResult<VCardProperty> {
    let (meta, raw_value) = split_name_value(line)?;
    let (group, name_and_params) = match meta.split_once('.') {
        Some((group, rest)) if !group.is_empty() && !group.contains(';') && !rest.is_empty() => {
            (Some(group.to_string()), rest)
        }
        _ => (None, meta),
    };
    let mut parts = name_and_params.split(';');
    let name = parts
        .next()
        .filter(|name| !name.is_empty())
        .ok_or_else(|| ContactsError::invalid_request("vCard property is missing a name."))?
        .to_ascii_uppercase();
    let mut params = Vec::new();
    for part in parts {
        if part.is_empty() {
            continue;
        }
        let (key, value) = match part.split_once('=') {
            Some((key, value)) => (key, value),
            None => (part, ""),
        };
        let values = split_param_values(value);
        params.push((key.to_ascii_uppercase(), values));
    }
    Ok(VCardProperty {
        group,
        name,
        params,
        value: unescape_text(raw_value),
    })
}

fn split_name_value(line: &str) -> ContactsResult<(&str, &str)> {
    let mut escaped = false;
    for (index, ch) in line.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == ':' {
            return Ok((&line[..index], &line[index + 1..]));
        }
    }
    Err(ContactsError::invalid_request(
        "vCard property is missing a colon separator.",
    ))
}

fn split_param_values(value: &str) -> Vec<String> {
    if value.is_empty() {
        return Vec::new();
    }
    let mut values = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for ch in value.chars() {
        match ch {
            '"' => quoted = !quoted,
            ',' if !quoted => {
                if !current.is_empty() {
                    values.push(current.clone());
                    current.clear();
                }
            }
            _ => current.push(ch),
        }
    }
    if !current.is_empty() {
        values.push(current);
    }
    values
}

pub fn serialize_vcard(card: &VCard) -> String {
    let mut out = String::new();
    for property in &card.properties {
        if !out.is_empty() {
            out.push_str("\r\n");
        }
        out.push_str(&fold_line(&format_property(property)));
    }
    if !out.ends_with("\r\n") {
        out.push_str("\r\n");
    }
    out
}

fn format_property(property: &VCardProperty) -> String {
    let mut line = String::new();
    if let Some(group) = &property.group {
        line.push_str(group);
        line.push('.');
    }
    line.push_str(&property.name);
    for (key, values) in &property.params {
        line.push(';');
        line.push_str(key);
        if !values.is_empty() {
            line.push('=');
            line.push_str(&values.join(","));
        }
    }
    line.push(':');
    let escaped = match property.name.as_str() {
        "N" | "ADR" | "ORG" => escape_structured(&property.value),
        _ => escape_text(&property.value),
    };
    line.push_str(&escaped);
    line
}

fn escape_structured(value: &str) -> String {
    // Structured fields are stored unescaped with `;` separators already applied
    // by the patcher. Escape only the RFC text specials inside each component.
    value
        .split(';')
        .map(escape_text)
        .collect::<Vec<_>>()
        .join(";")
}

pub fn contact_fields_from_vcard(card: &VCard) -> ContactFields {
    let name = card
        .first_value("N")
        .map(parse_structured_name)
        .unwrap_or_default();
    let display_name = card
        .first_value("FN")
        .map(str::to_string)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| name.formatted());
    let emails = card
        .get("EMAIL")
        .map(|property| TypedEmail {
            value: property.value.clone(),
            r#type: property.primary_type(),
            pref: property.has_pref(),
        })
        .filter(|email| !email.value.is_empty())
        .collect();
    let phones = card
        .get("TEL")
        .map(|property| TypedPhone {
            value: property.value.clone(),
            r#type: property.primary_type(),
            pref: property.has_pref(),
        })
        .filter(|phone| !phone.value.is_empty())
        .collect();
    let addresses = card
        .get("ADR")
        .map(|property| {
            let mut address = parse_address(&property.value);
            address.r#type = property.primary_type();
            address.pref = property.has_pref();
            address
        })
        .filter(|address| !address.is_empty())
        .collect();
    ContactFields {
        uid: card.first_value("UID").unwrap_or_default().to_string(),
        display_name,
        name,
        emails,
        phones,
        organization: card
            .first_value("ORG")
            .unwrap_or_default()
            .replace(';', " "),
        job_title: card.first_value("TITLE").unwrap_or_default().to_string(),
        addresses,
        birthday: card
            .first_value("BDAY")
            .map(normalize_birthday_value)
            .flatten(),
        notes: card.first_value("NOTE").unwrap_or_default().to_string(),
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ContactFields {
    pub uid: String,
    pub display_name: String,
    pub name: StructuredName,
    pub emails: Vec<TypedEmail>,
    pub phones: Vec<TypedPhone>,
    pub organization: String,
    pub job_title: String,
    pub addresses: Vec<PostalAddress>,
    pub birthday: Option<String>,
    pub notes: String,
}

fn parse_structured_name(value: &str) -> StructuredName {
    let mut parts = split_escaped(value, ';');
    parts.resize(5, String::new());
    StructuredName {
        family_name: parts[0].clone(),
        given_name: parts[1].clone(),
        additional_names: parts[2].clone(),
        honorific_prefix: parts[3].clone(),
        honorific_suffix: parts[4].clone(),
    }
}

fn format_structured_name(name: &StructuredName) -> String {
    [
        name.family_name.as_str(),
        name.given_name.as_str(),
        name.additional_names.as_str(),
        name.honorific_prefix.as_str(),
        name.honorific_suffix.as_str(),
    ]
    .join(";")
}

fn parse_address(value: &str) -> PostalAddress {
    let mut parts = split_escaped(value, ';');
    parts.resize(7, String::new());
    PostalAddress {
        po_box: parts[0].clone(),
        extended: parts[1].clone(),
        street: parts[2].clone(),
        locality: parts[3].clone(),
        region: parts[4].clone(),
        postal_code: parts[5].clone(),
        country: parts[6].clone(),
        r#type: "other".into(),
        pref: false,
    }
}

fn format_address(address: &PostalAddress) -> String {
    [
        address.po_box.as_str(),
        address.extended.as_str(),
        address.street.as_str(),
        address.locality.as_str(),
        address.region.as_str(),
        address.postal_code.as_str(),
        address.country.as_str(),
    ]
    .join(";")
}

fn normalize_birthday_value(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some(date) = parse_birthday(trimmed).ok().flatten() {
        return Some(date);
    }
    Some(trimmed.to_string())
}

pub fn apply_contact_patch(card: &mut VCard, patch: &ContactPatch) -> ContactsResult<()> {
    if let Some(name) = &patch.name {
        replace_single(card, "N", format_structured_name(name));
        if patch.display_name.is_none() && card.first_value("FN").unwrap_or_default().is_empty() {
            replace_single(card, "FN", name.formatted());
        }
    }
    if let Some(display_name) = &patch.display_name {
        replace_single(card, "FN", display_name.clone());
    }
    if let Some(emails) = &patch.emails {
        replace_typed(card, "EMAIL", emails, |email| {
            (email.value.clone(), email.r#type.as_str(), email.pref)
        });
    }
    if let Some(phones) = &patch.phones {
        replace_typed(card, "TEL", phones, |phone| {
            (phone.value.clone(), phone.r#type.as_str(), phone.pref)
        });
    }
    if let Some(organization) = &patch.organization {
        if organization.is_empty() {
            card.remove_all("ORG");
        } else {
            replace_single(card, "ORG", organization.clone());
        }
    }
    if let Some(job_title) = &patch.job_title {
        if job_title.is_empty() {
            card.remove_all("TITLE");
        } else {
            replace_single(card, "TITLE", job_title.clone());
        }
    }
    if let Some(addresses) = &patch.addresses {
        replace_addresses(card, addresses);
    }
    if let Some(birthday) = &patch.birthday {
        if birthday.is_empty() {
            card.remove_all("BDAY");
        } else {
            replace_single(card, "BDAY", birthday.replace('-', ""));
        }
    }
    if let Some(notes) = &patch.notes {
        if notes.is_empty() {
            card.remove_all("NOTE");
        } else {
            replace_single(card, "NOTE", notes.clone());
        }
    }
    card.set_or_insert("REV", Utc::now().format("%Y%m%dT%H%M%SZ").to_string());
    Ok(())
}

#[derive(Clone, Debug, Default)]
pub struct ContactPatch {
    pub display_name: Option<String>,
    pub name: Option<StructuredName>,
    pub emails: Option<Vec<TypedEmail>>,
    pub phones: Option<Vec<TypedPhone>>,
    pub organization: Option<String>,
    pub job_title: Option<String>,
    pub addresses: Option<Vec<PostalAddress>>,
    pub birthday: Option<String>,
    pub notes: Option<String>,
}

impl From<&UpdateContactRequest> for ContactPatch {
    fn from(request: &UpdateContactRequest) -> Self {
        Self {
            display_name: request.display_name.clone(),
            name: request.name.clone(),
            emails: request.emails.clone(),
            phones: request.phones.clone(),
            organization: request.organization.clone(),
            job_title: request.job_title.clone(),
            addresses: request.addresses.clone(),
            birthday: request.birthday.clone(),
            notes: request.notes.clone(),
        }
    }
}

impl From<&CreateContactRequest> for ContactPatch {
    fn from(request: &CreateContactRequest) -> Self {
        Self {
            display_name: request.display_name.clone(),
            name: request.name.clone(),
            emails: Some(request.emails.clone()),
            phones: Some(request.phones.clone()),
            organization: request.organization.clone(),
            job_title: request.job_title.clone(),
            addresses: Some(request.addresses.clone()),
            birthday: request.birthday.clone(),
            notes: request.notes.clone(),
        }
    }
}

fn replace_single(card: &mut VCard, name: &str, value: String) {
    let removed = card.remove_all(name);
    let mut property = removed
        .into_iter()
        .next()
        .unwrap_or_else(|| VCardProperty::new(name, ""));
    property.value = value;
    card.insert_after_known(name, property);
}

fn replace_typed<T>(
    card: &mut VCard,
    name: &str,
    items: &[T],
    view: impl Fn(&T) -> (String, &str, bool),
) {
    let existing = card.remove_all(name);
    if items.is_empty() {
        return;
    }
    let insert_at = card
        .properties
        .iter()
        .position(|property| property.name == "END")
        .unwrap_or(card.properties.len());
    let mut used = HashSet::new();
    let mut built = Vec::new();
    for item in items {
        let (value, type_name, pref) = view(item);
        let mut property = existing
            .iter()
            .enumerate()
            .find(|(index, property)| {
                !used.contains(index) && property.value.eq_ignore_ascii_case(&value)
            })
            .map(|(index, property)| {
                used.insert(index);
                property.clone()
            })
            .unwrap_or_else(|| VCardProperty::new(name, &value));
        property.value = value;
        property.params.retain(|(key, _)| {
            !key.eq_ignore_ascii_case("TYPE") && !key.eq_ignore_ascii_case("PREF")
        });
        property = property.with_type(type_name, pref);
        built.push(property);
    }
    for (offset, property) in built.into_iter().enumerate() {
        card.properties.insert(insert_at + offset, property);
    }
}

fn replace_addresses(card: &mut VCard, addresses: &[PostalAddress]) {
    let existing = card.remove_all("ADR");
    if addresses.is_empty() {
        return;
    }
    let insert_at = card
        .properties
        .iter()
        .position(|property| property.name == "END")
        .unwrap_or(card.properties.len());
    let mut used = HashSet::new();
    let mut built = Vec::new();
    for address in addresses {
        let value = format_address(address);
        let mut property = existing
            .iter()
            .enumerate()
            .find(|(index, property)| !used.contains(index) && property.value == value)
            .map(|(index, property)| {
                used.insert(index);
                property.clone()
            })
            .unwrap_or_else(|| VCardProperty::new("ADR", &value));
        property.value = value;
        property.params.retain(|(key, _)| {
            !key.eq_ignore_ascii_case("TYPE") && !key.eq_ignore_ascii_case("PREF")
        });
        property = property.with_type(&address.r#type, address.pref);
        built.push(property);
    }
    for (offset, property) in built.into_iter().enumerate() {
        card.properties.insert(insert_at + offset, property);
    }
}

pub fn vcard_from_create(uid: &str, request: &CreateContactRequest) -> ContactsResult<VCard> {
    let mut card = VCard {
        properties: vec![
            VCardProperty::new("BEGIN", "VCARD"),
            VCardProperty::new("VERSION", "4.0"),
            VCardProperty::new("UID", uid),
            VCardProperty::new("KIND", "individual"),
            VCardProperty::new("PRODID", "-//Foyer//Contacts//EN"),
            VCardProperty::new("END", "VCARD"),
        ],
    };
    let mut patch = ContactPatch::from(request);
    if patch
        .display_name
        .as_ref()
        .is_none_or(|value| value.is_empty())
    {
        patch.display_name = Some(derived_display_name(request));
    }
    apply_contact_patch(&mut card, &patch)?;
    Ok(card)
}

fn derived_display_name(request: &CreateContactRequest) -> String {
    if let Some(name) = request
        .display_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return name.to_string();
    }
    if let Some(formatted) = request
        .name
        .as_ref()
        .map(StructuredName::formatted)
        .filter(|value| !value.is_empty())
    {
        return formatted;
    }
    if let Some(email) = request.emails.first() {
        return email.value.clone();
    }
    if let Some(phone) = request.phones.first() {
        return phone.value.clone();
    }
    if let Some(org) = request
        .organization
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return org.to_string();
    }
    "Unnamed contact".into()
}

pub fn unknown_properties(card: &VCard) -> Vec<&VCardProperty> {
    card.properties
        .iter()
        .filter(|property| {
            !KNOWN_PROPERTIES
                .iter()
                .any(|known| property.name.eq_ignore_ascii_case(known))
        })
        .collect()
}

fn normalize_type(value: &str) -> String {
    match value.to_ascii_lowercase().as_str() {
        "work" | "home" | "cell" | "mobile" | "fax" | "voice" | "text" | "pager" | "other" => {
            if value.eq_ignore_ascii_case("mobile") {
                "cell".into()
            } else {
                value.to_ascii_lowercase()
            }
        }
        _ => value.to_ascii_lowercase(),
    }
}

#[derive(Clone, Debug)]
pub struct DavRequest {
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct DavResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl DavResponse {
    pub fn header(&self, name: &str) -> Option<String> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.clone())
    }

    pub fn etag(&self) -> Option<String> {
        self.header("ETag").map(|value| normalize_etag(&value))
    }

    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

#[derive(Clone)]
pub enum DavBackend {
    Memory(MemoryDav),
    Client(DavClient),
}

impl DavBackend {
    pub async fn send(&self, request: DavRequest) -> ContactsResult<DavResponse> {
        match self {
            Self::Memory(dav) => dav.send(request),
            Self::Client(_) => Err(ContactsError::unavailable(
                "raw DAV requests are not used with the shared DavClient",
            )),
        }
    }
}

#[derive(Clone, Default)]
pub struct MemoryDav {
    inner: Arc<Mutex<MemoryDavState>>,
}

#[derive(Default)]
struct MemoryDavState {
    resources: BTreeMap<String, MemoryResource>,
    next_etag: u64,
    next_sync: u64,
}

#[derive(Clone)]
struct MemoryResource {
    href: String,
    etag: String,
    body: Vec<u8>,
    display_name: String,
    description: String,
    is_collection: bool,
    sync_token: String,
}

impl MemoryDav {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn seed_principal(&self, principal: &str) {
        let mut state = self.inner.lock().expect("memory dav");
        let href = normalize_href(principal);
        if !state.resources.contains_key(&href) {
            state.next_etag += 1;
            state.next_sync += 1;
            let etag = format!("\"e{}\"", state.next_etag);
            let sync_token = format!("sync-{}", state.next_sync);
            state.resources.insert(
                href.clone(),
                MemoryResource {
                    href,
                    etag,
                    body: Vec::new(),
                    display_name: "principal".into(),
                    description: String::new(),
                    is_collection: true,
                    sync_token,
                },
            );
        }
    }

    fn send(&self, request: DavRequest) -> ContactsResult<DavResponse> {
        let mut state = self.inner.lock().expect("memory dav");
        let path = normalize_href(&request.path);
        match request.method.to_ascii_uppercase().as_str() {
            "PROPFIND" => Ok(state.propfind(&path, depth(&request))),
            "GET" => Ok(state.get(&path)),
            "PUT" => Ok(state.put(&path, &request)),
            "DELETE" => Ok(state.delete(&path, &request)),
            "MKCOL" => Ok(state.mkcol(&path, &request)),
            "PROPPATCH" => Ok(state.proppatch(&path, &request)),
            "MOVE" => Ok(state.move_resource(&path, &request)),
            "REPORT" => Ok(state.propfind(&path, 1)),
            other => Ok(DavResponse {
                status: 405,
                headers: Vec::new(),
                body: format!("unsupported method {other}").into_bytes(),
            }),
        }
    }
}

impl MemoryDavState {
    fn bump(&mut self) -> (String, String) {
        self.next_etag += 1;
        self.next_sync += 1;
        (
            format!("\"e{}\"", self.next_etag),
            format!("sync-{}", self.next_sync),
        )
    }

    fn precondition(request: &DavRequest, existing: Option<&MemoryResource>) -> Option<u16> {
        if let Some(if_match) = header(request, "If-Match") {
            match existing {
                None => return Some(412),
                Some(resource) if !etags_match(&if_match, &resource.etag) => return Some(412),
                Some(_) => {}
            }
        }
        if let Some(if_none) = header(request, "If-None-Match") {
            if if_none.trim() == "*" && existing.is_some() {
                return Some(412);
            }
            if let Some(resource) = existing {
                if etags_match(&if_none, &resource.etag) {
                    return Some(412);
                }
            }
        }
        None
    }

    fn get(&self, path: &str) -> DavResponse {
        match self.resources.get(path) {
            Some(resource) if !resource.is_collection => DavResponse {
                status: 200,
                headers: vec![
                    ("ETag".into(), resource.etag.clone()),
                    ("Content-Type".into(), "text/vcard; charset=utf-8".into()),
                ],
                body: resource.body.clone(),
            },
            Some(_) => DavResponse {
                status: 403,
                headers: Vec::new(),
                body: b"collection".to_vec(),
            },
            None => DavResponse {
                status: 404,
                headers: Vec::new(),
                body: b"missing".to_vec(),
            },
        }
    }

    fn put(&mut self, path: &str, request: &DavRequest) -> DavResponse {
        let existing = self.resources.get(path).cloned();
        if let Some(status) = Self::precondition(request, existing.as_ref()) {
            return DavResponse {
                status,
                headers: existing
                    .map(|resource| vec![("ETag".into(), resource.etag)])
                    .unwrap_or_default(),
                body: b"precondition".to_vec(),
            };
        }
        let (etag, sync_token) = self.bump();
        let parent = parent_href(path);
        if let Some(parent) = self.resources.get_mut(&parent) {
            parent.sync_token = sync_token.clone();
            parent.etag = etag.clone();
        }
        self.resources.insert(
            path.to_string(),
            MemoryResource {
                href: path.to_string(),
                etag: etag.clone(),
                body: request.body.clone(),
                display_name: path.rsplit('/').next().unwrap_or(path).to_string(),
                description: String::new(),
                is_collection: false,
                sync_token,
            },
        );
        DavResponse {
            status: if existing.is_some() { 204 } else { 201 },
            headers: vec![("ETag".into(), etag)],
            body: Vec::new(),
        }
    }

    fn delete(&mut self, path: &str, request: &DavRequest) -> DavResponse {
        let existing = self.resources.get(path).cloned();
        if existing.is_none() {
            return DavResponse {
                status: 404,
                headers: Vec::new(),
                body: b"missing".to_vec(),
            };
        }
        if let Some(status) = Self::precondition(request, existing.as_ref()) {
            return DavResponse {
                status,
                headers: existing
                    .map(|resource| vec![("ETag".into(), resource.etag)])
                    .unwrap_or_default(),
                body: b"precondition".to_vec(),
            };
        }
        let removed: Vec<String> = self
            .resources
            .keys()
            .filter(|href| *href == path || href.starts_with(&format!("{path}")))
            .cloned()
            .collect();
        for href in removed {
            self.resources.remove(&href);
        }
        let parent_path = parent_href(path);
        if self.resources.contains_key(&parent_path) {
            let (etag, sync_token) = self.bump();
            if let Some(parent) = self.resources.get_mut(&parent_path) {
                parent.etag = etag;
                parent.sync_token = sync_token;
            }
        }
        DavResponse {
            status: 204,
            headers: Vec::new(),
            body: Vec::new(),
        }
    }

    fn mkcol(&mut self, path: &str, request: &DavRequest) -> DavResponse {
        if self.resources.contains_key(path) {
            return DavResponse {
                status: 405,
                headers: Vec::new(),
                body: b"exists".to_vec(),
            };
        }
        let xml = String::from_utf8_lossy(&request.body);
        let display_name = xml_local_text(&xml, "displayname").unwrap_or_else(|| {
            path.trim_end_matches('/')
                .rsplit('/')
                .next()
                .unwrap_or("Contacts")
                .to_string()
        });
        let description = xml_local_text(&xml, "addressbook-description").unwrap_or_default();
        let (etag, sync_token) = self.bump();
        self.resources.insert(
            path.to_string(),
            MemoryResource {
                href: path.to_string(),
                etag: etag.clone(),
                body: Vec::new(),
                display_name,
                description,
                is_collection: true,
                sync_token,
            },
        );
        DavResponse {
            status: 201,
            headers: vec![("ETag".into(), etag)],
            body: Vec::new(),
        }
    }

    fn proppatch(&mut self, path: &str, request: &DavRequest) -> DavResponse {
        let Some(resource) = self.resources.get_mut(path) else {
            return DavResponse {
                status: 404,
                headers: Vec::new(),
                body: b"missing".to_vec(),
            };
        };
        if let Some(status) = Self::precondition(request, Some(resource)) {
            return DavResponse {
                status,
                headers: vec![("ETag".into(), resource.etag.clone())],
                body: b"precondition".to_vec(),
            };
        }
        let xml = String::from_utf8_lossy(&request.body);
        if let Some(name) = xml_local_text(&xml, "displayname") {
            resource.display_name = name;
        }
        if let Some(description) = xml_local_text(&xml, "addressbook-description") {
            resource.description = description;
        }
        let (etag, sync_token) = {
            // bump via copy because we hold resource mutably
            (self.next_etag + 1, self.next_sync + 1)
        };
        self.next_etag = etag;
        self.next_sync = sync_token;
        let etag = format!("\"e{etag}\"");
        let sync_token = format!("sync-{sync_token}");
        if let Some(resource) = self.resources.get_mut(path) {
            resource.etag = etag.clone();
            resource.sync_token = sync_token;
        }
        DavResponse {
            status: 207,
            headers: vec![("ETag".into(), etag)],
            body: b"<multistatus/>".to_vec(),
        }
    }

    fn move_resource(&mut self, path: &str, request: &DavRequest) -> DavResponse {
        let destination = match header(request, "Destination") {
            Some(value) => normalize_href(&strip_origin(&value)),
            None => {
                return DavResponse {
                    status: 400,
                    headers: Vec::new(),
                    body: b"missing destination".to_vec(),
                };
            }
        };
        let existing = self.resources.get(path).cloned();
        if existing.is_none() {
            return DavResponse {
                status: 404,
                headers: Vec::new(),
                body: b"missing".to_vec(),
            };
        }
        if let Some(status) = Self::precondition(request, existing.as_ref()) {
            return DavResponse {
                status,
                headers: existing
                    .map(|resource| vec![("ETag".into(), resource.etag)])
                    .unwrap_or_default(),
                body: b"precondition".to_vec(),
            };
        }
        if self.resources.contains_key(&destination) {
            return DavResponse {
                status: 412,
                headers: Vec::new(),
                body: b"destination exists".to_vec(),
            };
        }
        let mut resource = existing.expect("checked");
        let (etag, sync_token) = self.bump();
        self.resources.remove(path);
        resource.href = destination.clone();
        resource.etag = etag.clone();
        resource.sync_token = sync_token.clone();
        let dest_parent = parent_href(&destination);
        self.resources.insert(destination, resource);
        if let Some(parent) = self.resources.get_mut(&parent_href(path)) {
            parent.sync_token = sync_token.clone();
        }
        if let Some(parent) = self.resources.get_mut(&dest_parent) {
            parent.sync_token = sync_token;
        }
        DavResponse {
            status: 201,
            headers: vec![("ETag".into(), etag)],
            body: Vec::new(),
        }
    }

    fn propfind(&self, path: &str, depth: u8) -> DavResponse {
        let mut hrefs = Vec::new();
        if let Some(resource) = self.resources.get(path) {
            hrefs.push(resource.href.clone());
        }
        if depth > 0 {
            let prefix = if path.ends_with('/') {
                path.to_string()
            } else {
                format!("{path}/")
            };
            for href in self.resources.keys() {
                if href.starts_with(&prefix) && href.as_str() != path {
                    let rest = &href[prefix.len()..];
                    if !rest.trim_end_matches('/').contains('/') {
                        hrefs.push(href.clone());
                    }
                }
            }
        }
        let mut xml = String::from(
            r#"<?xml version="1.0"?><d:multistatus xmlns:d="DAV:" xmlns:card="urn:ietf:params:xml:ns:carddav" xmlns:cs="http://calendarserver.org/ns/">"#,
        );
        for href in hrefs {
            if let Some(resource) = self.resources.get(&href) {
                let _ = write!(
                    xml,
                    "<d:response><d:href>{}</d:href><d:propstat><d:prop>",
                    xml_escape(&resource.href)
                );
                if resource.is_collection {
                    xml.push_str(
                        "<d:resourcetype><d:collection/><card:addressbook/></d:resourcetype>",
                    );
                    let _ = write!(
                        xml,
                        "<d:sync-token>{}</d:sync-token><cs:getctag>{}</cs:getctag>",
                        xml_escape(&resource.sync_token),
                        xml_escape(&resource.sync_token)
                    );
                } else {
                    xml.push_str("<d:resourcetype/>");
                    xml.push_str("<d:getcontenttype>text/vcard</d:getcontenttype>");
                }
                let _ = write!(
                    xml,
                    "<d:displayname>{}</d:displayname><card:addressbook-description>{}</card:addressbook-description><d:getetag>{}</d:getetag></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>",
                    xml_escape(&resource.display_name),
                    xml_escape(&resource.description),
                    xml_escape(&resource.etag)
                );
            }
        }
        xml.push_str("</d:multistatus>");
        DavResponse {
            status: 207,
            headers: vec![("Content-Type".into(), "application/xml".into())],
            body: xml.into_bytes(),
        }
    }
}

fn header(request: &DavRequest, name: &str) -> Option<String> {
    request
        .headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.clone())
}

fn depth(request: &DavRequest) -> u8 {
    header(request, "Depth")
        .and_then(|value| value.parse().ok())
        .unwrap_or(0)
}

struct DavItem {
    href: String,
    etag: Option<String>,
    display_name: Option<String>,
    description: Option<String>,
    is_collection: bool,
    is_addressbook: bool,
    sync_token: Option<String>,
    ctag: Option<String>,
}

fn parse_multistatus(xml: &str) -> Vec<DavItem> {
    let mut items = Vec::new();
    let lower = xml;
    let mut rest = lower;
    while let Some(start) =
        find_ignore_case(rest, "<d:response").or_else(|| find_ignore_case(rest, "<response"))
    {
        let after = &rest[start..];
        let end_at = find_ignore_case(after, "</d:response>")
            .or_else(|| find_ignore_case(after, "</response>"))
            .map(|index| index + 12)
            .unwrap_or(after.len());
        let block = &after[..end_at.min(after.len())];
        let href = xml_local_text(block, "href").unwrap_or_default();
        items.push(DavItem {
            href: normalize_href(&strip_origin(&href)),
            etag: xml_local_text(block, "getetag").map(|value| normalize_etag(&value)),
            display_name: xml_local_text(block, "displayname"),
            description: xml_local_text(block, "addressbook-description"),
            is_collection: contains_ignore_case(block, "<d:collection")
                || contains_ignore_case(block, "<collection"),
            is_addressbook: contains_ignore_case(block, "addressbook"),
            sync_token: xml_local_text(block, "sync-token"),
            ctag: xml_local_text(block, "getctag"),
        });
        rest = &after[end_at.min(after.len())..];
    }
    items
}

fn xml_local_text(xml: &str, local_name: &str) -> Option<String> {
    let lower = xml.to_ascii_lowercase();
    let name = local_name.to_ascii_lowercase();
    let mut search_from = 0;
    while search_from < lower.len() {
        let rel = match lower[search_from..].find(&name) {
            Some(index) => index,
            None => return None,
        };
        let abs = search_from + rel;
        let Some(lt) = xml[..abs].rfind('<') else {
            search_from = abs + 1;
            continue;
        };
        if xml.as_bytes().get(lt + 1) == Some(&b'/') {
            search_from = abs + 1;
            continue;
        }
        let after_name = abs + name.len();
        let next = xml[after_name..].chars().next().unwrap_or('\0');
        if !matches!(next, '>' | ' ' | '/' | '\t' | '\r' | '\n') {
            search_from = abs + 1;
            continue;
        }
        let Some(gt_rel) = xml[abs..].find('>') else {
            return None;
        };
        let gt = abs + gt_rel;
        if xml[..gt].trim_end().ends_with('/') {
            return Some(String::new());
        }
        let content_start = gt + 1;
        let close_plain = format!("</{name}>");
        if let Some(close) = lower[content_start..].find(&close_plain) {
            return Some(unescape_xml(
                xml[content_start..content_start + close].trim(),
            ));
        }
        let close_ns = format!(":{name}>");
        if let Some(close) = lower[content_start..].find(&close_ns) {
            let close_abs = content_start + close;
            if let Some(open) = xml[..close_abs].rfind("</") {
                return Some(unescape_xml(xml[content_start..open].trim()));
            }
        }
        search_from = abs + 1;
    }
    None
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn unescape_xml(value: &str) -> String {
    value
        .replace("&quot;", "\"")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

fn find_ignore_case(haystack: &str, needle: &str) -> Option<usize> {
    haystack
        .to_ascii_lowercase()
        .find(&needle.to_ascii_lowercase())
}

fn contains_ignore_case(haystack: &str, needle: &str) -> bool {
    find_ignore_case(haystack, needle).is_some()
}

pub fn normalize_href(value: &str) -> String {
    let path = strip_origin(value);
    let mut href = if path.starts_with('/') {
        path
    } else {
        format!("/{path}")
    };
    if href.len() > 1 && href.ends_with(".vcf") {
        return href;
    }
    if href.len() > 1 && !href.ends_with('/') && !href.contains('.') {
        href.push('/');
    }
    href
}

fn strip_origin(value: &str) -> String {
    if let Some(idx) = value.find("://") {
        let after = &value[idx + 3..];
        after
            .find('/')
            .map(|slash| after[slash..].to_string())
            .unwrap_or_else(|| "/".into())
    } else {
        value.to_string()
    }
}

fn parent_href(href: &str) -> String {
    let trimmed = href.trim_end_matches('/');
    match trimmed.rsplit_once('/') {
        Some((parent, _)) => normalize_href(parent),
        None => "/".into(),
    }
}

fn normalize_etag(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.starts_with("W/") {
        return normalize_etag(&trimmed[2..]);
    }
    if trimmed.starts_with('"') && trimmed.ends_with('"') && trimmed.len() >= 2 {
        trimmed.to_string()
    } else if trimmed.is_empty() {
        trimmed.to_string()
    } else {
        format!("\"{trimmed}\"")
    }
}

fn etags_match(expected: &str, actual: &str) -> bool {
    if expected.trim() == "*" {
        return true;
    }
    normalize_etag(expected).eq_ignore_ascii_case(&normalize_etag(actual))
}

fn carddav_propfind_body() -> &'static [u8] {
    br#"<?xml version="1.0" encoding="utf-8" ?><d:propfind xmlns:d="DAV:" xmlns:card="urn:ietf:params:xml:ns:carddav" xmlns:cs="http://calendarserver.org/ns/"><d:prop><d:resourcetype/><d:displayname/><d:getetag/><d:sync-token/><cs:getctag/><card:addressbook-description/><d:getcontenttype/></d:prop></d:propfind>"#
}

async fn dav_propfind(dav: &DavBackend, path: &str, depth: u8) -> ContactsResult<Vec<DavItem>> {
    if let DavBackend::Client(client) = dav {
        return client_propfind(client, path, depth).await;
    }
    let response = dav
        .send(DavRequest {
            method: "PROPFIND".into(),
            path: path.into(),
            headers: vec![
                ("Depth".into(), depth.to_string()),
                (
                    "Content-Type".into(),
                    "application/xml; charset=utf-8".into(),
                ),
            ],
            body: carddav_propfind_body().to_vec(),
        })
        .await?;
    if response.status != 207 && response.status != 200 {
        return Err(ContactsError::unavailable(format!(
            "CardDAV PROPFIND failed ({})",
            response.status
        )));
    }
    Ok(parse_multistatus(&response.text()))
}

async fn client_propfind(
    client: &DavClient,
    path: &str,
    depth: u8,
) -> ContactsResult<Vec<DavItem>> {
    let user_id = user_id_from_href(path)?;
    let href = DavHref::parse(path).map_err(shared_to_contacts)?;
    if depth == 0 {
        let collection = client
            .load_collection(&user_id, &href, CollectionKind::AddressBook)
            .await
            .map_err(shared_to_contacts)?;
        return Ok(vec![dav_item_from_collection(&collection)]);
    }
    let collections = client
        .list_collection(&user_id, &href)
        .await
        .map_err(shared_to_contacts)?;
    let mut items = collections
        .iter()
        .map(dav_item_from_collection)
        .collect::<Vec<_>>();
    if let Ok(page) = client.sync_collection(&user_id, &href, None).await {
        for change in page.upserts {
            items.push(DavItem {
                href: change.href.as_str().to_string(),
                etag: change.etag.as_ref().map(|etag| etag.as_str().to_string()),
                display_name: None,
                description: None,
                is_collection: change.href.is_collection(),
                is_addressbook: false,
                sync_token: None,
                ctag: None,
            });
        }
    }
    Ok(items)
}

fn dav_item_from_collection(collection: &crate::dav::DavCollection) -> DavItem {
    DavItem {
        href: collection.href.as_str().to_string(),
        etag: collection
            .etag
            .as_ref()
            .map(|etag| etag.as_str().to_string()),
        display_name: collection.display_name.clone(),
        description: None,
        is_collection: true,
        is_addressbook: collection.kind == CollectionKind::AddressBook,
        sync_token: collection
            .sync_token
            .as_ref()
            .map(|token| token.as_str().to_string()),
        ctag: None,
    }
}

fn user_id_from_href(path: &str) -> ContactsResult<String> {
    path.trim_start_matches('/')
        .split('/')
        .next()
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| ContactsError::invalid_request("DAV href is missing a user segment"))
}

fn shared_to_contacts(error: crate::dav::DavError) -> ContactsError {
    match error {
        crate::dav::DavError::PreconditionFailed { expected, .. } => {
            ContactsError::stale_etag(&expected.unwrap_or_default(), None)
        }
        crate::dav::DavError::NotFound(message) => ContactsError::not_found(message),
        crate::dav::DavError::Conflict(message) => ContactsError::conflict(message),
        crate::dav::DavError::OperationConflict => {
            ContactsError::conflict("this DAV operation id is already bound to a different request")
        }
        crate::dav::DavError::InvalidRequest(message) => ContactsError::invalid_request(message),
        crate::dav::DavError::Gone(message) => ContactsError::gone(message),
        other => ContactsError::unavailable(other.to_string()),
    }
}

fn api_to_contacts(error: crate::error::ApiError) -> ContactsError {
    ContactsError::unavailable(error.to_string())
}

async fn dav_get(dav: &DavBackend, path: &str) -> ContactsResult<(String, String)> {
    if let DavBackend::Client(client) = dav {
        let user_id = user_id_from_href(path)?;
        let href = DavHref::parse(path).map_err(shared_to_contacts)?;
        let resource = client
            .get_resource(&user_id, &href, CollectionKind::AddressBook)
            .await
            .map_err(shared_to_contacts)?;
        return Ok((
            resource.etag.as_str().to_string(),
            resource.payload.raw().to_string(),
        ));
    }
    let response = dav
        .send(DavRequest {
            method: "GET".into(),
            path: path.into(),
            headers: Vec::new(),
            body: Vec::new(),
        })
        .await?;
    if response.status == 404 {
        return Err(ContactsError::not_found("CardDAV resource not found."));
    }
    if response.status >= 300 {
        return Err(ContactsError::unavailable(format!(
            "CardDAV GET failed ({})",
            response.status
        )));
    }
    let etag = response
        .etag()
        .ok_or_else(|| ContactsError::unavailable("CardDAV GET did not return an ETag."))?;
    Ok((etag, response.text()))
}

async fn dav_put(
    dav: &DavBackend,
    path: &str,
    body: &str,
    if_match: Option<&str>,
    if_none_match: bool,
) -> ContactsResult<String> {
    if let DavBackend::Client(client) = dav {
        let user_id = user_id_from_href(path)?;
        let href = DavHref::parse(path).map_err(shared_to_contacts)?;
        let payload =
            DavPayload::from_raw(DavMediaType::VCard, body).map_err(shared_to_contacts)?;
        let precondition = if if_none_match {
            PutPrecondition::IfNoneMatchStar
        } else if let Some(etag) = if_match {
            PutPrecondition::IfMatch(
                ETag::parse(&normalize_etag(etag)).map_err(shared_to_contacts)?,
            )
        } else {
            return Err(ContactsError::invalid_request(
                "conditional CardDAV PUT requires If-Match or If-None-Match",
            ));
        };
        let result = client
            .put_resource(&user_id, &href, &payload, precondition)
            .await
            .map_err(shared_to_contacts)?;
        if let Some(etag) = result.etag {
            return Ok(etag.as_str().to_string());
        }
        return dav_get(dav, path).await.map(|(etag, _)| etag);
    }
    let mut headers = vec![("Content-Type".into(), "text/vcard; charset=utf-8".into())];
    if let Some(etag) = if_match {
        headers.push(("If-Match".into(), normalize_etag(etag)));
    }
    if if_none_match {
        headers.push(("If-None-Match".into(), "*".into()));
    }
    let response = dav
        .send(DavRequest {
            method: "PUT".into(),
            path: path.into(),
            headers,
            body: body.as_bytes().to_vec(),
        })
        .await?;
    if response.status == 412 {
        return Err(ContactsError::stale_etag(
            if_match.unwrap_or("*"),
            response.etag().as_deref(),
        ));
    }
    if response.status >= 300 {
        return Err(ContactsError::unavailable(format!(
            "CardDAV PUT failed ({})",
            response.status
        )));
    }
    if let Some(etag) = response.etag() {
        return Ok(etag);
    }
    dav_get(dav, path).await.map(|(etag, _)| etag)
}

async fn dav_delete(dav: &DavBackend, path: &str, if_match: &str) -> ContactsResult<()> {
    if let DavBackend::Client(client) = dav {
        let user_id = user_id_from_href(path)?;
        let href = DavHref::parse(path).map_err(shared_to_contacts)?;
        let expected = ETag::parse(&normalize_etag(if_match)).map_err(shared_to_contacts)?;
        return match client.delete_resource(&user_id, &href, &expected).await {
            Ok(()) => Ok(()),
            Err(crate::dav::DavError::NotFound(_)) => Ok(()),
            Err(error) => Err(shared_to_contacts(error)),
        };
    }
    let response = dav
        .send(DavRequest {
            method: "DELETE".into(),
            path: path.into(),
            headers: vec![("If-Match".into(), normalize_etag(if_match))],
            body: Vec::new(),
        })
        .await?;
    if response.status == 412 {
        return Err(ContactsError::stale_etag(
            if_match,
            response.etag().as_deref(),
        ));
    }
    if response.status == 404 {
        return Ok(());
    }
    if response.status >= 300 {
        return Err(ContactsError::unavailable(format!(
            "CardDAV DELETE failed ({})",
            response.status
        )));
    }
    Ok(())
}

async fn dav_mkcol(
    dav: &DavBackend,
    path: &str,
    display_name: &str,
    description: &str,
) -> ContactsResult<Option<String>> {
    if let DavBackend::Client(client) = dav {
        let user_id = user_id_from_href(path)?;
        let collection_id = path
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap_or_default()
            .to_string();
        let collection = client
            .create_address_book(
                &user_id,
                &NewAddressBook {
                    collection_id,
                    display_name: display_name.to_string(),
                },
            )
            .await
            .map_err(shared_to_contacts)?;
        let _ = description;
        return Ok(collection.etag.map(|etag| etag.as_str().to_string()));
    }
    let body = format!(
        r#"<?xml version="1.0" encoding="utf-8" ?><d:mkcol xmlns:d="DAV:" xmlns:card="urn:ietf:params:xml:ns:carddav"><d:set><d:prop><d:resourcetype><d:collection/><card:addressbook/></d:resourcetype><d:displayname>{}</d:displayname><card:addressbook-description>{}</card:addressbook-description></d:prop></d:set></d:mkcol>"#,
        xml_escape(display_name),
        xml_escape(description)
    );
    let response = dav
        .send(DavRequest {
            method: "MKCOL".into(),
            path: path.into(),
            headers: vec![(
                "Content-Type".into(),
                "application/xml; charset=utf-8".into(),
            )],
            body: body.into_bytes(),
        })
        .await?;
    if response.status == 405 || response.status == 412 {
        return Err(ContactsError::conflict(
            "An address book already exists at this href.",
        ));
    }
    if response.status >= 300 {
        return Err(ContactsError::unavailable(format!(
            "CardDAV MKCOL failed ({})",
            response.status
        )));
    }
    Ok(response.etag())
}

async fn dav_proppatch(
    dav: &DavBackend,
    path: &str,
    display_name: Option<&str>,
    description: Option<&str>,
    if_match: Option<&str>,
) -> ContactsResult<Option<String>> {
    if let DavBackend::Client(client) = dav {
        let user_id = user_id_from_href(path)?;
        let href = DavHref::parse(path).map_err(shared_to_contacts)?;
        if let Some(name) = display_name {
            client
                .set_display_name(&user_id, &href, name, None)
                .await
                .map_err(shared_to_contacts)?;
        }
        let _ = (description, if_match);
        return Ok(None);
    }
    let mut props = String::new();
    if let Some(name) = display_name {
        let _ = write!(props, "<d:displayname>{}</d:displayname>", xml_escape(name));
    }
    if let Some(description) = description {
        let _ = write!(
            props,
            "<card:addressbook-description>{}</card:addressbook-description>",
            xml_escape(description)
        );
    }
    let body = format!(
        r#"<?xml version="1.0" encoding="utf-8" ?><d:propertyupdate xmlns:d="DAV:" xmlns:card="urn:ietf:params:xml:ns:carddav"><d:set><d:prop>{props}</d:prop></d:set></d:propertyupdate>"#
    );
    let mut headers = vec![(
        "Content-Type".into(),
        "application/xml; charset=utf-8".into(),
    )];
    if let Some(etag) = if_match {
        headers.push(("If-Match".into(), normalize_etag(etag)));
    }
    let response = dav
        .send(DavRequest {
            method: "PROPPATCH".into(),
            path: path.into(),
            headers,
            body: body.into_bytes(),
        })
        .await?;
    if response.status == 412 {
        return Err(ContactsError::stale_etag(
            if_match.unwrap_or("*"),
            response.etag().as_deref(),
        ));
    }
    if response.status >= 300 && response.status != 207 {
        return Err(ContactsError::unavailable(format!(
            "CardDAV PROPPATCH failed ({})",
            response.status
        )));
    }
    Ok(response.etag())
}

async fn dav_move(
    dav: &DavBackend,
    path: &str,
    destination: &str,
    if_match: &str,
    absolute_destination: Option<&str>,
) -> ContactsResult<Option<String>> {
    if let DavBackend::Client(client) = dav {
        let user_id = user_id_from_href(path)?;
        let href = DavHref::parse(path).map_err(shared_to_contacts)?;
        let dest = DavHref::parse(destination).map_err(shared_to_contacts)?;
        let expected = ETag::parse(&normalize_etag(if_match)).map_err(shared_to_contacts)?;
        let result = client
            .move_resource(&user_id, &href, &dest, &expected)
            .await
            .map_err(shared_to_contacts)?;
        let _ = absolute_destination;
        return Ok(result.etag.map(|etag| etag.as_str().to_string()));
    }
    let dest_header = absolute_destination
        .map(str::to_string)
        .unwrap_or_else(|| destination.to_string());
    let response = dav
        .send(DavRequest {
            method: "MOVE".into(),
            path: path.into(),
            headers: vec![
                ("Destination".into(), dest_header),
                ("Overwrite".into(), "F".into()),
                ("If-Match".into(), normalize_etag(if_match)),
            ],
            body: Vec::new(),
        })
        .await?;
    if response.status == 412 {
        return Err(ContactsError::stale_etag(
            if_match,
            response.etag().as_deref(),
        ));
    }
    if response.status >= 300 {
        return Err(ContactsError::unavailable(format!(
            "CardDAV MOVE failed ({})",
            response.status
        )));
    }
    Ok(response.etag())
}

#[derive(Clone)]
pub struct ContactsService {
    pub pool: PgPool,
    pub dav: DavBackend,
    pub principal_path: String,
    pub public_origin: Option<String>,
}

impl ContactsService {
    pub fn new(pool: PgPool, dav: DavBackend, principal_path: impl Into<String>) -> Self {
        Self {
            pool,
            dav,
            principal_path: normalize_href(&principal_path.into()),
            public_origin: None,
        }
    }

    pub fn from_state(state: &AppState, user_id: &str) -> ContactsResult<Self> {
        Ok(Self {
            pool: state.pool.clone(),
            dav: DavBackend::Client(state.dav_client().map_err(api_to_contacts)?.clone()),
            // The service account owns multiple logical Foyer users. Project
            // this user's CardDAV home directly; a depth-one listing of the
            // principal only contains the `addressbooks` container and would
            // incorrectly tombstone every address book.
            principal_path: format!("/{user_id}/addressbooks/"),
            public_origin: None,
        })
    }

    fn book_href(&self, user_id: &str, uid: &str) -> String {
        format!("/{}/addressbooks/{}/", user_id, uid)
    }

    pub fn contact_href(&self, user_id: &str, book_uid: &str, contact_uid: &str) -> String {
        join_href(
            &self.book_href(user_id, book_uid),
            &format!("{contact_uid}.vcf"),
        )
    }

    pub fn absolute_href(&self, href: &str) -> Option<String> {
        self.public_origin
            .as_ref()
            .map(|origin| format!("{}{href}", origin.trim_end_matches('/')))
    }
}

fn join_href(parent: &str, child: &str) -> String {
    let parent = parent.trim_end_matches('/');
    let child = child.trim_start_matches('/');
    normalize_href(&format!("{parent}/{child}"))
}

pub fn parse_uuid(field: &str, value: &str) -> ContactsResult<String> {
    Uuid::parse_str(value)
        .map(|uuid| uuid.to_string())
        .map_err(|_| ContactsError::invalid_request(format!("{field} must be a UUID.")))
}

fn validate_book_name(value: &str) -> ContactsResult<String> {
    validate_text("displayName", value, 1, MAX_BOOK_NAME)
}

fn validate_book_description(value: &str) -> ContactsResult<String> {
    if value.chars().count() > MAX_BOOK_DESCRIPTION {
        return Err(ContactsError::invalid_request(format!(
            "description must be at most {MAX_BOOK_DESCRIPTION} characters."
        )));
    }
    if value.contains('\0') {
        return Err(ContactsError::invalid_request(
            "description cannot contain NUL bytes.",
        ));
    }
    Ok(value.to_string())
}

fn validate_text(field: &str, value: &str, min: usize, max: usize) -> ContactsResult<String> {
    let trimmed = value.trim();
    if trimmed.chars().count() < min || trimmed.chars().count() > max {
        return Err(ContactsError::invalid_request(format!(
            "{field} must be between {min} and {max} characters."
        )));
    }
    if trimmed.contains('\0') || trimmed.chars().any(|ch| ch.is_control()) {
        return Err(ContactsError::invalid_request(format!(
            "{field} cannot contain control characters."
        )));
    }
    Ok(trimmed.to_string())
}

fn validate_optional_text(
    field: &str,
    value: Option<&str>,
    max: usize,
) -> ContactsResult<Option<String>> {
    match value {
        None => Ok(None),
        Some(value) if value.trim().is_empty() => Ok(Some(String::new())),
        Some(value) => validate_text(field, value, 0, max).map(Some),
    }
}

fn validate_name_part(field: &str, value: &str) -> ContactsResult<String> {
    if value.chars().count() > MAX_NAME_PART {
        return Err(ContactsError::invalid_request(format!(
            "{field} must be at most {MAX_NAME_PART} characters."
        )));
    }
    if value.contains('\0') || value.chars().any(|ch| ch.is_control()) {
        return Err(ContactsError::invalid_request(format!(
            "{field} cannot contain control characters."
        )));
    }
    Ok(value.trim().to_string())
}

fn validate_structured_name(name: &StructuredName) -> ContactsResult<StructuredName> {
    Ok(StructuredName {
        family_name: validate_name_part("familyName", &name.family_name)?,
        given_name: validate_name_part("givenName", &name.given_name)?,
        additional_names: validate_name_part("additionalNames", &name.additional_names)?,
        honorific_prefix: validate_name_part("honorificPrefix", &name.honorific_prefix)?,
        honorific_suffix: validate_name_part("honorificSuffix", &name.honorific_suffix)?,
    })
}

fn validate_email(email: &TypedEmail) -> ContactsResult<TypedEmail> {
    let value = email.value.trim().to_string();
    if value.is_empty() || value.len() > MAX_EMAIL || !value.contains('@') || value.contains(' ') {
        return Err(ContactsError::invalid_request(
            "email values must be addr-spec strings at most 254 characters.",
        ));
    }
    if value.contains('\0') || value.chars().any(|ch| ch.is_control()) {
        return Err(ContactsError::invalid_request(
            "email values cannot contain control characters.",
        ));
    }
    Ok(TypedEmail {
        value,
        r#type: normalize_type(&email.r#type),
        pref: email.pref,
    })
}

fn validate_phone(phone: &TypedPhone) -> ContactsResult<TypedPhone> {
    let value = phone.value.trim().to_string();
    if value.is_empty() || value.chars().count() > MAX_PHONE {
        return Err(ContactsError::invalid_request(format!(
            "phone values must be between 1 and {MAX_PHONE} characters."
        )));
    }
    if !value.chars().all(|ch| {
        ch.is_ascii_digit() || matches!(ch, ' ' | '+' | '-' | '(' | ')' | '.' | '/' | 'x' | 'X')
    }) {
        return Err(ContactsError::invalid_request(
            "phone values may contain digits, spaces, and + - ( ) . / x only.",
        ));
    }
    Ok(TypedPhone {
        value,
        r#type: normalize_type(&phone.r#type),
        pref: phone.pref,
    })
}

fn validate_address(address: &PostalAddress) -> ContactsResult<PostalAddress> {
    let check = |field: &str, value: &str| -> ContactsResult<String> {
        if value.chars().count() > MAX_ADDRESS_LINE {
            return Err(ContactsError::invalid_request(format!(
                "{field} must be at most {MAX_ADDRESS_LINE} characters."
            )));
        }
        if value.contains('\0') || value.chars().any(|ch| ch.is_control() && ch != '\n') {
            return Err(ContactsError::invalid_request(format!(
                "{field} cannot contain control characters."
            )));
        }
        Ok(value.to_string())
    };
    let parsed = PostalAddress {
        po_box: check("poBox", &address.po_box)?,
        extended: check("extended", &address.extended)?,
        street: check("street", &address.street)?,
        locality: check("locality", &address.locality)?,
        region: check("region", &address.region)?,
        postal_code: check("postalCode", &address.postal_code)?,
        country: check("country", &address.country)?,
        r#type: normalize_type(&address.r#type),
        pref: address.pref,
    };
    if parsed.is_empty() {
        return Err(ContactsError::invalid_request(
            "A postal address must include at least one line.",
        ));
    }
    Ok(parsed)
}

fn validate_notes(value: &str) -> ContactsResult<String> {
    if value.chars().count() > MAX_NOTE_CHARS {
        return Err(ContactsError::invalid_request(format!(
            "notes must be at most {MAX_NOTE_CHARS} characters."
        )));
    }
    if value.contains('\0') {
        return Err(ContactsError::invalid_request(
            "notes cannot contain NUL bytes.",
        ));
    }
    Ok(value.to_string())
}

fn parse_birthday(value: &str) -> ContactsResult<Option<String>> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let compact = trimmed.replace('-', "");
    if compact.len() == 8 && compact.chars().all(|ch| ch.is_ascii_digit()) {
        let year: i32 = compact[0..4].parse().unwrap_or(0);
        let month: u32 = compact[4..6].parse().unwrap_or(0);
        let day: u32 = compact[6..8].parse().unwrap_or(0);
        NaiveDate::from_ymd_opt(year, month, day).ok_or_else(|| {
            ContactsError::invalid_request("birthday must be a valid calendar date.")
        })?;
        return Ok(Some(format!(
            "{year:04}-{month:02}-{day:02}",
            year = year,
            month = month,
            day = day
        )));
    }
    Err(ContactsError::invalid_request(
        "birthday must be YYYY-MM-DD or YYYYMMDD.",
    ))
}

fn validate_create_contact(request: &CreateContactRequest) -> ContactsResult<CreateContactRequest> {
    let name = match &request.name {
        Some(name) => Some(validate_structured_name(name)?),
        None => None,
    };
    let emails = validate_bounded_list("emails", &request.emails, MAX_EMAILS, validate_email)?;
    let phones = validate_bounded_list("phones", &request.phones, MAX_PHONES, validate_phone)?;
    let addresses = validate_bounded_list(
        "addresses",
        &request.addresses,
        MAX_ADDRESSES,
        validate_address,
    )?;
    let display_name = match &request.display_name {
        Some(value) if !value.trim().is_empty() => {
            Some(validate_text("displayName", value, 1, MAX_DISPLAY_NAME)?)
        }
        _ => None,
    };
    let derived = CreateContactRequest {
        operation_id: request.operation_id.clone(),
        id: request.id.clone(),
        address_book_id: request.address_book_id.clone(),
        display_name,
        name,
        emails,
        phones,
        organization: validate_optional_text(
            "organization",
            request.organization.as_deref(),
            MAX_ORGANIZATION,
        )?,
        job_title: validate_optional_text("jobTitle", request.job_title.as_deref(), MAX_JOB_TITLE)?,
        addresses,
        birthday: match &request.birthday {
            Some(value) => parse_birthday(value)?,
            None => None,
        },
        notes: match &request.notes {
            Some(value) => Some(validate_notes(value)?),
            None => None,
        },
    };
    let has_identity = derived
        .display_name
        .as_ref()
        .is_some_and(|value| !value.is_empty())
        || derived.name.as_ref().is_some_and(|name| !name.is_empty())
        || !derived.emails.is_empty()
        || !derived.phones.is_empty()
        || derived
            .organization
            .as_ref()
            .is_some_and(|value| !value.is_empty());
    if !has_identity {
        return Err(ContactsError::invalid_request(
            "A contact needs a display name, structured name, email, phone, or organization.",
        ));
    }
    Ok(derived)
}

fn validate_update_contact(request: &UpdateContactRequest) -> ContactsResult<UpdateContactRequest> {
    Ok(UpdateContactRequest {
        operation_id: request.operation_id.clone(),
        expected_etag: request.expected_etag.clone(),
        expected_revision: request.expected_revision,
        display_name: match &request.display_name {
            Some(value) if value.trim().is_empty() => Some(String::new()),
            Some(value) => Some(validate_text("displayName", value, 1, MAX_DISPLAY_NAME)?),
            None => None,
        },
        name: match &request.name {
            Some(name) => Some(validate_structured_name(name)?),
            None => None,
        },
        emails: match &request.emails {
            Some(emails) => Some(validate_bounded_list(
                "emails",
                emails,
                MAX_EMAILS,
                validate_email,
            )?),
            None => None,
        },
        phones: match &request.phones {
            Some(phones) => Some(validate_bounded_list(
                "phones",
                phones,
                MAX_PHONES,
                validate_phone,
            )?),
            None => None,
        },
        organization: validate_optional_text(
            "organization",
            request.organization.as_deref(),
            MAX_ORGANIZATION,
        )?,
        job_title: validate_optional_text("jobTitle", request.job_title.as_deref(), MAX_JOB_TITLE)?,
        addresses: match &request.addresses {
            Some(addresses) => Some(validate_bounded_list(
                "addresses",
                addresses,
                MAX_ADDRESSES,
                validate_address,
            )?),
            None => None,
        },
        birthday: match &request.birthday {
            Some(value) if value.trim().is_empty() => Some(String::new()),
            Some(value) => parse_birthday(value)?.or(Some(String::new())),
            None => None,
        },
        notes: match &request.notes {
            Some(value) => Some(validate_notes(value)?),
            None => None,
        },
    })
}

fn validate_bounded_list<T, U>(
    field: &str,
    items: &[T],
    max: usize,
    validate: impl Fn(&T) -> ContactsResult<U>,
) -> ContactsResult<Vec<U>> {
    if items.len() > max {
        return Err(ContactsError::invalid_request(format!(
            "{field} may contain at most {max} values."
        )));
    }
    items.iter().map(validate).collect()
}

fn validate_revision(value: Option<i64>) -> ContactsResult<Option<i64>> {
    match value {
        None => Ok(None),
        Some(value) if value < 1 => Err(ContactsError::invalid_request(
            "expectedRevision must be at least 1.",
        )),
        Some(value) => Ok(Some(value)),
    }
}

fn require_precondition(etag: Option<&str>, revision: Option<i64>) -> ContactsResult<()> {
    if etag.is_none() && revision.is_none() {
        return Err(ContactsError::invalid_request(
            "expectedEtag or expectedRevision is required.",
        ));
    }
    Ok(())
}

fn uid_from_contact_id(id: &str) -> String {
    format!("urn:uuid:{id}")
}

fn id_from_uid(uid: &str) -> String {
    let trimmed = uid
        .strip_prefix("urn:uuid:")
        .or_else(|| uid.strip_prefix("URN:UUID:"))
        .unwrap_or(uid);
    if Uuid::parse_str(trimmed).is_ok() {
        return trimmed.to_string();
    }
    stable_uuid("foyer.contact.uid", uid)
}

fn stable_uuid(namespace: &str, name: &str) -> String {
    let mut acc: u128 = 0x6ba7b810_9dad_11d1_80b4_00c04fd430c8;
    for byte in namespace
        .bytes()
        .chain(std::iter::once(0))
        .chain(name.bytes())
    {
        acc ^= byte as u128;
        acc = acc.wrapping_mul(0x9e37_79b9_7f4a_7c15_f39c_c060_5ced_c835);
        acc ^= acc >> 47;
    }
    let mut bytes = acc.to_be_bytes();
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes).to_string()
}

pub async fn list_address_books(pool: &PgPool, user_id: &str) -> ContactsResult<AddressBookList> {
    let address_books = sqlx::query_as::<_, AddressBook>(
        "SELECT id, user_id, uid, href, etag, display_name, description, sync_token, ctag,
                revision, created_at, updated_at, deleted_at
         FROM contacts_address_books
         WHERE user_id = $1 AND deleted_at IS NULL
         ORDER BY display_name, id",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await
    .map_err(database_error)?;
    Ok(AddressBookList { address_books })
}

pub async fn get_address_book(
    pool: &PgPool,
    user_id: &str,
    address_book_id: &str,
) -> ContactsResult<AddressBook> {
    let book = load_book(pool, address_book_id).await?;
    visible_book(user_id, book)
}

pub async fn list_contacts(
    pool: &PgPool,
    user_id: &str,
    address_book_id: Option<&str>,
) -> ContactsResult<ContactList> {
    let rows = if let Some(address_book_id) = address_book_id {
        sqlx::query_as::<_, ContactRow>(
            "SELECT id, user_id, address_book_id, uid, href, etag, display_name, given_name,
                    family_name, additional_names, honorific_prefix, honorific_suffix,
                    organization, job_title, birthday, notes, emails, phones, addresses,
                    revision, created_at, updated_at, deleted_at
             FROM contacts
             WHERE user_id = $1 AND address_book_id = $2 AND deleted_at IS NULL
             ORDER BY display_name, id",
        )
        .bind(user_id)
        .bind(address_book_id)
        .fetch_all(pool)
        .await
        .map_err(database_error)?
    } else {
        sqlx::query_as::<_, ContactRow>(
            "SELECT id, user_id, address_book_id, uid, href, etag, display_name, given_name,
                    family_name, additional_names, honorific_prefix, honorific_suffix,
                    organization, job_title, birthday, notes, emails, phones, addresses,
                    revision, created_at, updated_at, deleted_at
             FROM contacts
             WHERE user_id = $1 AND deleted_at IS NULL
             ORDER BY display_name, id",
        )
        .bind(user_id)
        .fetch_all(pool)
        .await
        .map_err(database_error)?
    };
    Ok(ContactList {
        contacts: rows.into_iter().map(Contact::from).collect(),
    })
}

pub async fn get_contact(
    pool: &PgPool,
    user_id: &str,
    contact_id: &str,
) -> ContactsResult<Contact> {
    let contact = load_contact(pool, contact_id).await?;
    visible_contact(user_id, contact)
}

pub async fn create_address_book(
    service: &ContactsService,
    user_id: &str,
    request: CreateAddressBookRequest,
) -> ContactsResult<AddressBook> {
    let request_body = operation_request(&request)?;
    let operation_id = parse_uuid("operationId", &request.operation_id)?;
    let id = parse_uuid("id", &request.id)?;
    let display_name = validate_book_name(&request.display_name)?;
    let description = validate_book_description(request.description.as_deref().unwrap_or(""))?;
    let href = service.book_href(&user_id, &id);
    let user_id = user_id.to_string();
    let dav = service.dav.clone();
    with_operation(
        &service.pool,
        operation_binding(
            user_id.clone(),
            operation_id,
            "address_book",
            id.clone(),
            "create",
            request_body,
        ),
        move |tx| {
            Box::pin(async move {
                if let Some(existing) = load_book_tx(tx, &id).await? {
                    return Err(existing_identity_error(
                        &user_id,
                        existing.user_id,
                        existing.deleted_at.is_some(),
                    ));
                }
                ensure_book_quota(tx, &user_id).await?;
                let etag = match dav_mkcol(&dav, &href, &display_name, &description).await {
                    Ok(etag) => etag,
                    Err(error) if error.code() == "conflict" => {
                        let items = dav_propfind(&dav, &href, 0).await.unwrap_or_default();
                        items.into_iter().next().and_then(|item| item.etag)
                    }
                    Err(error) => return Err(error),
                };
                let now = Utc::now();
                sqlx::query(
                    "INSERT INTO contacts_address_books
                        (id, user_id, uid, href, etag, display_name, description, sync_token, ctag,
                         revision, created_at, updated_at, deleted_at)
                     VALUES ($1, $2, $3, $4, $5, $6, $7, NULL, NULL, 1, $8, $8, NULL)",
                )
                .bind(&id)
                .bind(&user_id)
                .bind(&id)
                .bind(&href)
                .bind(etag)
                .bind(&display_name)
                .bind(&description)
                .bind(now)
                .execute(&mut **tx)
                .await
                .map_err(database_error)?;
                load_required_book(tx, &id).await
            })
        },
    )
    .await
}

pub async fn update_address_book(
    service: &ContactsService,
    user_id: &str,
    address_book_id: &str,
    request: UpdateAddressBookRequest,
) -> ContactsResult<AddressBook> {
    let request_body = operation_request(&request)?;
    let operation_id = parse_uuid("operationId", &request.operation_id)?;
    let address_book_id = parse_uuid("addressBookId", address_book_id)?;
    let expected_revision = validate_revision(request.expected_revision)?;
    require_precondition(request.expected_etag.as_deref(), expected_revision)?;
    let display_name = match &request.display_name {
        Some(value) => Some(validate_book_name(value)?),
        None => None,
    };
    let description = match &request.description {
        Some(value) => Some(validate_book_description(value)?),
        None => None,
    };
    let expected_etag = request.expected_etag.clone();
    let user_id = user_id.to_string();
    let dav = service.dav.clone();
    with_operation(
        &service.pool,
        operation_binding(
            user_id.clone(),
            operation_id,
            "address_book",
            address_book_id.clone(),
            "update",
            request_body,
        ),
        move |tx| {
            Box::pin(async move {
                let book = locked_live_book(
                    tx,
                    &user_id,
                    &address_book_id,
                    expected_etag.as_deref(),
                    expected_revision,
                )
                .await?;
                let next_name = display_name.unwrap_or_else(|| book.display_name.clone());
                let next_description = description.unwrap_or_else(|| book.description.clone());
                let etag = dav_proppatch(
                    &dav,
                    &book.href,
                    Some(&next_name),
                    Some(&next_description),
                    book.etag.as_deref(),
                )
                .await?;
                let now = Utc::now();
                sqlx::query(
                    "UPDATE contacts_address_books
                     SET display_name = $2, description = $3, etag = COALESCE($4, etag),
                         revision = revision + 1, updated_at = $5
                     WHERE id = $1",
                )
                .bind(&book.id)
                .bind(&next_name)
                .bind(&next_description)
                .bind(etag)
                .bind(now)
                .execute(&mut **tx)
                .await
                .map_err(database_error)?;
                load_required_book(tx, &book.id).await
            })
        },
    )
    .await
}

pub async fn delete_address_book(
    service: &ContactsService,
    user_id: &str,
    address_book_id: &str,
    request: DeleteRequest,
) -> ContactsResult<AddressBook> {
    let request_body = operation_request(&request)?;
    let operation_id = parse_uuid("operationId", &request.operation_id)?;
    let address_book_id = parse_uuid("addressBookId", address_book_id)?;
    let expected_revision = validate_revision(request.expected_revision)?;
    require_precondition(request.expected_etag.as_deref(), expected_revision)?;
    let expected_etag = request.expected_etag.clone();
    let user_id = user_id.to_string();
    let dav = service.dav.clone();
    with_operation(
        &service.pool,
        operation_binding(
            user_id.clone(),
            operation_id,
            "address_book",
            address_book_id.clone(),
            "delete",
            request_body,
        ),
        move |tx| {
            Box::pin(async move {
                let book = locked_live_book(
                    tx,
                    &user_id,
                    &address_book_id,
                    expected_etag.as_deref(),
                    expected_revision,
                )
                .await?;
                let live = sqlx::query_scalar::<_, i64>(
                    "SELECT COUNT(*) FROM contacts
                     WHERE user_id = $1 AND address_book_id = $2 AND deleted_at IS NULL",
                )
                .bind(&user_id)
                .bind(&book.id)
                .fetch_one(&mut **tx)
                .await
                .map_err(database_error)?;
                if live > 0 {
                    return Err(ContactsError::address_book_not_empty(
                        "An address book can be deleted only when it has no live contacts.",
                    ));
                }
                if let Some(etag) = book.etag.as_deref() {
                    dav_delete(&dav, &book.href, etag).await?;
                } else {
                    dav_delete(&dav, &book.href, "*").await?;
                }
                let now = Utc::now();
                sqlx::query(
                    "UPDATE contacts_address_books
                     SET deleted_at = $2, revision = revision + 1, updated_at = $2
                     WHERE id = $1",
                )
                .bind(&book.id)
                .bind(now)
                .execute(&mut **tx)
                .await
                .map_err(database_error)?;
                load_required_book(tx, &book.id).await
            })
        },
    )
    .await
}

pub async fn create_contact(
    service: &ContactsService,
    user_id: &str,
    request: CreateContactRequest,
) -> ContactsResult<Contact> {
    let request_body = operation_request(&request)?;
    let operation_id = parse_uuid("operationId", &request.operation_id)?;
    let id = parse_uuid("id", &request.id)?;
    let address_book_id = parse_uuid("addressBookId", &request.address_book_id)?;
    let validated = validate_create_contact(&request)?;
    let uid = uid_from_contact_id(&id);
    let user_id = user_id.to_string();
    let dav = service.dav.clone();
    with_operation(
        &service.pool,
        operation_binding(
            user_id.clone(),
            operation_id,
            "contact",
            id.clone(),
            "create",
            request_body,
        ),
        move |tx| {
            Box::pin(async move {
                if let Some(existing) = load_contact_tx(tx, &id).await? {
                    return Err(existing_identity_error(
                        &user_id,
                        existing.user_id,
                        existing.deleted_at.is_some(),
                    ));
                }
                let book = ensure_live_book(tx, &user_id, &address_book_id).await?;
                ensure_contact_quota(tx, &user_id).await?;
                let href = join_href(&book.href, &format!("{id}.vcf"));
                let card = vcard_from_create(&uid, &validated)?;
                let serialized = serialize_vcard(&card);
                let etag = match dav_put(&dav, &href, &serialized, None, true).await {
                    Ok(etag) => etag,
                    Err(error) if error.code() == "stale_etag" => {
                        recover_create(&dav, &href, &uid, &validated).await?
                    }
                    Err(error) => return Err(error),
                };
                let fields = contact_fields_from_vcard(&card);
                upsert_contact_row(
                    tx,
                    NewContactRow {
                        id: id.clone(),
                        user_id: user_id.clone(),
                        address_book_id: book.id.clone(),
                        uid,
                        href,
                        etag,
                        fields,
                        now: Utc::now(),
                    },
                )
                .await?;
                refresh_book_collection_tag(tx, &dav, &book).await?;
                load_required_contact(tx, &id).await
            })
        },
    )
    .await
}

pub async fn update_contact(
    service: &ContactsService,
    user_id: &str,
    contact_id: &str,
    request: UpdateContactRequest,
) -> ContactsResult<Contact> {
    let request_body = operation_request(&request)?;
    let operation_id = parse_uuid("operationId", &request.operation_id)?;
    let contact_id = parse_uuid("contactId", contact_id)?;
    let expected_revision = validate_revision(request.expected_revision)?;
    require_precondition(request.expected_etag.as_deref(), expected_revision)?;
    let validated = validate_update_contact(&request)?;
    let expected_etag = request.expected_etag.clone();
    let user_id = user_id.to_string();
    let dav = service.dav.clone();
    with_operation(
        &service.pool,
        operation_binding(
            user_id.clone(),
            operation_id,
            "contact",
            contact_id.clone(),
            "update",
            request_body,
        ),
        move |tx| {
            Box::pin(async move {
                let contact = locked_live_contact(
                    tx,
                    &user_id,
                    &contact_id,
                    expected_etag.as_deref(),
                    expected_revision,
                )
                .await?;
                let (current_etag, raw) = dav_get(&dav, &contact.href).await?;
                if !etags_match(&contact.etag, &current_etag) {
                    return Err(ContactsError::stale_etag(
                        &contact.etag,
                        Some(&current_etag),
                    ));
                }
                let mut card = parse_vcard(&raw)?;
                card.ensure_envelope(&contact.uid);
                apply_contact_patch(&mut card, &ContactPatch::from(&validated))?;
                let serialized = serialize_vcard(&card);
                let etag =
                    match dav_put(&dav, &contact.href, &serialized, Some(&current_etag), false)
                        .await
                    {
                        Ok(etag) => etag,
                        Err(error) if error.code() == "stale_etag" => {
                            recover_update(&dav, &contact.href, &ContactPatch::from(&validated))
                                .await?
                        }
                        Err(error) => return Err(error),
                    };
                let fields = contact_fields_from_vcard(&card);
                update_contact_row(
                    tx,
                    &contact.id,
                    &contact.address_book_id,
                    &contact.href,
                    &etag,
                    &fields,
                )
                .await?;
                if let Ok(book) = load_required_book(tx, &contact.address_book_id).await {
                    refresh_book_collection_tag(tx, &dav, &book).await?;
                }
                load_required_contact(tx, &contact.id).await
            })
        },
    )
    .await
}

pub async fn move_contact(
    service: &ContactsService,
    user_id: &str,
    contact_id: &str,
    request: MoveContactRequest,
) -> ContactsResult<Contact> {
    let request_body = operation_request(&request)?;
    let operation_id = parse_uuid("operationId", &request.operation_id)?;
    let contact_id = parse_uuid("contactId", contact_id)?;
    let address_book_id = parse_uuid("addressBookId", &request.address_book_id)?;
    let expected_revision = validate_revision(request.expected_revision)?;
    require_precondition(request.expected_etag.as_deref(), expected_revision)?;
    let expected_etag = request.expected_etag.clone();
    let user_id = user_id.to_string();
    let dav = service.dav.clone();
    let origin = service.public_origin.clone();
    with_operation(
        &service.pool,
        operation_binding(
            user_id.clone(),
            operation_id,
            "contact",
            contact_id.clone(),
            "move",
            request_body,
        ),
        move |tx| {
            Box::pin(async move {
                let contact = locked_live_contact(
                    tx,
                    &user_id,
                    &contact_id,
                    expected_etag.as_deref(),
                    expected_revision,
                )
                .await?;
                let book = ensure_live_book(tx, &user_id, &address_book_id).await?;
                if book.id == contact.address_book_id {
                    return load_required_contact(tx, &contact.id).await;
                }
                let destination =
                    join_href(&book.href, &format!("{}.vcf", file_stem(&contact.href)));
                let absolute = origin
                    .as_ref()
                    .map(|base| format!("{}{destination}", base.trim_end_matches('/')));
                let etag = dav_move(
                    &dav,
                    &contact.href,
                    &destination,
                    &contact.etag,
                    absolute.as_deref(),
                )
                .await?
                .unwrap_or(contact.etag.clone());
                let now = Utc::now();
                sqlx::query(
                    "UPDATE contacts
                     SET address_book_id = $2, href = $3, etag = $4, revision = revision + 1,
                         updated_at = $5
                     WHERE id = $1",
                )
                .bind(&contact.id)
                .bind(&book.id)
                .bind(&destination)
                .bind(etag)
                .bind(now)
                .execute(&mut **tx)
                .await
                .map_err(database_error)?;
                refresh_book_collection_tag(tx, &dav, &book).await?;
                if let Ok(old_book) = load_required_book(tx, &contact.address_book_id).await {
                    refresh_book_collection_tag(tx, &dav, &old_book).await?;
                }
                load_required_contact(tx, &contact.id).await
            })
        },
    )
    .await
}

pub async fn delete_contact(
    service: &ContactsService,
    user_id: &str,
    contact_id: &str,
    request: DeleteRequest,
) -> ContactsResult<Contact> {
    let request_body = operation_request(&request)?;
    let operation_id = parse_uuid("operationId", &request.operation_id)?;
    let contact_id = parse_uuid("contactId", contact_id)?;
    let expected_revision = validate_revision(request.expected_revision)?;
    require_precondition(request.expected_etag.as_deref(), expected_revision)?;
    let expected_etag = request.expected_etag.clone();
    let user_id = user_id.to_string();
    let dav = service.dav.clone();
    with_operation(
        &service.pool,
        operation_binding(
            user_id.clone(),
            operation_id,
            "contact",
            contact_id.clone(),
            "delete",
            request_body,
        ),
        move |tx| {
            Box::pin(async move {
                let contact = locked_live_contact(
                    tx,
                    &user_id,
                    &contact_id,
                    expected_etag.as_deref(),
                    expected_revision,
                )
                .await?;
                dav_delete(&dav, &contact.href, &contact.etag).await?;
                let now = Utc::now();
                sqlx::query(
                    "UPDATE contacts
                     SET deleted_at = $2, revision = revision + 1, updated_at = $2
                     WHERE id = $1",
                )
                .bind(&contact.id)
                .bind(now)
                .execute(&mut **tx)
                .await
                .map_err(database_error)?;
                if let Ok(book) = load_required_book(tx, &contact.address_book_id).await {
                    refresh_book_collection_tag(tx, &dav, &book).await?;
                }
                load_required_contact(tx, &contact.id).await
            })
        },
    )
    .await
}

async fn refresh_book_collection_tag(
    tx: &mut Transaction<'_, Postgres>,
    dav: &DavBackend,
    book: &AddressBook,
) -> ContactsResult<()> {
    let items = dav_propfind(dav, &book.href, 0).await.unwrap_or_default();
    let Some(item) = items.into_iter().next() else {
        return Ok(());
    };
    sqlx::query(
        "UPDATE contacts_address_books
         SET etag = COALESCE($2, etag),
             sync_token = COALESCE($3, sync_token),
             ctag = COALESCE($4, ctag),
             updated_at = NOW()
         WHERE id = $1",
    )
    .bind(&book.id)
    .bind(&item.etag)
    .bind(&item.sync_token)
    .bind(&item.ctag)
    .execute(&mut **tx)
    .await
    .map_err(database_error)?;
    Ok(())
}

async fn recover_create(
    dav: &DavBackend,
    href: &str,
    uid: &str,
    request: &CreateContactRequest,
) -> ContactsResult<String> {
    let (etag, raw) = dav_get(dav, href).await?;
    let card = parse_vcard(&raw)?;
    let fields = contact_fields_from_vcard(&card);
    if !fields.uid.is_empty() && fields.uid != uid {
        return Err(ContactsError::conflict(
            "A different contact already occupies this href.",
        ));
    }
    let desired = contact_fields_from_vcard(&vcard_from_create(uid, request)?);
    if fields_match_desired(&fields, &desired) {
        Ok(etag)
    } else {
        Err(ContactsError::stale_etag("*", Some(&etag)))
    }
}

async fn recover_update(
    dav: &DavBackend,
    href: &str,
    patch: &ContactPatch,
) -> ContactsResult<String> {
    let (etag, raw) = dav_get(dav, href).await?;
    let card = parse_vcard(&raw)?;
    let fields = contact_fields_from_vcard(&card);
    if patch_already_applied(&fields, patch) {
        Ok(etag)
    } else {
        Err(ContactsError::stale_etag("", Some(&etag)))
    }
}

fn fields_match_desired(actual: &ContactFields, desired: &ContactFields) -> bool {
    actual.display_name == desired.display_name
        && actual.name == desired.name
        && actual.emails == desired.emails
        && actual.phones == desired.phones
        && actual.organization == desired.organization
        && actual.job_title == desired.job_title
        && actual.addresses == desired.addresses
        && actual.birthday == desired.birthday
        && actual.notes == desired.notes
}

fn patch_already_applied(fields: &ContactFields, patch: &ContactPatch) -> bool {
    patch
        .display_name
        .as_ref()
        .is_none_or(|value| value == &fields.display_name)
        && patch
            .name
            .as_ref()
            .is_none_or(|value| value == &fields.name)
        && patch
            .emails
            .as_ref()
            .is_none_or(|value| value == &fields.emails)
        && patch
            .phones
            .as_ref()
            .is_none_or(|value| value == &fields.phones)
        && patch
            .organization
            .as_ref()
            .is_none_or(|value| value == &fields.organization)
        && patch
            .job_title
            .as_ref()
            .is_none_or(|value| value == &fields.job_title)
        && patch
            .addresses
            .as_ref()
            .is_none_or(|value| value == &fields.addresses)
        && patch.birthday.as_ref().is_none_or(|value| {
            Some(value.as_str()) == fields.birthday.as_deref()
                || (value.is_empty() && fields.birthday.is_none())
        })
        && patch
            .notes
            .as_ref()
            .is_none_or(|value| value == &fields.notes)
}

pub async fn project_user(service: &ContactsService, user_id: &str) -> ContactsResult<usize> {
    let items = dav_propfind(&service.dav, &service.principal_path, 1).await?;
    let mut projected = 0;
    let mut seen_books = HashSet::new();
    for item in items {
        if item.href == normalize_href(&service.principal_path) {
            continue;
        }
        if !(item.is_collection && item.is_addressbook) {
            continue;
        }
        let book = upsert_projected_book(&service.pool, user_id, &item).await?;
        seen_books.insert(book.id.clone());
        projected += project_address_book(service, user_id, &book).await?;
    }
    tombstone_missing_books(&service.pool, user_id, &seen_books).await?;
    Ok(projected)
}

pub async fn rebuild_user_projections(
    service: &ContactsService,
    user_id: &str,
) -> ContactsResult<usize> {
    sqlx::query("DELETE FROM contacts_projection_checkpoints WHERE user_id = $1")
        .bind(user_id)
        .execute(&service.pool)
        .await
        .map_err(database_error)?;
    sqlx::query("UPDATE contacts SET deleted_at = NOW(), revision = revision + 1, updated_at = NOW() WHERE user_id = $1 AND deleted_at IS NULL")
        .bind(user_id)
        .execute(&service.pool)
        .await
        .map_err(database_error)?;
    sqlx::query("UPDATE contacts_address_books SET deleted_at = NOW(), revision = revision + 1, updated_at = NOW() WHERE user_id = $1 AND deleted_at IS NULL")
        .bind(user_id)
        .execute(&service.pool)
        .await
        .map_err(database_error)?;
    project_user(service, user_id).await
}

async fn project_address_book(
    service: &ContactsService,
    user_id: &str,
    book: &AddressBook,
) -> ContactsResult<usize> {
    let items = dav_propfind(&service.dav, &book.href, 1).await?;
    let mut seen = HashSet::new();
    let mut count = 0;
    for item in items {
        if item.href == book.href || item.is_collection {
            if item.sync_token.is_some() || item.ctag.is_some() {
                sqlx::query(
                    "UPDATE contacts_address_books
                     SET sync_token = COALESCE($2, sync_token), ctag = COALESCE($3, ctag),
                         etag = COALESCE($4, etag), updated_at = NOW()
                     WHERE id = $1",
                )
                .bind(&book.id)
                .bind(&item.sync_token)
                .bind(&item.ctag)
                .bind(&item.etag)
                .execute(&service.pool)
                .await
                .map_err(database_error)?;
            }
            continue;
        }
        let Some(etag) = item.etag.clone() else {
            continue;
        };
        let (_, raw) = dav_get(&service.dav, &item.href).await?;
        let card = match parse_vcard(&raw) {
            Ok(card) => card,
            Err(error) => {
                tracing::warn!(href = %item.href, error = %error, "skipping invalid vCard");
                continue;
            }
        };
        let fields = contact_fields_from_vcard(&card);
        if fields.uid.is_empty() {
            tracing::warn!(href = %item.href, "skipping vCard without UID");
            continue;
        }
        let id = id_from_uid(&fields.uid);
        upsert_projected_contact(
            &service.pool,
            user_id,
            &book.id,
            &id,
            &item.href,
            &etag,
            &fields,
        )
        .await?;
        seen.insert(id);
        count += 1;
    }
    tombstone_missing_contacts(&service.pool, user_id, &book.id, &seen).await?;
    sqlx::query(
        "INSERT INTO contacts_projection_checkpoints (user_id, address_book_id, href, sync_token, projected_at)
         VALUES ($1, $2, $3, $4, NOW())
         ON CONFLICT (user_id, address_book_id)
         DO UPDATE SET href = EXCLUDED.href, sync_token = EXCLUDED.sync_token, projected_at = EXCLUDED.projected_at",
    )
    .bind(user_id)
    .bind(&book.id)
    .bind(&book.href)
    .bind(&book.sync_token)
    .execute(&service.pool)
    .await
    .map_err(database_error)?;
    Ok(count)
}

async fn upsert_projected_book(
    pool: &PgPool,
    user_id: &str,
    item: &DavItem,
) -> ContactsResult<AddressBook> {
    let uid = item
        .href
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or(&item.href)
        .to_string();
    let id = if Uuid::parse_str(&uid).is_ok() {
        uid.clone()
    } else {
        stable_uuid(&format!("foyer.book.{user_id}"), &item.href)
    };
    let display_name = item
        .display_name
        .clone()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| uid.clone());
    let now = Utc::now();
    sqlx::query(
        "INSERT INTO contacts_address_books
            (id, user_id, uid, href, etag, display_name, description, sync_token, ctag,
             revision, created_at, updated_at, deleted_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, 1, $10, $10, NULL)
         ON CONFLICT (id) DO UPDATE SET
            href = EXCLUDED.href,
            etag = EXCLUDED.etag,
            display_name = EXCLUDED.display_name,
            description = EXCLUDED.description,
            sync_token = EXCLUDED.sync_token,
            ctag = EXCLUDED.ctag,
            deleted_at = NULL,
            revision = CASE WHEN
                contacts_address_books.href IS DISTINCT FROM EXCLUDED.href OR
                contacts_address_books.etag IS DISTINCT FROM EXCLUDED.etag OR
                contacts_address_books.display_name IS DISTINCT FROM EXCLUDED.display_name OR
                contacts_address_books.description IS DISTINCT FROM EXCLUDED.description OR
                contacts_address_books.sync_token IS DISTINCT FROM EXCLUDED.sync_token OR
                contacts_address_books.ctag IS DISTINCT FROM EXCLUDED.ctag OR
                contacts_address_books.deleted_at IS NOT NULL
              THEN contacts_address_books.revision + 1
              ELSE contacts_address_books.revision END,
            updated_at = CASE WHEN
                contacts_address_books.href IS DISTINCT FROM EXCLUDED.href OR
                contacts_address_books.etag IS DISTINCT FROM EXCLUDED.etag OR
                contacts_address_books.display_name IS DISTINCT FROM EXCLUDED.display_name OR
                contacts_address_books.description IS DISTINCT FROM EXCLUDED.description OR
                contacts_address_books.sync_token IS DISTINCT FROM EXCLUDED.sync_token OR
                contacts_address_books.ctag IS DISTINCT FROM EXCLUDED.ctag OR
                contacts_address_books.deleted_at IS NOT NULL
              THEN EXCLUDED.updated_at
              ELSE contacts_address_books.updated_at END
         WHERE contacts_address_books.user_id = EXCLUDED.user_id",
    )
    .bind(&id)
    .bind(user_id)
    .bind(&uid)
    .bind(&item.href)
    .bind(&item.etag)
    .bind(&display_name)
    .bind(item.description.clone().unwrap_or_default())
    .bind(&item.sync_token)
    .bind(&item.ctag)
    .bind(now)
    .execute(pool)
    .await
    .map_err(database_error)?;
    load_book(pool, &id)
        .await?
        .ok_or_else(|| ContactsError::unavailable("projected address book disappeared"))
}

async fn upsert_projected_contact(
    pool: &PgPool,
    user_id: &str,
    address_book_id: &str,
    id: &str,
    href: &str,
    etag: &str,
    fields: &ContactFields,
) -> ContactsResult<()> {
    let now = Utc::now();
    sqlx::query(
        "INSERT INTO contacts
            (id, user_id, address_book_id, uid, href, etag, display_name, given_name, family_name,
             additional_names, honorific_prefix, honorific_suffix, organization, job_title,
             birthday, notes, emails, phones, addresses, revision, created_at, updated_at, deleted_at)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,1,$20,$20,NULL)
         ON CONFLICT (id) DO UPDATE SET
            address_book_id = EXCLUDED.address_book_id,
            uid = EXCLUDED.uid,
            href = EXCLUDED.href,
            etag = EXCLUDED.etag,
            display_name = EXCLUDED.display_name,
            given_name = EXCLUDED.given_name,
            family_name = EXCLUDED.family_name,
            additional_names = EXCLUDED.additional_names,
            honorific_prefix = EXCLUDED.honorific_prefix,
            honorific_suffix = EXCLUDED.honorific_suffix,
            organization = EXCLUDED.organization,
            job_title = EXCLUDED.job_title,
            birthday = EXCLUDED.birthday,
            notes = EXCLUDED.notes,
            emails = EXCLUDED.emails,
            phones = EXCLUDED.phones,
            addresses = EXCLUDED.addresses,
            deleted_at = NULL,
            revision = CASE WHEN
                contacts.address_book_id IS DISTINCT FROM EXCLUDED.address_book_id OR
                contacts.href IS DISTINCT FROM EXCLUDED.href OR
                contacts.etag IS DISTINCT FROM EXCLUDED.etag OR
                contacts.deleted_at IS NOT NULL
              THEN contacts.revision + 1
              ELSE contacts.revision END,
            updated_at = CASE WHEN
                contacts.address_book_id IS DISTINCT FROM EXCLUDED.address_book_id OR
                contacts.href IS DISTINCT FROM EXCLUDED.href OR
                contacts.etag IS DISTINCT FROM EXCLUDED.etag OR
                contacts.deleted_at IS NOT NULL
              THEN EXCLUDED.updated_at
              ELSE contacts.updated_at END
         WHERE contacts.user_id = EXCLUDED.user_id",
    )
    .bind(id)
    .bind(user_id)
    .bind(address_book_id)
    .bind(&fields.uid)
    .bind(href)
    .bind(etag)
    .bind(if fields.display_name.is_empty() {
        "Unnamed contact"
    } else {
        fields.display_name.as_str()
    })
    .bind(&fields.name.given_name)
    .bind(&fields.name.family_name)
    .bind(&fields.name.additional_names)
    .bind(&fields.name.honorific_prefix)
    .bind(&fields.name.honorific_suffix)
    .bind(&fields.organization)
    .bind(&fields.job_title)
    .bind(&fields.birthday)
    .bind(&fields.notes)
    .bind(SqlJson(fields.emails.clone()))
    .bind(SqlJson(fields.phones.clone()))
    .bind(SqlJson(fields.addresses.clone()))
    .bind(now)
    .execute(pool)
    .await
    .map_err(database_error)?;
    Ok(())
}

struct NewContactRow {
    id: String,
    user_id: String,
    address_book_id: String,
    uid: String,
    href: String,
    etag: String,
    fields: ContactFields,
    now: DateTime<Utc>,
}

async fn upsert_contact_row(
    tx: &mut Transaction<'_, Postgres>,
    row: NewContactRow,
) -> ContactsResult<()> {
    sqlx::query(
        "INSERT INTO contacts
            (id, user_id, address_book_id, uid, href, etag, display_name, given_name, family_name,
             additional_names, honorific_prefix, honorific_suffix, organization, job_title,
             birthday, notes, emails, phones, addresses, revision, created_at, updated_at, deleted_at)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,1,$20,$20,NULL)",
    )
    .bind(&row.id)
    .bind(&row.user_id)
    .bind(&row.address_book_id)
    .bind(&row.uid)
    .bind(&row.href)
    .bind(&row.etag)
    .bind(if row.fields.display_name.is_empty() {
        "Unnamed contact"
    } else {
        row.fields.display_name.as_str()
    })
    .bind(&row.fields.name.given_name)
    .bind(&row.fields.name.family_name)
    .bind(&row.fields.name.additional_names)
    .bind(&row.fields.name.honorific_prefix)
    .bind(&row.fields.name.honorific_suffix)
    .bind(&row.fields.organization)
    .bind(&row.fields.job_title)
    .bind(&row.fields.birthday)
    .bind(&row.fields.notes)
    .bind(SqlJson(row.fields.emails.clone()))
    .bind(SqlJson(row.fields.phones.clone()))
    .bind(SqlJson(row.fields.addresses.clone()))
    .bind(row.now)
    .execute(&mut **tx)
    .await
    .map_err(database_error)?;
    Ok(())
}

async fn update_contact_row(
    tx: &mut Transaction<'_, Postgres>,
    id: &str,
    address_book_id: &str,
    href: &str,
    etag: &str,
    fields: &ContactFields,
) -> ContactsResult<()> {
    sqlx::query(
        "UPDATE contacts SET
            address_book_id = $2,
            href = $3,
            etag = $4,
            display_name = $5,
            given_name = $6,
            family_name = $7,
            additional_names = $8,
            honorific_prefix = $9,
            honorific_suffix = $10,
            organization = $11,
            job_title = $12,
            birthday = $13,
            notes = $14,
            emails = $15,
            phones = $16,
            addresses = $17,
            revision = revision + 1,
            updated_at = $18
         WHERE id = $1",
    )
    .bind(id)
    .bind(address_book_id)
    .bind(href)
    .bind(etag)
    .bind(if fields.display_name.is_empty() {
        "Unnamed contact"
    } else {
        fields.display_name.as_str()
    })
    .bind(&fields.name.given_name)
    .bind(&fields.name.family_name)
    .bind(&fields.name.additional_names)
    .bind(&fields.name.honorific_prefix)
    .bind(&fields.name.honorific_suffix)
    .bind(&fields.organization)
    .bind(&fields.job_title)
    .bind(&fields.birthday)
    .bind(&fields.notes)
    .bind(SqlJson(fields.emails.clone()))
    .bind(SqlJson(fields.phones.clone()))
    .bind(SqlJson(fields.addresses.clone()))
    .bind(Utc::now())
    .execute(&mut **tx)
    .await
    .map_err(database_error)?;
    Ok(())
}

async fn tombstone_missing_books(
    pool: &PgPool,
    user_id: &str,
    seen: &HashSet<String>,
) -> ContactsResult<()> {
    let books = sqlx::query_as::<_, AddressBook>(
        "SELECT id, user_id, uid, href, etag, display_name, description, sync_token, ctag,
                revision, created_at, updated_at, deleted_at
         FROM contacts_address_books WHERE user_id = $1 AND deleted_at IS NULL",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await
    .map_err(database_error)?;
    for book in books {
        if !seen.contains(&book.id) {
            sqlx::query(
                "UPDATE contacts_address_books
                 SET deleted_at = NOW(), revision = revision + 1, updated_at = NOW()
                 WHERE id = $1",
            )
            .bind(&book.id)
            .execute(pool)
            .await
            .map_err(database_error)?;
            sqlx::query(
                "UPDATE contacts
                 SET deleted_at = NOW(), revision = revision + 1, updated_at = NOW()
                 WHERE address_book_id = $1 AND deleted_at IS NULL",
            )
            .bind(&book.id)
            .execute(pool)
            .await
            .map_err(database_error)?;
        }
    }
    Ok(())
}

async fn tombstone_missing_contacts(
    pool: &PgPool,
    user_id: &str,
    address_book_id: &str,
    seen: &HashSet<String>,
) -> ContactsResult<()> {
    let ids: Vec<String> = sqlx::query_scalar(
        "SELECT id FROM contacts
         WHERE user_id = $1 AND address_book_id = $2 AND deleted_at IS NULL",
    )
    .bind(user_id)
    .bind(address_book_id)
    .fetch_all(pool)
    .await
    .map_err(database_error)?;
    for id in ids {
        if !seen.contains(&id) {
            sqlx::query(
                "UPDATE contacts
                 SET deleted_at = NOW(), revision = revision + 1, updated_at = NOW()
                 WHERE id = $1",
            )
            .bind(&id)
            .execute(pool)
            .await
            .map_err(database_error)?;
        }
    }
    Ok(())
}

async fn with_operation<T, F>(
    pool: &PgPool,
    binding: OperationBinding,
    work: F,
) -> ContactsResult<T>
where
    T: Serialize + for<'de> Deserialize<'de> + Send,
    F: for<'c> FnOnce(
        &'c mut Transaction<'_, Postgres>,
    ) -> Pin<Box<dyn Future<Output = ContactsResult<T>> + Send + 'c>>,
{
    let OperationBinding {
        user_id,
        operation_id,
        entity_type,
        entity_id,
        operation,
        request_body,
    } = binding;
    let mut tx = pool.begin().await.map_err(database_error)?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(&operation_id)
        .execute(&mut *tx)
        .await
        .map_err(database_error)?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 1))")
        .bind(&user_id)
        .execute(&mut *tx)
        .await
        .map_err(database_error)?;
    if let Some(stored) = load_operation(&mut tx, &operation_id).await? {
        if stored.user_id != user_id
            || stored.entity_type != entity_type
            || stored.entity_id != entity_id
            || stored.operation != operation
            || stored.request_body != request_body
        {
            return Err(ContactsError::conflict(
                "This operation id is already bound to a different request.",
            ));
        }
        tx.commit().await.map_err(database_error)?;
        if stored.result_status != 200 {
            return Err(ContactsError::conflict(
                "This operation id already produced a non-success result.",
            ));
        }
        return serde_json::from_value(stored.result_body).map_err(|error| {
            ContactsError::unavailable(format!("stored operation payload is invalid: {error}"))
        });
    }
    let result = work(&mut tx).await?;
    let body = serde_json::to_value(&result).map_err(|error| {
        ContactsError::unavailable(format!("failed to store operation: {error}"))
    })?;
    sqlx::query(
        "INSERT INTO contacts_operations
            (operation_id, user_id, entity_type, entity_id, operation, request_body,
             result_status, result_body, created_at)
         VALUES ($1, $2, $3, $4, $5, $6, 200, $7, $8)",
    )
    .bind(operation_id)
    .bind(user_id)
    .bind(entity_type)
    .bind(entity_id)
    .bind(operation)
    .bind(request_body)
    .bind(body)
    .bind(Utc::now())
    .execute(&mut *tx)
    .await
    .map_err(database_error)?;
    tx.commit().await.map_err(database_error)?;
    Ok(result)
}

struct OperationBinding {
    user_id: String,
    operation_id: String,
    entity_type: &'static str,
    entity_id: String,
    operation: &'static str,
    request_body: Value,
}

fn operation_binding(
    user_id: String,
    operation_id: String,
    entity_type: &'static str,
    entity_id: String,
    operation: &'static str,
    request_body: Value,
) -> OperationBinding {
    OperationBinding {
        user_id,
        operation_id,
        entity_type,
        entity_id,
        operation,
        request_body,
    }
}

fn operation_request<T: Serialize>(request: &T) -> ContactsResult<Value> {
    serde_json::to_value(request)
        .map_err(|error| ContactsError::unavailable(format!("failed to encode operation: {error}")))
}

#[derive(Debug, FromRow)]
struct StoredOperation {
    user_id: String,
    entity_type: String,
    entity_id: String,
    operation: String,
    request_body: Value,
    result_status: i32,
    result_body: Value,
}

async fn load_operation(
    tx: &mut Transaction<'_, Postgres>,
    operation_id: &str,
) -> ContactsResult<Option<StoredOperation>> {
    sqlx::query_as::<_, StoredOperation>(
        "SELECT user_id, entity_type, entity_id, operation, request_body, result_status, result_body
         FROM contacts_operations WHERE operation_id = $1",
    )
    .bind(operation_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(database_error)
}

async fn load_book(pool: &PgPool, id: &str) -> ContactsResult<Option<AddressBook>> {
    sqlx::query_as::<_, AddressBook>(
        "SELECT id, user_id, uid, href, etag, display_name, description, sync_token, ctag,
                revision, created_at, updated_at, deleted_at
         FROM contacts_address_books WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(database_error)
}

async fn load_book_tx(
    tx: &mut Transaction<'_, Postgres>,
    id: &str,
) -> ContactsResult<Option<AddressBook>> {
    sqlx::query_as::<_, AddressBook>(
        "SELECT id, user_id, uid, href, etag, display_name, description, sync_token, ctag,
                revision, created_at, updated_at, deleted_at
         FROM contacts_address_books WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(database_error)
}

async fn load_required_book(
    tx: &mut Transaction<'_, Postgres>,
    id: &str,
) -> ContactsResult<AddressBook> {
    load_book_tx(tx, id)
        .await?
        .ok_or_else(|| ContactsError::unavailable("address book disappeared after write"))
}

async fn load_contact(pool: &PgPool, id: &str) -> ContactsResult<Option<Contact>> {
    Ok(sqlx::query_as::<_, ContactRow>(
        "SELECT id, user_id, address_book_id, uid, href, etag, display_name, given_name,
                family_name, additional_names, honorific_prefix, honorific_suffix,
                organization, job_title, birthday, notes, emails, phones, addresses,
                revision, created_at, updated_at, deleted_at
         FROM contacts WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(database_error)?
    .map(Contact::from))
}

async fn load_contact_tx(
    tx: &mut Transaction<'_, Postgres>,
    id: &str,
) -> ContactsResult<Option<Contact>> {
    Ok(sqlx::query_as::<_, ContactRow>(
        "SELECT id, user_id, address_book_id, uid, href, etag, display_name, given_name,
                family_name, additional_names, honorific_prefix, honorific_suffix,
                organization, job_title, birthday, notes, emails, phones, addresses,
                revision, created_at, updated_at, deleted_at
         FROM contacts WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(database_error)?
    .map(Contact::from))
}

async fn load_required_contact(
    tx: &mut Transaction<'_, Postgres>,
    id: &str,
) -> ContactsResult<Contact> {
    load_contact_tx(tx, id)
        .await?
        .ok_or_else(|| ContactsError::unavailable("contact disappeared after write"))
}

async fn locked_live_book(
    tx: &mut Transaction<'_, Postgres>,
    user_id: &str,
    id: &str,
    expected_etag: Option<&str>,
    expected_revision: Option<i64>,
) -> ContactsResult<AddressBook> {
    let book = sqlx::query_as::<_, AddressBook>(
        "SELECT id, user_id, uid, href, etag, display_name, description, sync_token, ctag,
                revision, created_at, updated_at, deleted_at
         FROM contacts_address_books WHERE id = $1 FOR UPDATE",
    )
    .bind(id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(database_error)?;
    let book = visible_book(user_id, book)?;
    check_precondition(
        expected_etag,
        expected_revision,
        book.etag.as_deref(),
        book.revision,
    )?;
    Ok(book)
}

async fn locked_live_contact(
    tx: &mut Transaction<'_, Postgres>,
    user_id: &str,
    id: &str,
    expected_etag: Option<&str>,
    expected_revision: Option<i64>,
) -> ContactsResult<Contact> {
    let contact = sqlx::query_as::<_, ContactRow>(
        "SELECT id, user_id, address_book_id, uid, href, etag, display_name, given_name,
                family_name, additional_names, honorific_prefix, honorific_suffix,
                organization, job_title, birthday, notes, emails, phones, addresses,
                revision, created_at, updated_at, deleted_at
         FROM contacts WHERE id = $1 FOR UPDATE",
    )
    .bind(id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(database_error)?;
    let contact = visible_contact(user_id, contact.map(Contact::from))?;
    check_precondition(
        expected_etag,
        expected_revision,
        Some(&contact.etag),
        contact.revision,
    )?;
    Ok(contact)
}

fn check_precondition(
    expected_etag: Option<&str>,
    expected_revision: Option<i64>,
    actual_etag: Option<&str>,
    actual_revision: i64,
) -> ContactsResult<()> {
    if let Some(expected) = expected_etag {
        match actual_etag {
            Some(actual) if etags_match(expected, actual) => {}
            actual => return Err(ContactsError::stale_etag(expected, actual)),
        }
    }
    if let Some(expected) = expected_revision {
        if expected != actual_revision {
            return Err(ContactsError::stale_revision(expected, actual_revision));
        }
    }
    Ok(())
}

fn visible_book(user_id: &str, book: Option<AddressBook>) -> ContactsResult<AddressBook> {
    match book {
        Some(book) if book.user_id != user_id => {
            Err(ContactsError::not_found("Address book not found."))
        }
        Some(book) if book.deleted_at.is_some() => {
            Err(ContactsError::gone("This address book has been deleted."))
        }
        Some(book) => Ok(book),
        None => Err(ContactsError::not_found("Address book not found.")),
    }
}

fn visible_contact(user_id: &str, contact: Option<Contact>) -> ContactsResult<Contact> {
    match contact {
        Some(contact) if contact.user_id != user_id => {
            Err(ContactsError::not_found("Contact not found."))
        }
        Some(contact) if contact.deleted_at.is_some() => {
            Err(ContactsError::gone("This contact has been deleted."))
        }
        Some(contact) => Ok(contact),
        None => Err(ContactsError::not_found("Contact not found.")),
    }
}

fn existing_identity_error(user_id: &str, owner_id: String, deleted: bool) -> ContactsError {
    if owner_id != user_id {
        return ContactsError::conflict("This identifier is already in use.");
    }
    if deleted {
        ContactsError::gone("A tombstoned item cannot be resurrected.")
    } else {
        ContactsError::conflict("This identifier is already in use.")
    }
}

async fn ensure_live_book(
    tx: &mut Transaction<'_, Postgres>,
    user_id: &str,
    address_book_id: &str,
) -> ContactsResult<AddressBook> {
    match load_book_tx(tx, address_book_id).await? {
        None => Err(ContactsError::invalid_parent(
            "The address book does not exist.",
        )),
        Some(book) if book.user_id != user_id => Err(ContactsError::invalid_parent(
            "The address book does not exist.",
        )),
        Some(book) if book.deleted_at.is_some() => Err(ContactsError::invalid_parent(
            "The address book has been deleted.",
        )),
        Some(book) => Ok(book),
    }
}

async fn ensure_book_quota(
    tx: &mut Transaction<'_, Postgres>,
    user_id: &str,
) -> ContactsResult<()> {
    let count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM contacts_address_books WHERE user_id = $1 AND deleted_at IS NULL",
    )
    .bind(user_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(database_error)?;
    if count >= MAX_ADDRESS_BOOKS {
        return Err(ContactsError::limit_exceeded(format!(
            "A user may have at most {MAX_ADDRESS_BOOKS} address books."
        )));
    }
    Ok(())
}

async fn ensure_contact_quota(
    tx: &mut Transaction<'_, Postgres>,
    user_id: &str,
) -> ContactsResult<()> {
    let count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM contacts WHERE user_id = $1 AND deleted_at IS NULL",
    )
    .bind(user_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(database_error)?;
    if count >= MAX_CONTACTS {
        return Err(ContactsError::limit_exceeded(format!(
            "A user may have at most {MAX_CONTACTS} contacts."
        )));
    }
    Ok(())
}

fn file_stem(href: &str) -> String {
    href.rsplit('/')
        .next()
        .unwrap_or(href)
        .trim_end_matches(".vcf")
        .to_string()
}

fn database_error(error: sqlx::Error) -> ContactsError {
    ContactsError::unavailable(format!("database error: {error}"))
}

/// PowerSync column contract shared by Android and Foyer Shell.
pub fn powersync_projection_columns() -> &'static [&'static str] {
    &[
        "id",
        "user_id",
        "address_book_id",
        "uid",
        "href",
        "etag",
        "display_name",
        "given_name",
        "family_name",
        "additional_names",
        "honorific_prefix",
        "honorific_suffix",
        "organization",
        "job_title",
        "birthday",
        "notes",
        "emails",
        "phones",
        "addresses",
        "revision",
        "created_at",
        "updated_at",
    ]
}

pub fn search_contacts<'a>(contacts: &'a [Contact], query: &str) -> Vec<&'a Contact> {
    let query = query.trim().to_ascii_lowercase();
    if query.is_empty() {
        return contacts.iter().collect();
    }
    contacts
        .iter()
        .filter(|contact| contact_matches(contact, &query))
        .collect()
}

pub fn contact_matches(contact: &Contact, query: &str) -> bool {
    let query = query.trim().to_ascii_lowercase();
    if query.is_empty() {
        return true;
    }
    let mut haystacks = vec![
        contact.display_name.as_str(),
        contact.name.given_name.as_str(),
        contact.name.family_name.as_str(),
        contact.name.additional_names.as_str(),
        contact.organization.as_str(),
        contact.job_title.as_str(),
        contact.notes.as_str(),
    ];
    for email in &contact.emails {
        haystacks.push(email.value.as_str());
    }
    for phone in &contact.phones {
        haystacks.push(phone.value.as_str());
    }
    haystacks
        .into_iter()
        .any(|value| value.to_ascii_lowercase().contains(&query))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/v1/address-books",
            get(handle_list_address_books).post(handle_create_address_book),
        )
        .route(
            "/v1/address-books/{addressBookId}",
            get(handle_get_address_book),
        )
        .route(
            "/v1/address-books/{addressBookId}/update",
            post(handle_update_address_book),
        )
        .route(
            "/v1/address-books/{addressBookId}/delete",
            post(handle_delete_address_book),
        )
        .route(
            "/v1/contacts",
            get(handle_list_contacts).post(handle_create_contact),
        )
        .route("/v1/contacts/{contactId}", get(handle_get_contact))
        .route(
            "/v1/contacts/{contactId}/update",
            post(handle_update_contact),
        )
        .route("/v1/contacts/{contactId}/move", post(handle_move_contact))
        .route(
            "/v1/contacts/{contactId}/delete",
            post(handle_delete_contact),
        )
}

pub async fn reconcile_user(state: &AppState, user_id: &str) -> Result<(), String> {
    let service = ContactsService::from_state(state, user_id).map_err(|error| error.to_string())?;
    project_user(&service, user_id)
        .await
        .map_err(|error| error.to_string())?;
    if let Ok(client) = state.dav_client() {
        let books = list_address_books(&state.pool, user_id)
            .await
            .map_err(|error| error.to_string())?;
        for book in books.address_books {
            let href = DavHref::parse(&book.href).map_err(|error| error.to_string())?;
            let collection = crate::dav::DavCollection {
                href: href.clone(),
                kind: CollectionKind::AddressBook,
                display_name: Some(book.display_name.clone()),
                etag: book
                    .etag
                    .as_deref()
                    .and_then(|value| ETag::parse(value).ok()),
                sync_token: book
                    .sync_token
                    .as_deref()
                    .and_then(|value| crate::dav::SyncToken::parse(value).ok()),
                supported_components: Vec::new(),
            };
            let _ = state
                .projector
                .remember_collection(user_id, &book.id, &collection)
                .await;
            if let Ok(plan) = state.projector.plan_sync(client, user_id, &href).await {
                let mut tx = state
                    .pool
                    .begin()
                    .await
                    .map_err(|error| error.to_string())?;
                state
                    .projector
                    .commit_checkpoint(
                        &mut tx,
                        user_id,
                        &href,
                        plan.page.sync_token.as_ref(),
                        collection.etag.as_ref(),
                    )
                    .await
                    .map_err(|error| error.to_string())?;
                tx.commit().await.map_err(|error| error.to_string())?;
            }
        }
    }
    Ok(())
}

pub async fn handle_list_address_books(
    State(state): State<AppState>,
    principal: Principal,
) -> ContactsResult<Json<AddressBookList>> {
    list_address_books(&state.pool, &principal.user_id)
        .await
        .map(Json)
}

pub async fn handle_get_address_book(
    State(state): State<AppState>,
    principal: Principal,
    Path(address_book_id): Path<String>,
) -> ContactsResult<Json<AddressBook>> {
    get_address_book(&state.pool, &principal.user_id, &address_book_id)
        .await
        .map(Json)
}

pub async fn handle_create_address_book(
    State(state): State<AppState>,
    principal: Principal,
    Json(request): Json<CreateAddressBookRequest>,
) -> ContactsResult<Json<AddressBook>> {
    let service = ContactsService::from_state(&state, &principal.user_id)?;
    create_address_book(&service, &principal.user_id, request)
        .await
        .map(Json)
}

pub async fn handle_update_address_book(
    State(state): State<AppState>,
    principal: Principal,
    Path(address_book_id): Path<String>,
    Json(request): Json<UpdateAddressBookRequest>,
) -> ContactsResult<Json<AddressBook>> {
    let service = ContactsService::from_state(&state, &principal.user_id)?;
    update_address_book(&service, &principal.user_id, &address_book_id, request)
        .await
        .map(Json)
}

pub async fn handle_delete_address_book(
    State(state): State<AppState>,
    principal: Principal,
    Path(address_book_id): Path<String>,
    Json(request): Json<DeleteRequest>,
) -> ContactsResult<Json<AddressBook>> {
    let service = ContactsService::from_state(&state, &principal.user_id)?;
    delete_address_book(&service, &principal.user_id, &address_book_id, request)
        .await
        .map(Json)
}

pub async fn handle_list_contacts(
    State(state): State<AppState>,
    principal: Principal,
    Query(query): Query<ContactListQuery>,
) -> ContactsResult<Json<ContactList>> {
    list_contacts(
        &state.pool,
        &principal.user_id,
        query.address_book_id.as_deref(),
    )
    .await
    .map(Json)
}

pub async fn handle_get_contact(
    State(state): State<AppState>,
    principal: Principal,
    Path(contact_id): Path<String>,
) -> ContactsResult<Json<Contact>> {
    get_contact(&state.pool, &principal.user_id, &contact_id)
        .await
        .map(Json)
}

pub async fn handle_create_contact(
    State(state): State<AppState>,
    principal: Principal,
    Json(request): Json<CreateContactRequest>,
) -> ContactsResult<Json<Contact>> {
    let service = ContactsService::from_state(&state, &principal.user_id)?;
    create_contact(&service, &principal.user_id, request)
        .await
        .map(Json)
}

pub async fn handle_update_contact(
    State(state): State<AppState>,
    principal: Principal,
    Path(contact_id): Path<String>,
    Json(request): Json<UpdateContactRequest>,
) -> ContactsResult<Json<Contact>> {
    let service = ContactsService::from_state(&state, &principal.user_id)?;
    update_contact(&service, &principal.user_id, &contact_id, request)
        .await
        .map(Json)
}

pub async fn handle_move_contact(
    State(state): State<AppState>,
    principal: Principal,
    Path(contact_id): Path<String>,
    Json(request): Json<MoveContactRequest>,
) -> ContactsResult<Json<Contact>> {
    let service = ContactsService::from_state(&state, &principal.user_id)?;
    move_contact(&service, &principal.user_id, &contact_id, request)
        .await
        .map(Json)
}

pub async fn handle_delete_contact(
    State(state): State<AppState>,
    principal: Principal,
    Path(contact_id): Path<String>,
    Json(request): Json<DeleteRequest>,
) -> ContactsResult<Json<Contact>> {
    let service = ContactsService::from_state(&state, &principal.user_id)?;
    delete_contact(&service, &principal.user_id, &contact_id, request)
        .await
        .map(Json)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unfolding_joins_folded_lines() {
        let folded = "NOTE:line one\r\n  still one\r\n\tcontinues";
        assert_eq!(unfold(folded), "NOTE:line one still onecontinues");
    }

    #[test]
    fn folding_splits_on_octet_budget() {
        let line = format!("NOTE:{}", "a".repeat(90));
        let folded = fold_line(&line);
        assert!(folded.contains("\r\n "));
        assert!(unfold(&folded).contains(&"a".repeat(90)));
        for piece in folded.split("\r\n") {
            assert!(piece.len() <= 75, "{piece:?}");
        }
    }

    #[test]
    fn escaping_round_trips_specials() {
        let original = "comma, semi; slash\\ and\nnewline";
        assert_eq!(unescape_text(&escape_text(original)), original);
    }

    #[test]
    fn unknown_properties_survive_partial_edits() {
        let raw = "BEGIN:VCARD\r\nVERSION:4.0\r\nUID:urn:uuid:11111111-1111-4111-8111-111111111111\r\nFN:Ada Lovelace\r\nEMAIL;TYPE=work:ada@example.com\r\nX-ABLABEL:Lab\r\nitem1.X-CUSTOM:keep-me\r\nPHOTO;ENCODING=b:abcd\r\nNOTE:old\r\nEND:VCARD\r\n";
        let mut card = parse_vcard(raw).unwrap();
        apply_contact_patch(
            &mut card,
            &ContactPatch {
                notes: Some("new note".into()),
                emails: Some(vec![TypedEmail {
                    value: "ada@example.com".into(),
                    r#type: "work".into(),
                    pref: false,
                }]),
                ..ContactPatch::default()
            },
        )
        .unwrap();
        let unknown: Vec<_> = unknown_properties(&card)
            .into_iter()
            .map(|property| property.name.as_str())
            .collect();
        assert!(unknown.contains(&"X-ABLABEL"));
        assert!(unknown.contains(&"X-CUSTOM"));
        assert!(unknown.contains(&"PHOTO"));
        assert_eq!(card.first_value("NOTE"), Some("new note"));
        let serialized = serialize_vcard(&card);
        assert!(serialized.contains("X-ABLABEL:Lab"));
        assert!(serialized.contains("item1.X-CUSTOM:keep-me"));
        assert!(serialized.contains("PHOTO"));
    }

    #[test]
    fn structured_name_and_multivalue_fields_round_trip() {
        let raw = "BEGIN:VCARD\r\nVERSION:4.0\r\nUID:abc\r\nFN:Dr. Jane Marie Doe PhD\r\nN:Doe;Jane;Marie;Dr.;PhD\r\nEMAIL;TYPE=work,pref:work@example.com\r\nEMAIL;TYPE=home:home@example.com\r\nTEL;TYPE=cell:+1-555-0100\r\nORG:Amazity\r\nTITLE:Engineer\r\nADR;TYPE=home:;;123 Main St;Springfield;IL;62701;USA\r\nBDAY:19900115\r\nNOTE:Line 1\\nLine 2\r\nEND:VCARD\r\n";
        let card = parse_vcard(raw).unwrap();
        let fields = contact_fields_from_vcard(&card);
        assert_eq!(fields.name.family_name, "Doe");
        assert_eq!(fields.name.given_name, "Jane");
        assert_eq!(fields.emails.len(), 2);
        assert!(fields.emails[0].pref);
        assert_eq!(fields.phones[0].r#type, "cell");
        assert_eq!(fields.addresses[0].locality, "Springfield");
        assert_eq!(fields.birthday.as_deref(), Some("1990-01-15"));
        assert_eq!(fields.notes, "Line 1\nLine 2");
    }

    #[test]
    fn validation_rejects_oversize_and_control_characters() {
        assert!(validate_book_name("").is_err());
        assert!(validate_book_name(&"x".repeat(MAX_BOOK_NAME + 1)).is_err());
        assert!(validate_notes("ok\0no").is_err());
        assert!(parse_birthday("2024-13-01").is_err());
        assert_eq!(
            parse_birthday("1990-01-15").unwrap().as_deref(),
            Some("1990-01-15")
        );
    }

    #[test]
    fn search_matches_email_and_name() {
        let contact = Contact {
            id: "1".into(),
            user_id: "u".into(),
            address_book_id: "b".into(),
            uid: "uid".into(),
            href: "/b/1.vcf".into(),
            etag: "\"e1\"".into(),
            display_name: "Ada Lovelace".into(),
            name: StructuredName {
                given_name: "Ada".into(),
                family_name: "Lovelace".into(),
                ..StructuredName::default()
            },
            emails: vec![TypedEmail {
                value: "ada@example.com".into(),
                r#type: "work".into(),
                pref: true,
            }],
            phones: Vec::new(),
            organization: "Analytical".into(),
            job_title: String::new(),
            addresses: Vec::new(),
            birthday: None,
            notes: String::new(),
            revision: 1,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            deleted_at: None,
        };
        assert!(contact_matches(&contact, "ADA@"));
        assert!(contact_matches(&contact, "analytical"));
        assert!(!contact_matches(&contact, "bob"));
    }
}
