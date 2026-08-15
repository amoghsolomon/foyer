//! Hosted calendar adapter. Reads come from immutable snapshots; I/O never runs in GPUI.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

pub mod api;
mod recurrence;
mod replica;

pub use recurrence::{
    RecurrenceRule, encode_exdates, expand_event, format_stamp, format_when, parse_exdates,
    parse_rrule, recurrence_summary,
};
pub use replica::{
    Command, Controller, Runtime, apply_local, calendar_schema, public_upload_error, read_snapshot,
    replica_path, schema_tables, start, upload_entry,
};

pub const CALENDARS_TABLE: &str = "calendar_calendars";
pub const EVENTS_TABLE: &str = "calendar_events";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Availability {
    Loading,
    Available,
    Unavailable(String),
}

impl Availability {
    pub fn detail(&self) -> &str {
        match self {
            Self::Loading => "Loading calendars…",
            Self::Available => "Available",
            Self::Unavailable(error) => error,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Calendar {
    pub id: String,
    pub uid: String,
    pub href: String,
    pub etag: String,
    #[serde(rename = "displayName")]
    pub display_name: String,
    pub description: String,
    pub color: Option<String>,
    pub revision: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    pub id: String,
    #[serde(rename = "calendarId")]
    pub calendar_id: String,
    pub uid: String,
    pub href: String,
    pub etag: String,
    pub summary: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub location: String,
    #[serde(rename = "allDay", default)]
    pub all_day: bool,
    pub dtstart: String,
    pub dtend: Option<String>,
    pub tzid: Option<String>,
    pub rrule: Option<String>,
    #[serde(default = "default_exdates")]
    pub exdates: String,
    pub revision: i64,
}

fn default_exdates() -> String {
    "[]".into()
}

impl Event {
    pub fn is_recurring(&self) -> bool {
        self.rrule.as_deref().is_some_and(|value| !value.is_empty())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventDraft {
    pub summary: String,
    pub description: String,
    pub location: String,
    pub all_day: bool,
    pub dtstart: String,
    pub dtend: Option<String>,
    pub tzid: Option<String>,
    pub rrule: Option<String>,
    pub exdates: Vec<String>,
    pub calendar_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Occurrence {
    pub event_id: String,
    pub calendar_id: String,
    pub uid: String,
    pub summary: String,
    pub description: String,
    pub location: String,
    pub all_day: bool,
    pub tzid: Option<String>,
    pub start_ms: Option<i64>,
    pub end_ms: Option<i64>,
    pub start_local: String,
    pub end_local: Option<String>,
    pub recurrence_id: String,
    pub is_recurring: bool,
}

#[derive(Clone, Debug)]
pub struct Snapshot {
    pub availability: Availability,
    pub development: bool,
    pub using_powersync: bool,
    pub offline: bool,
    pub pending_uploads: usize,
    pub last_error: Option<String>,
    pub calendars: Arc<Vec<Calendar>>,
    pub events: Arc<Vec<Event>>,
    pub selected_calendar_id: Option<String>,
}

impl Default for Snapshot {
    fn default() -> Self {
        Self {
            availability: Availability::Loading,
            development: true,
            using_powersync: false,
            offline: false,
            pending_uploads: 0,
            last_error: None,
            calendars: Arc::new(Vec::new()),
            events: Arc::new(Vec::new()),
            selected_calendar_id: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SyncBanner {
    Offline { pending: usize },
    Pending { pending: usize },
    StaleEtag { message: String },
    Error { message: String },
}

impl Snapshot {
    pub fn calendar(&self, id: &str) -> Option<&Calendar> {
        self.calendars.iter().find(|calendar| calendar.id == id)
    }

    pub fn event(&self, id: &str) -> Option<&Event> {
        self.events.iter().find(|event| event.id == id)
    }

    pub fn selected_calendar(&self) -> Option<&Calendar> {
        self.selected_calendar_id
            .as_deref()
            .and_then(|id| self.calendar(id))
            .or_else(|| self.calendars.first())
    }

    pub fn events_in(&self, calendar_id: Option<&str>) -> Vec<Event> {
        self.events
            .iter()
            .filter(|event| calendar_id.is_none_or(|id| event.calendar_id == id))
            .cloned()
            .collect()
    }

    pub fn validate_calendar_delete(&self, calendar_id: &str) -> Result<(), String> {
        if self.calendar(calendar_id).is_none() {
            return Err("The calendar was not found.".into());
        }
        if self.events_in(Some(calendar_id)).is_empty() {
            Ok(())
        } else {
            Err("This calendar still has events. Move or delete them first.".into())
        }
    }

    pub fn validate_event_draft(&self, draft: &EventDraft) -> Result<(), String> {
        if draft.summary.trim().is_empty() {
            return Err("A title is required.".into());
        }
        if self.calendar(&draft.calendar_id).is_none() {
            return Err("Choose a calendar.".into());
        }
        if draft.dtstart.trim().is_empty() {
            return Err("A start date is required.".into());
        }
        Ok(())
    }

    pub fn sync_banner(&self) -> Option<SyncBanner> {
        if let Availability::Unavailable(error) = &self.availability {
            return Some(SyncBanner::Error {
                message: error.clone(),
            });
        }
        if let Some(error) = self.last_error.clone().filter(|value| !value.is_empty()) {
            return Some(if is_stale(&error) {
                SyncBanner::StaleEtag { message: error }
            } else {
                SyncBanner::Error { message: error }
            });
        }
        if self.offline {
            Some(SyncBanner::Offline {
                pending: self.pending_uploads,
            })
        } else if self.pending_uploads > 0 {
            Some(SyncBanner::Pending {
                pending: self.pending_uploads,
            })
        } else {
            None
        }
    }
}

fn is_stale(error: &str) -> bool {
    let lower = error.to_ascii_lowercase();
    lower.contains("stale_etag")
        || lower.contains("stale etag")
        || lower.contains("stale_revision")
        || lower.contains("stale revision")
}

pub fn shared_schema_tables() -> [&'static str; 2] {
    [CALENDARS_TABLE, EVENTS_TABLE]
}
