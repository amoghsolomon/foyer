//! Bounded Calendar slice: Radicale is the CalDAV authority; PostgreSQL rows are
//! rebuildable projections; clients consume only normalized fields.
#![allow(
    clippy::collapsible_if,
    clippy::if_same_then_else,
    clippy::manual_range_contains,
    clippy::redundant_closure,
    clippy::too_many_arguments
)]
//!
//! Isolated in-memory DAV/projection backends remain under tests. Production
//! handlers in `calendar/api.rs` persist rebuildable PostgreSQL projections
//! and issue conditional writes through the shared `DavClient`.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

mod api;
pub use api::{reconcile_user, routes};

pub const MAX_CALENDARS_PER_USER: usize = 64;
pub const MAX_EVENTS_PER_USER: usize = 4096;
pub const MAX_SUMMARY_CHARS: usize = 500;
pub const MAX_LOCATION_CHARS: usize = 500;
pub const MAX_DESCRIPTION_BYTES: usize = 64 * 1024;
pub const MAX_EXPANSION_INSTANCES: usize = 512;
pub const MAX_WINDOW_DAYS: i32 = 366 * 2;
pub const ICAL_FOLD_OCTETS: usize = 75;

pub const MIGRATION_SQL: &str = include_str!("../migrations/0006_calendar_projection.sql");

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CalendarError {
    InvalidRequest(String),
    NotFound(String),
    Gone(String),
    Conflict(String),
    StaleEtag { expected: String, actual: String },
    StaleRevision { expected: i64, actual: i64 },
    LimitExceeded(String),
    Dav(String),
    Parse(String),
}

impl fmt::Display for CalendarError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRequest(m)
            | Self::NotFound(m)
            | Self::Gone(m)
            | Self::Conflict(m)
            | Self::LimitExceeded(m)
            | Self::Dav(m)
            | Self::Parse(m) => f.write_str(m),
            Self::StaleEtag { expected, actual } => {
                write!(f, "stale ETag: expected {expected}, actual {actual}")
            }
            Self::StaleRevision { expected, actual } => {
                write!(
                    f,
                    "The expected revision does not match the current revision ({expected} != {actual})."
                )
            }
        }
    }
}

impl std::error::Error for CalendarError {}

pub type CalendarResult<T> = Result<T, CalendarError>;

// ---------------------------------------------------------------------------
// Civil date / time
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Date {
    pub year: i32,
    pub month: u8,
    pub day: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Time {
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DateTime {
    pub date: Date,
    pub time: Time,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Weekday {
    Monday,
    Tuesday,
    Wednesday,
    Thursday,
    Friday,
    Saturday,
    Sunday,
}

impl Weekday {
    pub fn from_index(index: i32) -> Self {
        match index.rem_euclid(7) {
            0 => Self::Monday,
            1 => Self::Tuesday,
            2 => Self::Wednesday,
            3 => Self::Thursday,
            4 => Self::Friday,
            5 => Self::Saturday,
            _ => Self::Sunday,
        }
    }

    pub fn index(self) -> i32 {
        match self {
            Self::Monday => 0,
            Self::Tuesday => 1,
            Self::Wednesday => 2,
            Self::Thursday => 3,
            Self::Friday => 4,
            Self::Saturday => 5,
            Self::Sunday => 6,
        }
    }

    pub fn parse_ical(token: &str) -> CalendarResult<Self> {
        match token {
            "MO" => Ok(Self::Monday),
            "TU" => Ok(Self::Tuesday),
            "WE" => Ok(Self::Wednesday),
            "TH" => Ok(Self::Thursday),
            "FR" => Ok(Self::Friday),
            "SA" => Ok(Self::Saturday),
            "SU" => Ok(Self::Sunday),
            other => Err(CalendarError::Parse(format!("unknown weekday {other}"))),
        }
    }

    pub fn ical(self) -> &'static str {
        match self {
            Self::Monday => "MO",
            Self::Tuesday => "TU",
            Self::Wednesday => "WE",
            Self::Thursday => "TH",
            Self::Friday => "FR",
            Self::Saturday => "SA",
            Self::Sunday => "SU",
        }
    }
}

impl Date {
    pub fn new(year: i32, month: u8, day: u8) -> CalendarResult<Self> {
        if !(1..=12).contains(&month) || day == 0 || day > month_length(year, month) {
            return Err(CalendarError::Parse(format!(
                "invalid date {year:04}-{month:02}-{day:02}"
            )));
        }
        Ok(Self { year, month, day })
    }

    pub fn parse_ical(value: &str) -> CalendarResult<Self> {
        if value.len() != 8 || !value.bytes().all(|b| b.is_ascii_digit()) {
            return Err(CalendarError::Parse(format!("invalid DATE {value}")));
        }
        Self::new(
            value[0..4].parse().unwrap(),
            value[4..6].parse().unwrap(),
            value[6..8].parse().unwrap(),
        )
    }

    pub fn ical(self) -> String {
        format!("{:04}{:02}{:02}", self.year, self.month, self.day)
    }

    pub fn iso(self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }

    pub fn to_rata(self) -> i32 {
        let y = if self.month <= 2 {
            self.year - 1
        } else {
            self.year
        };
        let era = y.div_euclid(400);
        let yoe = y - era * 400;
        let mp = if self.month > 2 {
            self.month as i32 - 3
        } else {
            self.month as i32 + 9
        };
        let doy = (153 * mp + 2) / 5 + self.day as i32 - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146097 + doe - 719468
    }

    pub fn from_rata(days: i32) -> Self {
        let z = days + 719468;
        let era = z.div_euclid(146097);
        let doe = z - era * 146097;
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
        let y = yoe + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = (doy - (153 * mp + 2) / 5 + 1) as u8;
        let month = if mp < 10 { mp + 3 } else { mp - 9 } as u8;
        let year = if month <= 2 { y + 1 } else { y };
        Self { year, month, day }
    }

    pub fn weekday(self) -> Weekday {
        // 1970-01-01 is Thursday.
        Weekday::from_index(self.to_rata() + 3)
    }

    pub fn add_days(self, days: i32) -> Self {
        Self::from_rata(self.to_rata() + days)
    }

    pub fn add_months(self, months: i32) -> Option<Self> {
        let total = (self.year * 12 + self.month as i32 - 1) + months;
        let year = total.div_euclid(12);
        let month = (total.rem_euclid(12) + 1) as u8;
        let last = month_length(year, month);
        if self.day > last {
            None
        } else {
            Some(Self {
                year,
                month,
                day: self.day,
            })
        }
    }

    pub fn add_years(self, years: i32) -> Option<Self> {
        self.add_months(years * 12)
    }

    pub fn nth_weekday_of_month(year: i32, month: u8, weekday: Weekday, nth: i32) -> Option<Self> {
        if nth == 0 {
            return None;
        }
        let last = month_length(year, month);
        if nth > 0 {
            let first = Self {
                year,
                month,
                day: 1,
            };
            let delta = (weekday.index() - first.weekday().index()).rem_euclid(7);
            let day = 1 + delta + (nth - 1) * 7;
            if day < 1 || day > last as i32 {
                None
            } else {
                Some(Self {
                    year,
                    month,
                    day: day as u8,
                })
            }
        } else {
            let last_date = Self {
                year,
                month,
                day: last,
            };
            let delta = (last_date.weekday().index() - weekday.index()).rem_euclid(7);
            let day = last as i32 - delta + (nth + 1) * 7;
            if day < 1 || day > last as i32 {
                None
            } else {
                Some(Self {
                    year,
                    month,
                    day: day as u8,
                })
            }
        }
    }
}

impl Time {
    pub fn new(hour: u8, minute: u8, second: u8) -> CalendarResult<Self> {
        if hour > 23 || minute > 59 || second > 60 {
            return Err(CalendarError::Parse(format!(
                "invalid time {hour:02}:{minute:02}:{second:02}"
            )));
        }
        Ok(Self {
            hour,
            minute,
            second,
        })
    }

    pub fn midnight() -> Self {
        Self {
            hour: 0,
            minute: 0,
            second: 0,
        }
    }

    pub fn ical(self) -> String {
        format!("{:02}{:02}{:02}", self.hour, self.minute, self.second)
    }
}

impl DateTime {
    pub fn new(date: Date, time: Time) -> Self {
        Self { date, time }
    }

    pub fn parse_ical(value: &str) -> CalendarResult<(Self, bool)> {
        let (stamp, utc) = if let Some(stripped) = value.strip_suffix('Z') {
            (stripped, true)
        } else {
            (value, false)
        };
        if stamp.len() != 15 || stamp.as_bytes().get(8) != Some(&b'T') {
            return Err(CalendarError::Parse(format!("invalid DATE-TIME {value}")));
        }
        let date = Date::parse_ical(&stamp[..8])?;
        let time = Time::new(
            stamp[9..11]
                .parse()
                .map_err(|_| CalendarError::Parse(format!("invalid DATE-TIME {value}")))?,
            stamp[11..13]
                .parse()
                .map_err(|_| CalendarError::Parse(format!("invalid DATE-TIME {value}")))?,
            stamp[13..15]
                .parse()
                .map_err(|_| CalendarError::Parse(format!("invalid DATE-TIME {value}")))?,
        )?;
        Ok((Self { date, time }, utc))
    }

    pub fn ical(self, utc: bool) -> String {
        let mut out = format!("{}T{}", self.date.ical(), self.time.ical());
        if utc {
            out.push('Z');
        }
        out
    }

    pub fn add_days(self, days: i32) -> Self {
        Self {
            date: self.date.add_days(days),
            time: self.time,
        }
    }
}

fn month_length(year: i32, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap(year) => 29,
        2 => 28,
        _ => 0,
    }
}

fn is_leap(year: i32) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

pub fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub fn format_rfc3339_utc(unix: i64) -> String {
    let dt = utc_datetime(unix);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        dt.date.year, dt.date.month, dt.date.day, dt.time.hour, dt.time.minute, dt.time.second
    )
}

pub fn utc_datetime(unix: i64) -> DateTime {
    let days = unix.div_euclid(86_400) as i32;
    let rem = unix.rem_euclid(86_400) as u32;
    DateTime {
        date: Date::from_rata(days),
        time: Time {
            hour: (rem / 3600) as u8,
            minute: ((rem % 3600) / 60) as u8,
            second: (rem % 60) as u8,
        },
    }
}

pub fn datetime_to_unix_utc(dt: DateTime) -> i64 {
    dt.date.to_rata() as i64 * 86_400
        + dt.time.hour as i64 * 3600
        + dt.time.minute as i64 * 60
        + dt.time.second as i64
}

// ---------------------------------------------------------------------------
// IANA time zones (bounded built-in rules for first-party expansion)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
struct ZoneRule {
    standard: i32,
    daylight: i32,
    kind: DstKind,
}

#[derive(Clone, Copy, Debug)]
enum DstKind {
    None,
    Us,
    Eu,
    Sydney,
}

fn zone_rule(tzid: &str) -> CalendarResult<ZoneRule> {
    match tzid {
        "" | "UTC" | "Etc/UTC" | "GMT" | "Z" => Ok(ZoneRule {
            standard: 0,
            daylight: 0,
            kind: DstKind::None,
        }),
        "America/New_York" => Ok(ZoneRule {
            standard: -5 * 3600,
            daylight: -4 * 3600,
            kind: DstKind::Us,
        }),
        "America/Chicago" => Ok(ZoneRule {
            standard: -6 * 3600,
            daylight: -5 * 3600,
            kind: DstKind::Us,
        }),
        "America/Denver" => Ok(ZoneRule {
            standard: -7 * 3600,
            daylight: -6 * 3600,
            kind: DstKind::Us,
        }),
        "America/Los_Angeles" => Ok(ZoneRule {
            standard: -8 * 3600,
            daylight: -7 * 3600,
            kind: DstKind::Us,
        }),
        "Europe/London" => Ok(ZoneRule {
            standard: 0,
            daylight: 3600,
            kind: DstKind::Eu,
        }),
        "Europe/Paris" | "Europe/Berlin" | "Europe/Amsterdam" => Ok(ZoneRule {
            standard: 3600,
            daylight: 2 * 3600,
            kind: DstKind::Eu,
        }),
        "Asia/Tokyo" => Ok(ZoneRule {
            standard: 9 * 3600,
            daylight: 9 * 3600,
            kind: DstKind::None,
        }),
        "Australia/Sydney" => Ok(ZoneRule {
            standard: 10 * 3600,
            daylight: 11 * 3600,
            kind: DstKind::Sydney,
        }),
        other => Err(CalendarError::InvalidRequest(format!(
            "unsupported IANA TZID {other}"
        ))),
    }
}

pub fn is_supported_tzid(tzid: &str) -> bool {
    zone_rule(tzid).is_ok()
}

fn nth_sunday(year: i32, month: u8, nth: i32) -> Date {
    Date::nth_weekday_of_month(year, month, Weekday::Sunday, nth).expect("sunday exists")
}

fn last_sunday(year: i32, month: u8) -> Date {
    Date::nth_weekday_of_month(year, month, Weekday::Sunday, -1).expect("sunday exists")
}

/// Convert a local civil time in `tzid` to a Unix instant.
///
/// Invalid spring-forward times return `None`. Ambiguous fall-back times use the
/// first (daylight) occurrence, matching RFC 5545 DATE-TIME.
pub fn local_to_unix(tzid: &str, local: DateTime) -> CalendarResult<Option<i64>> {
    let rule = zone_rule(tzid)?;
    Ok(local_to_unix_rule(rule, local))
}

fn local_to_unix_rule(rule: ZoneRule, local: DateTime) -> Option<i64> {
    let naive = datetime_to_unix_utc(local);
    match rule.kind {
        DstKind::None => Some(naive - rule.standard as i64),
        DstKind::Us => us_local_to_unix(rule, local, naive),
        DstKind::Eu => eu_local_to_unix(rule, local, naive),
        DstKind::Sydney => sydney_local_to_unix(rule, local, naive),
    }
}

fn us_local_to_unix(rule: ZoneRule, local: DateTime, naive: i64) -> Option<i64> {
    let start = DateTime {
        date: nth_sunday(local.date.year, 3, 2),
        time: Time::new(2, 0, 0).unwrap(),
    };
    let end = DateTime {
        date: nth_sunday(local.date.year, 11, 1),
        time: Time::new(2, 0, 0).unwrap(),
    };
    // 02:00-02:59 on the spring-forward Sunday does not exist.
    if local.date == start.date && local.time.hour == 2 {
        return None;
    }
    let in_dst = if local.date == end.date && local.time.hour < 2 {
        // 00:00-01:59 on the fall-back Sunday: first occurrence is still DST.
        true
    } else {
        local >= start && local < end
    };
    let offset = if in_dst { rule.daylight } else { rule.standard };
    Some(naive - offset as i64)
}

fn eu_local_to_unix(rule: ZoneRule, local: DateTime, naive: i64) -> Option<i64> {
    // Transitions are at 01:00 UTC.
    let start_utc = datetime_to_unix_utc(DateTime {
        date: last_sunday(local.date.year, 3),
        time: Time::new(1, 0, 0).unwrap(),
    });
    let end_utc = datetime_to_unix_utc(DateTime {
        date: last_sunday(local.date.year, 10),
        time: Time::new(1, 0, 0).unwrap(),
    });
    let as_std = naive - rule.standard as i64;
    let as_dst = naive - rule.daylight as i64;
    if as_std < start_utc {
        Some(as_std)
    } else if as_dst >= end_utc {
        Some(as_std)
    } else if as_dst < start_utc {
        // Spring gap in local time.
        None
    } else {
        Some(as_dst)
    }
}

fn sydney_local_to_unix(rule: ZoneRule, local: DateTime, naive: i64) -> Option<i64> {
    let start = DateTime {
        date: nth_sunday(local.date.year, 10, 1),
        time: Time::new(2, 0, 0).unwrap(),
    };
    let end = DateTime {
        date: nth_sunday(local.date.year, 4, 1),
        time: Time::new(3, 0, 0).unwrap(),
    };
    // Southern hemisphere: DST spans the year boundary.
    let in_dst = if local.date.month > 10 || local.date.month < 4 {
        true
    } else if local.date.month == 10 {
        local >= start
    } else if local.date.month == 4 {
        local < end
    } else {
        false
    };
    if local.date == start.date && local.time.hour == 2 {
        return None;
    }
    let offset = if in_dst { rule.daylight } else { rule.standard };
    Some(naive - offset as i64)
}

pub fn unix_to_local(tzid: &str, unix: i64) -> CalendarResult<DateTime> {
    let rule = zone_rule(tzid)?;
    Ok(unix_to_local_rule(rule, unix))
}

fn unix_to_local_rule(rule: ZoneRule, unix: i64) -> DateTime {
    let offset = match rule.kind {
        DstKind::None => rule.standard,
        DstKind::Us => {
            let local_std = utc_datetime(unix + rule.standard as i64);
            let start_utc = local_to_unix_rule(
                ZoneRule {
                    standard: rule.standard,
                    daylight: rule.standard,
                    kind: DstKind::None,
                },
                DateTime {
                    date: nth_sunday(local_std.date.year, 3, 2),
                    time: Time::new(2, 0, 0).unwrap(),
                },
            )
            .unwrap();
            let end_utc = {
                let end_local = DateTime {
                    date: nth_sunday(local_std.date.year, 11, 1),
                    time: Time::new(2, 0, 0).unwrap(),
                };
                datetime_to_unix_utc(end_local) - rule.daylight as i64
            };
            if unix >= start_utc && unix < end_utc {
                rule.daylight
            } else {
                rule.standard
            }
        }
        DstKind::Eu => {
            let year = utc_datetime(unix).date.year;
            let start = datetime_to_unix_utc(DateTime {
                date: last_sunday(year, 3),
                time: Time::new(1, 0, 0).unwrap(),
            });
            let end = datetime_to_unix_utc(DateTime {
                date: last_sunday(year, 10),
                time: Time::new(1, 0, 0).unwrap(),
            });
            if unix >= start && unix < end {
                rule.daylight
            } else {
                rule.standard
            }
        }
        DstKind::Sydney => {
            let local_std = utc_datetime(unix + rule.standard as i64);
            let year = local_std.date.year;
            let start = datetime_to_unix_utc(DateTime {
                date: nth_sunday(year, 10, 1),
                time: Time::new(2, 0, 0).unwrap(),
            }) - rule.standard as i64;
            let end = datetime_to_unix_utc(DateTime {
                date: nth_sunday(year, 4, 1),
                time: Time::new(3, 0, 0).unwrap(),
            }) - rule.daylight as i64;
            if unix >= start || unix < end {
                // year-spanning DST: after October start or before April end
                if local_std.date.month >= 10 || local_std.date.month < 4 {
                    rule.daylight
                } else if local_std.date.month == 4 {
                    if unix < end {
                        rule.daylight
                    } else {
                        rule.standard
                    }
                } else {
                    rule.standard
                }
            } else {
                rule.standard
            }
        }
    };
    utc_datetime(unix + offset as i64)
}

// ---------------------------------------------------------------------------
// iCalendar
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IcalParam {
    pub name: String,
    pub value: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IcalProperty {
    pub name: String,
    pub params: Vec<IcalParam>,
    pub value: String,
}

impl IcalProperty {
    pub fn new(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            params: Vec::new(),
            value: value.into(),
        }
    }

    pub fn with_param(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.params.push(IcalParam {
            name: name.into(),
            value: value.into(),
        });
        self
    }

    pub fn param(&self, name: &str) -> Option<&str> {
        self.params
            .iter()
            .find(|p| p.name.eq_ignore_ascii_case(name))
            .map(|p| p.value.as_str())
    }

    pub fn is_name(&self, name: &str) -> bool {
        self.name.eq_ignore_ascii_case(name)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IcalComponent {
    pub name: String,
    pub properties: Vec<IcalProperty>,
    pub components: Vec<IcalComponent>,
}

impl IcalComponent {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            properties: Vec::new(),
            components: Vec::new(),
        }
    }

    pub fn is_name(&self, name: &str) -> bool {
        self.name.eq_ignore_ascii_case(name)
    }

    pub fn property(&self, name: &str) -> Option<&IcalProperty> {
        self.properties.iter().find(|p| p.is_name(name))
    }

    pub fn property_mut(&mut self, name: &str) -> Option<&mut IcalProperty> {
        self.properties.iter_mut().find(|p| p.is_name(name))
    }

    pub fn properties_named(&self, name: &str) -> impl Iterator<Item = &IcalProperty> {
        self.properties.iter().filter(move |p| p.is_name(name))
    }

    pub fn set_or_insert(&mut self, name: &str, build: IcalProperty) {
        if let Some(existing) = self.property_mut(name) {
            *existing = build;
        } else {
            self.insert_known(build);
        }
    }

    pub fn remove_named(&mut self, name: &str) {
        self.properties.retain(|p| !p.is_name(name));
    }

    fn insert_known(&mut self, property: IcalProperty) {
        let order = known_property_rank(&property.name);
        let insert_at = self
            .properties
            .iter()
            .position(|p| known_property_rank(&p.name) > order)
            .unwrap_or(self.properties.len());
        self.properties.insert(insert_at, property);
    }

    pub fn text(&self, name: &str) -> Option<String> {
        self.property(name).map(|p| unescape_text(&p.value))
    }
}

fn known_property_rank(name: &str) -> u8 {
    match name.to_ascii_uppercase().as_str() {
        "UID" => 0,
        "DTSTAMP" => 1,
        "DTSTART" => 2,
        "DTEND" => 3,
        "DURATION" => 4,
        "RRULE" => 5,
        "EXDATE" => 6,
        "SUMMARY" => 7,
        "LOCATION" => 8,
        "DESCRIPTION" => 9,
        "STATUS" => 10,
        _ => 40,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IcalDocument {
    pub properties: Vec<IcalProperty>,
    pub components: Vec<IcalComponent>,
}

impl IcalDocument {
    pub fn vevents(&self) -> impl Iterator<Item = &IcalComponent> {
        self.components.iter().filter(|c| c.is_name("VEVENT"))
    }

    pub fn vevent_mut(&mut self) -> CalendarResult<&mut IcalComponent> {
        self.components
            .iter_mut()
            .find(|c| c.is_name("VEVENT"))
            .ok_or_else(|| CalendarError::Parse("iCalendar document has no VEVENT".into()))
    }

    pub fn vevent(&self) -> CalendarResult<&IcalComponent> {
        self.components
            .iter()
            .find(|c| c.is_name("VEVENT"))
            .ok_or_else(|| CalendarError::Parse("iCalendar document has no VEVENT".into()))
    }
}

pub fn unfold_ical(input: &str) -> String {
    let normalized = input.replace("\r\n", "\n").replace('\r', "\n");
    let mut out = String::with_capacity(normalized.len());
    let mut chars = normalized.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\n' {
            match chars.peek() {
                Some(' ') | Some('\t') => {
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

pub fn fold_ical_line(line: &str) -> String {
    let bytes = line.as_bytes();
    if bytes.len() <= ICAL_FOLD_OCTETS {
        return line.to_string();
    }
    let mut out = String::new();
    let mut start = 0;
    let mut first = true;
    while start < bytes.len() {
        let budget = if first {
            ICAL_FOLD_OCTETS
        } else {
            ICAL_FOLD_OCTETS - 1
        };
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
            ';' => out.push_str("\\;"),
            ',' => out.push_str("\\,"),
            '\n' => out.push_str("\\n"),
            '\r' => {}
            other => out.push(other),
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
                Some('n') | Some('N') => out.push('\n'),
                Some('\\') => out.push('\\'),
                Some(';') => out.push(';'),
                Some(',') => out.push(','),
                Some(other) => out.push(other),
                None => out.push('\\'),
            }
        } else {
            out.push(ch);
        }
    }
    out
}

fn parse_params(raw: &str) -> CalendarResult<Vec<IcalParam>> {
    let mut params = Vec::new();
    let mut rest = raw;
    while !rest.is_empty() {
        let name_end = rest
            .find('=')
            .ok_or_else(|| CalendarError::Parse(format!("invalid iCalendar parameter {raw}")))?;
        let name = rest[..name_end].to_string();
        rest = &rest[name_end + 1..];
        let value = if rest.starts_with('"') {
            let close = rest[1..]
                .find('"')
                .ok_or_else(|| CalendarError::Parse("unterminated quoted parameter".into()))?;
            let value = rest[1..1 + close].to_string();
            rest = &rest[2 + close..];
            if rest.starts_with(';') {
                rest = &rest[1..];
            }
            value
        } else {
            let end = rest.find(';').unwrap_or(rest.len());
            let value = rest[..end].to_string();
            rest = if end < rest.len() {
                &rest[end + 1..]
            } else {
                ""
            };
            value
        };
        params.push(IcalParam { name, value });
    }
    Ok(params)
}

fn parse_property_line(line: &str) -> CalendarResult<IcalProperty> {
    let colon = line
        .find(':')
        .ok_or_else(|| CalendarError::Parse(format!("iCalendar line missing value: {line}")))?;
    let head = &line[..colon];
    let value = line[colon + 1..].to_string();
    if let Some(semi) = head.find(';') {
        Ok(IcalProperty {
            name: head[..semi].to_string(),
            params: parse_params(&head[semi + 1..])?,
            value,
        })
    } else {
        Ok(IcalProperty {
            name: head.to_string(),
            params: Vec::new(),
            value,
        })
    }
}

fn write_property(out: &mut String, property: &IcalProperty) {
    let mut line = property.name.clone();
    for param in &property.params {
        line.push(';');
        line.push_str(&param.name);
        line.push('=');
        if param
            .value
            .chars()
            .any(|c| matches!(c, ';' | ':' | ',' | ' ' | '"'))
        {
            line.push('"');
            line.push_str(&param.value);
            line.push('"');
        } else {
            line.push_str(&param.value);
        }
    }
    line.push(':');
    line.push_str(&property.value);
    out.push_str(&fold_ical_line(&line));
    out.push_str("\r\n");
}

fn write_component(out: &mut String, component: &IcalComponent) {
    out.push_str("BEGIN:");
    out.push_str(&component.name);
    out.push_str("\r\n");
    for property in &component.properties {
        write_property(out, property);
    }
    for child in &component.components {
        write_component(out, child);
    }
    out.push_str("END:");
    out.push_str(&component.name);
    out.push_str("\r\n");
}

pub fn parse_ical(input: &str) -> CalendarResult<IcalDocument> {
    let unfolded = unfold_ical(input);
    let mut lines = unfolded
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.is_empty());
    let first = lines
        .next()
        .ok_or_else(|| CalendarError::Parse("empty iCalendar document".into()))?;
    if !first.eq_ignore_ascii_case("BEGIN:VCALENDAR") {
        return Err(CalendarError::Parse(
            "iCalendar document must begin with BEGIN:VCALENDAR".into(),
        ));
    }
    let mut document = IcalDocument {
        properties: Vec::new(),
        components: Vec::new(),
    };
    parse_body(
        &mut lines,
        &mut document.properties,
        &mut document.components,
        "VCALENDAR",
    )?;
    Ok(document)
}

fn parse_body<'a, I>(
    lines: &mut I,
    properties: &mut Vec<IcalProperty>,
    components: &mut Vec<IcalComponent>,
    expected_end: &str,
) -> CalendarResult<()>
where
    I: Iterator<Item = &'a str>,
{
    while let Some(line) = lines.next() {
        if let Some(name) = line.strip_prefix("BEGIN:") {
            let mut child = IcalComponent::new(name.trim());
            parse_body(
                lines,
                &mut child.properties,
                &mut child.components,
                name.trim(),
            )?;
            components.push(child);
        } else if let Some(name) = line.strip_prefix("END:") {
            if !name.trim().eq_ignore_ascii_case(expected_end) {
                return Err(CalendarError::Parse(format!(
                    "expected END:{expected_end}, found END:{}",
                    name.trim()
                )));
            }
            return Ok(());
        } else {
            properties.push(parse_property_line(line)?);
        }
    }
    Err(CalendarError::Parse(format!(
        "unterminated component {expected_end}"
    )))
}

pub fn serialize_ical(document: &IcalDocument) -> String {
    let mut out = String::from("BEGIN:VCALENDAR\r\n");
    if !document.properties.iter().any(|p| p.is_name("PRODID")) {
        write_property(
            &mut out,
            &IcalProperty::new("PRODID", "-//Foyer//Calendar//EN"),
        );
    }
    if !document.properties.iter().any(|p| p.is_name("VERSION")) {
        write_property(&mut out, &IcalProperty::new("VERSION", "2.0"));
    }
    if !document.properties.iter().any(|p| p.is_name("CALSCALE")) {
        write_property(&mut out, &IcalProperty::new("CALSCALE", "GREGORIAN"));
    }
    for property in &document.properties {
        write_property(&mut out, property);
    }
    for component in &document.components {
        write_component(&mut out, component);
    }
    out.push_str("END:VCALENDAR\r\n");
    out
}

pub fn new_event_document(
    uid: &str,
    draft: &EventDraft,
    now_unix: i64,
) -> CalendarResult<IcalDocument> {
    validate_draft(draft)?;
    let mut event = IcalComponent::new("VEVENT");
    event.properties.push(IcalProperty::new("UID", uid));
    event.properties.push(IcalProperty::new(
        "DTSTAMP",
        utc_datetime(now_unix).ical(true),
    ));
    apply_draft_to_vevent(&mut event, draft)?;
    Ok(IcalDocument {
        properties: vec![
            IcalProperty::new("PRODID", "-//Foyer//Calendar//EN"),
            IcalProperty::new("VERSION", "2.0"),
            IcalProperty::new("CALSCALE", "GREGORIAN"),
        ],
        components: vec![event],
    })
}

pub fn patch_event_document(
    document: &mut IcalDocument,
    draft: &EventDraft,
    now_unix: i64,
) -> CalendarResult<()> {
    validate_draft(draft)?;
    let event = document.vevent_mut()?;
    if let Some(stamp) = event.property_mut("DTSTAMP") {
        stamp.value = utc_datetime(now_unix).ical(true);
        stamp.params.clear();
    } else {
        event.properties.insert(
            0,
            IcalProperty::new("DTSTAMP", utc_datetime(now_unix).ical(true)),
        );
    }
    apply_draft_to_vevent(event, draft)
}

fn apply_draft_to_vevent(event: &mut IcalComponent, draft: &EventDraft) -> CalendarResult<()> {
    event.set_or_insert(
        "SUMMARY",
        IcalProperty::new("SUMMARY", escape_text(&draft.summary)),
    );
    event.set_or_insert(
        "DESCRIPTION",
        IcalProperty::new("DESCRIPTION", escape_text(&draft.description)),
    );
    if draft.location.is_empty() {
        event.remove_named("LOCATION");
    } else {
        event.set_or_insert(
            "LOCATION",
            IcalProperty::new("LOCATION", escape_text(&draft.location)),
        );
    }
    event.set_or_insert("DTSTART", datetime_property("DTSTART", draft)?);
    if let Some(end) = draft.dtend.as_ref() {
        let mut end_draft = draft.clone();
        end_draft.dtstart = end.clone();
        event.set_or_insert("DTEND", datetime_property("DTEND", &end_draft)?);
        event.remove_named("DURATION");
    }
    if let Some(rrule) = draft.rrule.as_ref().filter(|r| !r.is_empty()) {
        parse_rrule(rrule)?;
        event.set_or_insert("RRULE", IcalProperty::new("RRULE", rrule.clone()));
    } else {
        event.remove_named("RRULE");
    }
    event.remove_named("EXDATE");
    if !draft.exdates.is_empty() {
        for exdate in &draft.exdates {
            event.properties.push(exdate_property(draft, exdate)?);
        }
    }
    Ok(())
}

fn datetime_property(name: &str, draft: &EventDraft) -> CalendarResult<IcalProperty> {
    if draft.all_day {
        let date = Date::parse_ical(&normalize_date_stamp(&draft.dtstart)?)?;
        return Ok(IcalProperty::new(name, date.ical()).with_param("VALUE", "DATE"));
    }
    let (dt, utc) = DateTime::parse_ical(&normalize_datetime_stamp(&draft.dtstart)?)?;
    if utc
        || draft
            .tzid
            .as_deref()
            .is_none_or(|tz| tz.is_empty() || tz == "UTC")
    {
        Ok(IcalProperty::new(name, dt.ical(true)))
    } else {
        let tzid = draft.tzid.clone().unwrap();
        zone_rule(&tzid)?;
        Ok(IcalProperty::new(name, dt.ical(false)).with_param("TZID", tzid))
    }
}

fn exdate_property(draft: &EventDraft, stamp: &str) -> CalendarResult<IcalProperty> {
    if draft.all_day {
        let date = Date::parse_ical(&normalize_date_stamp(stamp)?)?;
        Ok(IcalProperty::new("EXDATE", date.ical()).with_param("VALUE", "DATE"))
    } else {
        let (dt, utc) = DateTime::parse_ical(&normalize_datetime_stamp(stamp)?)?;
        if utc
            || draft
                .tzid
                .as_deref()
                .is_none_or(|tz| tz.is_empty() || tz == "UTC")
        {
            Ok(IcalProperty::new("EXDATE", dt.ical(true)))
        } else {
            Ok(IcalProperty::new("EXDATE", dt.ical(false))
                .with_param("TZID", draft.tzid.clone().unwrap()))
        }
    }
}

fn normalize_date_stamp(value: &str) -> CalendarResult<String> {
    let compact: String = value
        .chars()
        .filter(|c| c.is_ascii_digit())
        .take(8)
        .collect();
    if compact.len() != 8 {
        return Err(CalendarError::InvalidRequest(format!(
            "invalid all-day date {value}"
        )));
    }
    Date::parse_ical(&compact)?;
    Ok(compact)
}

fn normalize_datetime_stamp(value: &str) -> CalendarResult<String> {
    let value = value.trim();
    if let Ok(timestamp) = chrono::DateTime::parse_from_rfc3339(value) {
        return Ok(timestamp
            .with_timezone(&chrono::Utc)
            .format("%Y%m%dT%H%M%SZ")
            .to_string());
    }
    if let Ok(timestamp) = chrono::NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S%.f") {
        return Ok(timestamp.format("%Y%m%dT%H%M%S").to_string());
    }
    if value.ends_with('Z') || value.contains('T') {
        let (dt, utc) = DateTime::parse_ical(value)?;
        return Ok(dt.ical(utc));
    }
    let digits: String = value.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() == 8 {
        return Ok(format!("{digits}T000000"));
    }
    if digits.len() >= 14 {
        let stamp = format!("{}T{}", &digits[..8], &digits[8..14]);
        DateTime::parse_ical(&stamp)?;
        return Ok(stamp);
    }
    Err(CalendarError::InvalidRequest(format!(
        "invalid date-time {value}"
    )))
}

#[cfg(test)]
mod datetime_normalization_tests {
    use super::normalize_datetime_stamp;

    #[test]
    fn accepts_contract_rfc3339_timestamps() {
        assert_eq!(
            normalize_datetime_stamp("2026-08-15T10:00:00Z").unwrap(),
            "20260815T100000Z"
        );
        assert_eq!(
            normalize_datetime_stamp("2026-08-15T15:30:00+05:30").unwrap(),
            "20260815T100000Z"
        );
    }

    #[test]
    fn accepts_floating_local_contract_timestamps() {
        assert_eq!(
            normalize_datetime_stamp("2026-08-15T10:00:00").unwrap(),
            "20260815T100000"
        );
    }
}

// ---------------------------------------------------------------------------
// Recurrence
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Freq {
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecurrenceRule {
    pub freq: Freq,
    pub interval: u32,
    pub count: Option<u32>,
    pub until: Option<RecurrenceUntil>,
    pub by_day: Vec<ByDay>,
    pub by_month_day: Vec<i32>,
    pub by_month: Vec<u8>,
    pub by_set_pos: Vec<i32>,
    pub week_start: Weekday,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ByDay {
    pub weekday: Weekday,
    pub nth: Option<i32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecurrenceUntil {
    Date(Date),
    DateTimeUtc(i64),
}

impl RecurrenceRule {
    pub fn ical(&self) -> String {
        let mut parts = vec![format!(
            "FREQ={}",
            match self.freq {
                Freq::Daily => "DAILY",
                Freq::Weekly => "WEEKLY",
                Freq::Monthly => "MONTHLY",
                Freq::Yearly => "YEARLY",
            }
        )];
        if self.interval != 1 {
            parts.push(format!("INTERVAL={}", self.interval));
        }
        if let Some(count) = self.count {
            parts.push(format!("COUNT={count}"));
        }
        if let Some(until) = self.until {
            match until {
                RecurrenceUntil::Date(date) => parts.push(format!("UNTIL={}", date.ical())),
                RecurrenceUntil::DateTimeUtc(unix) => {
                    parts.push(format!("UNTIL={}", utc_datetime(unix).ical(true)))
                }
            }
        }
        if !self.by_day.is_empty() {
            let days = self
                .by_day
                .iter()
                .map(|d| match d.nth {
                    Some(n) => format!("{n}{}", d.weekday.ical()),
                    None => d.weekday.ical().to_string(),
                })
                .collect::<Vec<_>>()
                .join(",");
            parts.push(format!("BYDAY={days}"));
        }
        if !self.by_month_day.is_empty() {
            parts.push(format!(
                "BYMONTHDAY={}",
                self.by_month_day
                    .iter()
                    .map(|d| d.to_string())
                    .collect::<Vec<_>>()
                    .join(",")
            ));
        }
        if !self.by_month.is_empty() {
            parts.push(format!(
                "BYMONTH={}",
                self.by_month
                    .iter()
                    .map(|m| m.to_string())
                    .collect::<Vec<_>>()
                    .join(",")
            ));
        }
        if !self.by_set_pos.is_empty() {
            parts.push(format!(
                "BYSETPOS={}",
                self.by_set_pos
                    .iter()
                    .map(|p| p.to_string())
                    .collect::<Vec<_>>()
                    .join(",")
            ));
        }
        if self.week_start != Weekday::Monday {
            parts.push(format!("WKST={}", self.week_start.ical()));
        }
        parts.join(";")
    }
}

pub fn parse_rrule(value: &str) -> CalendarResult<RecurrenceRule> {
    let mut freq = None;
    let mut interval = 1u32;
    let mut count = None;
    let mut until = None;
    let mut by_day = Vec::new();
    let mut by_month_day = Vec::new();
    let mut by_month = Vec::new();
    let mut by_set_pos = Vec::new();
    let mut week_start = Weekday::Monday;
    for part in value.split(';').filter(|p| !p.is_empty()) {
        let (key, raw) = part
            .split_once('=')
            .ok_or_else(|| CalendarError::InvalidRequest(format!("invalid RRULE part {part}")))?;
        match key.to_ascii_uppercase().as_str() {
            "FREQ" => {
                freq = Some(match raw {
                    "DAILY" => Freq::Daily,
                    "WEEKLY" => Freq::Weekly,
                    "MONTHLY" => Freq::Monthly,
                    "YEARLY" => Freq::Yearly,
                    other => {
                        return Err(CalendarError::InvalidRequest(format!(
                            "unsupported FREQ {other}"
                        )));
                    }
                });
            }
            "INTERVAL" => {
                interval = raw.parse().map_err(|_| {
                    CalendarError::InvalidRequest(format!("invalid INTERVAL {raw}"))
                })?;
                if interval == 0 {
                    return Err(CalendarError::InvalidRequest(
                        "INTERVAL must be at least 1".into(),
                    ));
                }
            }
            "COUNT" => {
                count =
                    Some(raw.parse().map_err(|_| {
                        CalendarError::InvalidRequest(format!("invalid COUNT {raw}"))
                    })?);
            }
            "UNTIL" => {
                until = Some(if raw.len() == 8 {
                    RecurrenceUntil::Date(Date::parse_ical(raw)?)
                } else {
                    let (dt, utc) = DateTime::parse_ical(raw)?;
                    let unix = if utc {
                        datetime_to_unix_utc(dt)
                    } else {
                        datetime_to_unix_utc(dt)
                    };
                    RecurrenceUntil::DateTimeUtc(unix)
                });
            }
            "BYDAY" => {
                for token in raw.split(',') {
                    by_day.push(parse_byday(token)?);
                }
            }
            "BYMONTHDAY" => {
                for token in raw.split(',') {
                    by_month_day.push(token.parse().map_err(|_| {
                        CalendarError::InvalidRequest(format!("invalid BYMONTHDAY {token}"))
                    })?);
                }
            }
            "BYMONTH" => {
                for token in raw.split(',') {
                    let month: u8 = token.parse().map_err(|_| {
                        CalendarError::InvalidRequest(format!("invalid BYMONTH {token}"))
                    })?;
                    if !(1..=12).contains(&month) {
                        return Err(CalendarError::InvalidRequest(format!(
                            "invalid BYMONTH {token}"
                        )));
                    }
                    by_month.push(month);
                }
            }
            "BYSETPOS" => {
                for token in raw.split(',') {
                    by_set_pos.push(token.parse().map_err(|_| {
                        CalendarError::InvalidRequest(format!("invalid BYSETPOS {token}"))
                    })?);
                }
            }
            "WKST" => week_start = Weekday::parse_ical(raw)?,
            "BYHOUR" | "BYMINUTE" | "BYSECOND" | "BYWEEKNO" | "BYYEARDAY" | "BYEASTER" => {
                return Err(CalendarError::InvalidRequest(format!(
                    "unsupported RRULE part {key}"
                )));
            }
            other => {
                return Err(CalendarError::InvalidRequest(format!(
                    "unsupported RRULE part {other}"
                )));
            }
        }
    }
    let freq = freq.ok_or_else(|| CalendarError::InvalidRequest("RRULE requires FREQ".into()))?;
    if count.is_some() && until.is_some() {
        return Err(CalendarError::InvalidRequest(
            "RRULE cannot combine COUNT and UNTIL".into(),
        ));
    }
    Ok(RecurrenceRule {
        freq,
        interval,
        count,
        until,
        by_day,
        by_month_day,
        by_month,
        by_set_pos,
        week_start,
    })
}

fn parse_byday(token: &str) -> CalendarResult<ByDay> {
    if token.len() < 2 {
        return Err(CalendarError::InvalidRequest(format!(
            "invalid BYDAY {token}"
        )));
    }
    let (nth, day) = if token.as_bytes()[token.len() - 2].is_ascii_alphabetic() {
        let split = token.len() - 2;
        let day = Weekday::parse_ical(&token[split..])?;
        let nth = if split == 0 {
            None
        } else {
            Some(
                token[..split]
                    .parse()
                    .map_err(|_| CalendarError::InvalidRequest(format!("invalid BYDAY {token}")))?,
            )
        };
        (nth, day)
    } else {
        return Err(CalendarError::InvalidRequest(format!(
            "invalid BYDAY {token}"
        )));
    };
    if matches!(nth, Some(0)) {
        return Err(CalendarError::InvalidRequest(
            "BYDAY occurrence cannot be 0".into(),
        ));
    }
    Ok(ByDay { weekday: day, nth })
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Occurrence {
    #[serde(rename = "eventId")]
    pub event_id: String,
    #[serde(rename = "calendarId")]
    pub calendar_id: String,
    pub uid: String,
    pub summary: String,
    pub description: String,
    pub location: String,
    #[serde(rename = "allDay")]
    pub all_day: bool,
    pub tzid: Option<String>,
    #[serde(rename = "startUnix")]
    pub start_unix: Option<i64>,
    #[serde(rename = "endUnix")]
    pub end_unix: Option<i64>,
    #[serde(rename = "startLocal")]
    pub start_local: String,
    #[serde(rename = "endLocal")]
    pub end_local: Option<String>,
    #[serde(rename = "recurrenceId")]
    pub recurrence_id: String,
    #[serde(rename = "isRecurring")]
    pub is_recurring: bool,
}

pub fn expand_event(
    event: &EventRecord,
    window_start: Date,
    window_end: Date,
    limit: usize,
) -> CalendarResult<Vec<Occurrence>> {
    if window_end < window_start {
        return Err(CalendarError::InvalidRequest(
            "expansion window end must not precede start".into(),
        ));
    }
    if window_end.to_rata() - window_start.to_rata() > MAX_WINDOW_DAYS {
        return Err(CalendarError::InvalidRequest(format!(
            "expansion window may be at most {MAX_WINDOW_DAYS} days"
        )));
    }
    let limit = limit.clamp(1, MAX_EXPANSION_INSTANCES);
    let seed = event_seed(event)?;
    let duration = event_duration_seconds(event, &seed)?;
    let exclusions = event_exdates(event)?;
    let mut dates = if let Some(rrule) = event.rrule.as_deref().filter(|r| !r.is_empty()) {
        let rule = parse_rrule(rrule)?;
        expand_rule(&rule, seed.date, window_start, window_end, limit * 4)?
    } else if seed.date >= window_start && seed.date <= window_end {
        vec![seed.date]
    } else {
        Vec::new()
    };
    dates.retain(|date| !exclusions.contains(date));
    dates.retain(|date| *date >= window_start && *date <= window_end);
    dates.sort();
    dates.dedup();
    dates.truncate(limit);

    let mut out = Vec::new();
    for date in dates {
        let local = DateTime {
            date,
            time: seed.time,
        };
        let (start_unix, start_local) = if event.all_day {
            (None, date.iso())
        } else if let Some(tzid) = event
            .tzid
            .as_deref()
            .filter(|t| !t.is_empty() && *t != "UTC")
        {
            match local_to_unix(tzid, local)? {
                Some(unix) => (Some(unix), format!("{}T{}", date.ical(), seed.time.ical())),
                None => continue,
            }
        } else {
            let unix = datetime_to_unix_utc(local);
            (Some(unix), local.ical(true))
        };
        let end_unix = start_unix.map(|s| s + duration);
        let end_local = if event.all_day {
            event.dtend.clone().map(|end| {
                normalize_date_stamp(&end)
                    .ok()
                    .and_then(|stamp| Date::parse_ical(&stamp).ok())
                    .map(|d| d.iso())
                    .unwrap_or(end)
            })
        } else {
            end_unix.map(|unix| {
                if let Some(tzid) = event
                    .tzid
                    .as_deref()
                    .filter(|t| !t.is_empty() && *t != "UTC")
                {
                    let local = unix_to_local(tzid, unix).unwrap_or_else(|_| utc_datetime(unix));
                    format!("{}T{}", local.date.ical(), local.time.ical())
                } else {
                    utc_datetime(unix).ical(true)
                }
            })
        };
        out.push(Occurrence {
            event_id: event.id.clone(),
            calendar_id: event.calendar_id.clone(),
            uid: event.uid.clone(),
            summary: event.summary.clone(),
            description: event.description.clone(),
            location: event.location.clone(),
            all_day: event.all_day,
            tzid: event.tzid.clone(),
            start_unix,
            end_unix,
            start_local,
            end_local,
            recurrence_id: if event.all_day {
                date.ical()
            } else {
                format!("{}T{}", date.ical(), seed.time.ical())
            },
            is_recurring: event.rrule.as_deref().is_some_and(|r| !r.is_empty()),
        });
        if out.len() == limit {
            break;
        }
    }
    Ok(out)
}

struct EventSeed {
    date: Date,
    time: Time,
}

fn event_seed(event: &EventRecord) -> CalendarResult<EventSeed> {
    if event.all_day {
        let date = Date::parse_ical(&normalize_date_stamp(&event.dtstart)?)?;
        return Ok(EventSeed {
            date,
            time: Time::midnight(),
        });
    }
    let (dt, _) = DateTime::parse_ical(&normalize_datetime_stamp(&event.dtstart)?)?;
    Ok(EventSeed {
        date: dt.date,
        time: dt.time,
    })
}

fn event_duration_seconds(event: &EventRecord, seed: &EventSeed) -> CalendarResult<i64> {
    let Some(end) = event.dtend.as_deref().filter(|s| !s.is_empty()) else {
        return Ok(if event.all_day { 86_400 } else { 3600 });
    };
    if event.all_day {
        let end_date = Date::parse_ical(&normalize_date_stamp(end)?)?;
        return Ok((end_date.to_rata() - seed.date.to_rata()) as i64 * 86_400);
    }
    let (end_dt, _) = DateTime::parse_ical(&normalize_datetime_stamp(end)?)?;
    if let Some(tzid) = event
        .tzid
        .as_deref()
        .filter(|t| !t.is_empty() && *t != "UTC")
    {
        let start = local_to_unix(
            tzid,
            DateTime {
                date: seed.date,
                time: seed.time,
            },
        )?
        .unwrap_or_else(|| {
            datetime_to_unix_utc(DateTime {
                date: seed.date,
                time: seed.time,
            })
        });
        let end_unix = local_to_unix(tzid, end_dt)?.unwrap_or_else(|| datetime_to_unix_utc(end_dt));
        Ok((end_unix - start).max(0))
    } else {
        Ok((datetime_to_unix_utc(end_dt)
            - datetime_to_unix_utc(DateTime {
                date: seed.date,
                time: seed.time,
            }))
        .max(0))
    }
}

fn event_exdates(event: &EventRecord) -> CalendarResult<BTreeSet<Date>> {
    let mut out = BTreeSet::new();
    for raw in parse_exdate_list(&event.exdates) {
        if raw.len() == 8 || event.all_day {
            out.insert(Date::parse_ical(&normalize_date_stamp(&raw)?)?);
        } else {
            let (dt, _) = DateTime::parse_ical(&normalize_datetime_stamp(&raw)?)?;
            out.insert(dt.date);
        }
    }
    Ok(out)
}

pub fn parse_exdate_list(raw: &str) -> Vec<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed == "[]" {
        return Vec::new();
    }
    if trimmed.starts_with('[') {
        trimmed
            .trim_matches(|c| c == '[' || c == ']')
            .split(',')
            .filter_map(|part| {
                let cleaned = part.trim().trim_matches('"').trim();
                if cleaned.is_empty() {
                    None
                } else {
                    Some(cleaned.to_string())
                }
            })
            .collect()
    } else {
        trimmed
            .split([',', ';'])
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string)
            .collect()
    }
}

pub fn encode_exdates(values: &[String]) -> String {
    let mut out = String::from("[");
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push('"');
        out.push_str(value);
        out.push('"');
    }
    out.push(']');
    out
}

fn expand_rule(
    rule: &RecurrenceRule,
    seed: Date,
    window_start: Date,
    window_end: Date,
    safety: usize,
) -> CalendarResult<Vec<Date>> {
    let mut dates = Vec::new();
    let mut emitted = 0u32;
    let mut cursor = seed;
    let mut iterations = 0usize;
    let hard = (safety.max(16) * 8).max(64);
    while iterations < hard && dates.len() < safety {
        iterations += 1;
        let mut set = period_candidates(rule, seed, cursor);
        if !rule.by_month.is_empty() {
            set.retain(|d| rule.by_month.contains(&d.month));
        }
        if !rule.by_month_day.is_empty() {
            set.retain(|d| matches_month_day(*d, &rule.by_month_day));
        }
        if !rule.by_day.is_empty() && rule.freq != Freq::Weekly {
            set.retain(|d| matches_byday(*d, &rule.by_day));
        }
        if rule.freq == Freq::Weekly && !rule.by_day.is_empty() {
            set = weekly_byday(cursor, seed, rule);
        }
        if !rule.by_set_pos.is_empty() {
            set.sort();
            set.dedup();
            set = apply_set_pos(&set, &rule.by_set_pos);
        }
        set.sort();
        set.dedup();
        for date in set {
            if date < seed {
                continue;
            }
            if let Some(until) = rule.until {
                match until {
                    RecurrenceUntil::Date(limit) if date > limit => return Ok(dates),
                    RecurrenceUntil::DateTimeUtc(_) if date > window_end && date > seed => {}
                    _ => {}
                }
                if let RecurrenceUntil::Date(limit) = until {
                    if date > limit {
                        return Ok(dates);
                    }
                }
            }
            emitted += 1;
            if let Some(count) = rule.count {
                if emitted > count {
                    return Ok(dates);
                }
            }
            if date > window_end && date > seed {
                if date > window_end.add_days(366) {
                    return Ok(dates);
                }
            }
            if date >= window_start && date <= window_end {
                dates.push(date);
            }
            if let Some(RecurrenceUntil::DateTimeUtc(_)) = rule.until {
                if date > window_end {
                    return Ok(dates);
                }
            }
        }
        let Some(next) = advance_period(rule, cursor) else {
            break;
        };
        if next < cursor && rule.freq != Freq::Yearly {
            break;
        }
        cursor = next;
        if cursor > window_end.add_days(400) {
            break;
        }
    }
    Ok(dates)
}

fn period_candidates(rule: &RecurrenceRule, seed: Date, cursor: Date) -> Vec<Date> {
    match rule.freq {
        Freq::Daily => vec![cursor],
        Freq::Weekly => {
            if rule.by_day.is_empty() {
                vec![cursor]
            } else {
                weekly_byday(cursor, seed, rule)
            }
        }
        Freq::Monthly => {
            if rule.by_day.iter().any(|d| d.nth.is_some()) {
                rule.by_day
                    .iter()
                    .filter_map(|d| {
                        Date::nth_weekday_of_month(
                            cursor.year,
                            cursor.month,
                            d.weekday,
                            d.nth.unwrap_or(1),
                        )
                    })
                    .collect()
            } else if !rule.by_month_day.is_empty() {
                rule.by_month_day
                    .iter()
                    .filter_map(|day| month_day(cursor.year, cursor.month, *day))
                    .collect()
            } else if let Some(date) = cursor
                .add_months(0)
                .and_then(|_| Date::new(cursor.year, cursor.month, seed.day).ok())
            {
                vec![date]
            } else {
                Vec::new()
            }
        }
        Freq::Yearly => {
            if !rule.by_month.is_empty() {
                rule.by_month
                    .iter()
                    .filter_map(|month| Date::new(cursor.year, *month, seed.day).ok())
                    .collect()
            } else if let Some(date) = seed.add_years(cursor.year - seed.year) {
                vec![date]
            } else {
                Vec::new()
            }
        }
    }
}

fn weekly_byday(cursor: Date, seed: Date, rule: &RecurrenceRule) -> Vec<Date> {
    let week_start = week_start_date(cursor, rule.week_start);
    let seed_week = week_start_date(seed, rule.week_start);
    let weeks = (week_start.to_rata() - seed_week.to_rata()) / 7;
    if weeks.rem_euclid(rule.interval as i32) != 0 {
        return Vec::new();
    }
    rule.by_day
        .iter()
        .map(|day| {
            let delta = (day.weekday.index() - rule.week_start.index()).rem_euclid(7);
            week_start.add_days(delta)
        })
        .collect()
}

fn week_start_date(date: Date, week_start: Weekday) -> Date {
    let delta = (date.weekday().index() - week_start.index()).rem_euclid(7);
    date.add_days(-delta)
}

fn matches_month_day(date: Date, days: &[i32]) -> bool {
    let last = month_length(date.year, date.month) as i32;
    days.iter().any(|day| {
        if *day > 0 {
            date.day as i32 == *day
        } else {
            date.day as i32 == last + day + 1
        }
    })
}

fn matches_byday(date: Date, days: &[ByDay]) -> bool {
    days.iter().any(|spec| {
        if date.weekday() != spec.weekday {
            return false;
        }
        match spec.nth {
            None => true,
            Some(n) => {
                Date::nth_weekday_of_month(date.year, date.month, spec.weekday, n) == Some(date)
            }
        }
    })
}

fn month_day(year: i32, month: u8, day: i32) -> Option<Date> {
    let last = month_length(year, month) as i32;
    let resolved = if day > 0 { day } else { last + day + 1 };
    Date::new(year, month, resolved as u8).ok()
}

fn apply_set_pos(dates: &[Date], positions: &[i32]) -> Vec<Date> {
    let len = dates.len() as i32;
    positions
        .iter()
        .filter_map(|pos| {
            let index = if *pos > 0 { *pos - 1 } else { len + *pos };
            dates.get(index as usize).copied()
        })
        .collect()
}

fn advance_period(rule: &RecurrenceRule, cursor: Date) -> Option<Date> {
    match rule.freq {
        Freq::Daily => Some(cursor.add_days(rule.interval as i32)),
        Freq::Weekly => Some(cursor.add_days(7 * rule.interval as i32)),
        Freq::Monthly => cursor.add_months(rule.interval as i32),
        Freq::Yearly => cursor.add_years(rule.interval as i32),
    }
}

// ---------------------------------------------------------------------------
// Normalized records / commands
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CalendarRecord {
    pub id: String,
    pub user_id: String,
    pub uid: String,
    pub href: String,
    pub etag: String,
    pub display_name: String,
    pub description: String,
    pub color: Option<String>,
    pub ctag: Option<String>,
    pub sync_token: Option<String>,
    pub revision: i64,
    pub created_at: String,
    pub updated_at: String,
    pub deleted_at: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventRecord {
    pub id: String,
    pub user_id: String,
    pub calendar_id: String,
    pub uid: String,
    pub href: String,
    pub etag: String,
    pub summary: String,
    pub description: String,
    pub location: String,
    pub all_day: bool,
    pub dtstart: String,
    pub dtend: Option<String>,
    pub tzid: Option<String>,
    pub rrule: Option<String>,
    pub exdates: String,
    pub revision: i64,
    pub created_at: String,
    pub updated_at: String,
    pub deleted_at: Option<String>,
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
}

impl EventDraft {
    pub fn fingerprint(&self) -> String {
        format!(
            "summary={}\ndescription={}\nlocation={}\nallDay={}\ndtstart={}\ndtend={}\ntzid={}\nrrule={}\nexdates={}",
            self.summary,
            self.description,
            self.location,
            self.all_day,
            self.dtstart,
            self.dtend.clone().unwrap_or_default(),
            self.tzid.clone().unwrap_or_default(),
            self.rrule.clone().unwrap_or_default(),
            self.exdates.join(",")
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CreateCalendar {
    pub operation_id: String,
    pub id: String,
    pub display_name: String,
    pub description: String,
    pub color: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenameCalendar {
    pub operation_id: String,
    pub expected_revision: i64,
    pub expected_etag: Option<String>,
    pub display_name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeleteCommand {
    pub operation_id: String,
    pub expected_revision: i64,
    pub expected_etag: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CreateEvent {
    pub operation_id: String,
    pub id: String,
    pub calendar_id: String,
    pub uid: Option<String>,
    pub draft: EventDraft,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdateEvent {
    pub operation_id: String,
    pub expected_revision: i64,
    pub expected_etag: Option<String>,
    pub draft: EventDraft,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MoveEvent {
    pub operation_id: String,
    pub expected_revision: i64,
    pub expected_etag: Option<String>,
    pub calendar_id: String,
}

fn validate_draft(draft: &EventDraft) -> CalendarResult<()> {
    let summary = draft.summary.trim();
    if summary.is_empty() {
        return Err(CalendarError::InvalidRequest(
            "Event summary is required.".into(),
        ));
    }
    if summary.chars().count() > MAX_SUMMARY_CHARS {
        return Err(CalendarError::LimitExceeded(format!(
            "Summary may be at most {MAX_SUMMARY_CHARS} characters."
        )));
    }
    if draft.location.chars().count() > MAX_LOCATION_CHARS {
        return Err(CalendarError::LimitExceeded(format!(
            "Location may be at most {MAX_LOCATION_CHARS} characters."
        )));
    }
    if draft.description.len() > MAX_DESCRIPTION_BYTES {
        return Err(CalendarError::LimitExceeded(format!(
            "Description may be at most {MAX_DESCRIPTION_BYTES} bytes."
        )));
    }
    if draft.all_day {
        normalize_date_stamp(&draft.dtstart)?;
        if let Some(end) = draft.dtend.as_deref() {
            normalize_date_stamp(end)?;
        }
    } else {
        normalize_datetime_stamp(&draft.dtstart)?;
        if let Some(end) = draft.dtend.as_deref() {
            normalize_datetime_stamp(end)?;
        }
        if let Some(tzid) = draft.tzid.as_deref().filter(|t| !t.is_empty()) {
            zone_rule(tzid)?;
        }
    }
    if let Some(rrule) = draft.rrule.as_deref().filter(|r| !r.is_empty()) {
        parse_rrule(rrule)?;
    }
    Ok(())
}

pub fn parse_uuid(field: &str, value: &str) -> CalendarResult<String> {
    let trimmed = value.trim();
    if trimmed.len() != 36 {
        return Err(CalendarError::InvalidRequest(format!(
            "{field} must be a UUID."
        )));
    }
    let bytes = trimmed.as_bytes();
    let hex = |i: usize| bytes[i].is_ascii_hexdigit();
    let dash = |i: usize| bytes[i] == b'-';
    if hex(0)
        && hex(1)
        && hex(2)
        && hex(3)
        && hex(4)
        && hex(5)
        && hex(6)
        && hex(7)
        && dash(8)
        && hex(9)
        && hex(10)
        && hex(11)
        && hex(12)
        && dash(13)
        && hex(14)
        && hex(15)
        && hex(16)
        && hex(17)
        && dash(18)
        && hex(19)
        && hex(20)
        && hex(21)
        && hex(22)
        && dash(23)
        && (24..36).all(|i| hex(i))
    {
        Ok(trimmed.to_ascii_lowercase())
    } else {
        Err(CalendarError::InvalidRequest(format!(
            "{field} must be a UUID."
        )))
    }
}

pub fn event_from_ical(
    id: &str,
    user_id: &str,
    calendar_id: &str,
    href: &str,
    etag: &str,
    revision: i64,
    created_at: &str,
    updated_at: &str,
    ical: &str,
) -> CalendarResult<EventRecord> {
    let document = parse_ical(ical)?;
    let event = document.vevent()?;
    let uid = event
        .text("UID")
        .filter(|v| !v.is_empty())
        .ok_or_else(|| CalendarError::Parse("VEVENT is missing UID".into()))?;
    let summary = event.text("SUMMARY").unwrap_or_default();
    let description = event.text("DESCRIPTION").unwrap_or_default();
    let location = event.text("LOCATION").unwrap_or_default();
    let dtstart = event
        .property("DTSTART")
        .ok_or_else(|| CalendarError::Parse("VEVENT is missing DTSTART".into()))?;
    let all_day = dtstart
        .param("VALUE")
        .is_some_and(|v| v.eq_ignore_ascii_case("DATE"))
        || dtstart.value.len() == 8;
    let tzid = dtstart
        .param("TZID")
        .map(ToString::to_string)
        .filter(|v| !v.is_empty());
    let dtend = event.property("DTEND").map(|p| p.value.clone());
    let rrule = event.property("RRULE").map(|p| p.value.clone());
    let mut exdates = Vec::new();
    for property in event.properties_named("EXDATE") {
        for value in property.value.split(',') {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                exdates.push(trimmed.to_string());
            }
        }
    }
    Ok(EventRecord {
        id: id.to_string(),
        user_id: user_id.to_string(),
        calendar_id: calendar_id.to_string(),
        uid,
        href: href.to_string(),
        etag: etag.to_string(),
        summary,
        description,
        location,
        all_day,
        dtstart: dtstart.value.clone(),
        dtend,
        tzid,
        rrule,
        exdates: encode_exdates(&exdates),
        revision,
        created_at: created_at.to_string(),
        updated_at: updated_at.to_string(),
        deleted_at: None,
    })
}

// ---------------------------------------------------------------------------
// DAV
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DavResource {
    pub href: String,
    pub etag: String,
    pub body: String,
    pub content_type: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DavCollection {
    pub href: String,
    pub display_name: String,
    pub ctag: String,
    pub sync_token: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DavPrecondition {
    IfMatch(Option<&'static str>),
    IfMatchEtag,
    IfNoneMatchStar,
}

pub trait DavStore {
    fn mkcalendar(&mut self, href: &str, display_name: &str) -> CalendarResult<DavCollection>;
    fn propfind_collection(&self, href: &str) -> CalendarResult<DavCollection>;
    fn set_display_name(
        &mut self,
        href: &str,
        expected_ctag: Option<&str>,
        display_name: &str,
    ) -> CalendarResult<DavCollection>;
    fn delete_collection(&mut self, href: &str, expected_ctag: Option<&str>) -> CalendarResult<()>;
    fn put(
        &mut self,
        href: &str,
        body: &str,
        if_match: Option<&str>,
        if_none_match_star: bool,
    ) -> CalendarResult<DavResource>;
    fn get(&self, href: &str) -> CalendarResult<DavResource>;
    fn delete(&mut self, href: &str, if_match: Option<&str>) -> CalendarResult<()>;
    fn move_resource(
        &mut self,
        href: &str,
        destination: &str,
        if_match: Option<&str>,
    ) -> CalendarResult<DavResource>;
    fn list(&self, collection_href: &str) -> CalendarResult<Vec<DavResource>>;
}

#[derive(Default)]
pub struct MemoryDav {
    collections: BTreeMap<String, DavCollection>,
    resources: BTreeMap<String, DavResource>,
    next_etag: u64,
}

impl MemoryDav {
    pub fn new() -> Self {
        Self::default()
    }

    fn bump(&mut self) -> String {
        self.next_etag += 1;
        format!("\"e{}\"", self.next_etag)
    }

    fn bump_collection(&mut self, href: &str) -> CalendarResult<()> {
        let etag = self.bump();
        let collection = self
            .collections
            .get_mut(href)
            .ok_or_else(|| CalendarError::NotFound(format!("collection {href} not found")))?;
        collection.ctag = etag.clone();
        collection.sync_token = format!("sync-{}", etag.trim_matches('"'));
        Ok(())
    }

    fn check_match(
        resource: Option<&DavResource>,
        if_match: Option<&str>,
        if_none_match_star: bool,
    ) -> CalendarResult<()> {
        if if_none_match_star {
            if resource.is_some() {
                return Err(CalendarError::Conflict(
                    "A resource already exists at this href.".into(),
                ));
            }
            return Ok(());
        }
        if let Some(expected) = if_match {
            match resource {
                None => {
                    return Err(CalendarError::StaleEtag {
                        expected: expected.to_string(),
                        actual: String::new(),
                    });
                }
                Some(resource) if normalize_etag(&resource.etag) != normalize_etag(expected) => {
                    return Err(CalendarError::StaleEtag {
                        expected: expected.to_string(),
                        actual: resource.etag.clone(),
                    });
                }
                Some(_) => {}
            }
        }
        Ok(())
    }
}

pub fn normalize_etag(value: &str) -> String {
    value.trim().trim_matches('"').to_string()
}

impl DavStore for MemoryDav {
    fn mkcalendar(&mut self, href: &str, display_name: &str) -> CalendarResult<DavCollection> {
        let href = normalize_collection_href(href);
        if self.collections.contains_key(&href) {
            return Err(CalendarError::Conflict(format!(
                "Calendar collection {href} already exists."
            )));
        }
        let etag = self.bump();
        let collection = DavCollection {
            href: href.clone(),
            display_name: display_name.to_string(),
            ctag: etag.clone(),
            sync_token: format!("sync-{}", etag.trim_matches('"')),
        };
        self.collections.insert(href, collection.clone());
        Ok(collection)
    }

    fn propfind_collection(&self, href: &str) -> CalendarResult<DavCollection> {
        let href = normalize_collection_href(href);
        self.collections
            .get(&href)
            .cloned()
            .ok_or_else(|| CalendarError::NotFound(format!("collection {href} not found")))
    }

    fn set_display_name(
        &mut self,
        href: &str,
        expected_ctag: Option<&str>,
        display_name: &str,
    ) -> CalendarResult<DavCollection> {
        let href = normalize_collection_href(href);
        let current = self.propfind_collection(&href)?;
        if let Some(expected) = expected_ctag {
            if normalize_etag(&current.ctag) != normalize_etag(expected) {
                return Err(CalendarError::StaleEtag {
                    expected: expected.to_string(),
                    actual: current.ctag,
                });
            }
        }
        self.bump_collection(&href)?;
        let collection = self.collections.get_mut(&href).unwrap();
        collection.display_name = display_name.to_string();
        Ok(collection.clone())
    }

    fn delete_collection(&mut self, href: &str, expected_ctag: Option<&str>) -> CalendarResult<()> {
        let href = normalize_collection_href(href);
        let current = self.propfind_collection(&href)?;
        if let Some(expected) = expected_ctag {
            if normalize_etag(&current.ctag) != normalize_etag(expected) {
                return Err(CalendarError::StaleEtag {
                    expected: expected.to_string(),
                    actual: current.ctag,
                });
            }
        }
        if self
            .resources
            .keys()
            .any(|resource| resource.starts_with(&href))
        {
            return Err(CalendarError::Conflict(
                "A calendar can be deleted only when it has no live events.".into(),
            ));
        }
        self.collections.remove(&href);
        Ok(())
    }

    fn put(
        &mut self,
        href: &str,
        body: &str,
        if_match: Option<&str>,
        if_none_match_star: bool,
    ) -> CalendarResult<DavResource> {
        let href = href.to_string();
        Self::check_match(self.resources.get(&href), if_match, if_none_match_star)?;
        let etag = self.bump();
        let resource = DavResource {
            href: href.clone(),
            etag,
            body: body.to_string(),
            content_type: "text/calendar; charset=utf-8".into(),
        };
        self.resources.insert(href.clone(), resource.clone());
        if let Some(collection) = parent_collection(&href) {
            let _ = self.bump_collection(&collection);
        }
        Ok(resource)
    }

    fn get(&self, href: &str) -> CalendarResult<DavResource> {
        self.resources
            .get(href)
            .cloned()
            .ok_or_else(|| CalendarError::NotFound(format!("resource {href} not found")))
    }

    fn delete(&mut self, href: &str, if_match: Option<&str>) -> CalendarResult<()> {
        Self::check_match(self.resources.get(href), if_match, false)?;
        self.resources
            .remove(href)
            .ok_or_else(|| CalendarError::NotFound(format!("resource {href} not found")))?;
        if let Some(collection) = parent_collection(href) {
            let _ = self.bump_collection(&collection);
        }
        Ok(())
    }

    fn move_resource(
        &mut self,
        href: &str,
        destination: &str,
        if_match: Option<&str>,
    ) -> CalendarResult<DavResource> {
        Self::check_match(self.resources.get(href), if_match, false)?;
        if self.resources.contains_key(destination) {
            return Err(CalendarError::Conflict(format!(
                "A resource already exists at {destination}."
            )));
        }
        let mut resource = self
            .resources
            .remove(href)
            .ok_or_else(|| CalendarError::NotFound(format!("resource {href} not found")))?;
        resource.href = destination.to_string();
        resource.etag = self.bump();
        self.resources
            .insert(destination.to_string(), resource.clone());
        if let Some(collection) = parent_collection(href) {
            let _ = self.bump_collection(&collection);
        }
        if let Some(collection) = parent_collection(destination) {
            let _ = self.bump_collection(&collection);
        }
        Ok(resource)
    }

    fn list(&self, collection_href: &str) -> CalendarResult<Vec<DavResource>> {
        let href = normalize_collection_href(collection_href);
        if !self.collections.contains_key(&href) {
            return Err(CalendarError::NotFound(format!(
                "collection {href} not found"
            )));
        }
        Ok(self
            .resources
            .values()
            .filter(|resource| resource.href.starts_with(&href))
            .cloned()
            .collect())
    }
}

fn normalize_collection_href(href: &str) -> String {
    if href.ends_with('/') {
        href.to_string()
    } else {
        format!("{href}/")
    }
}

fn parent_collection(href: &str) -> Option<String> {
    let trimmed = href.trim_end_matches('/');
    trimmed
        .rfind('/')
        .map(|idx| format!("{}/", &trimmed[..=idx]))
}

// Projection + semantic commands
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredOperation {
    pub operation_id: String,
    pub user_id: String,
    pub entity_type: String,
    pub entity_id: String,
    pub operation: String,
    pub request_body: String,
    pub result_status: i32,
    pub result_kind: StoredResult,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StoredResult {
    Calendar(CalendarRecord),
    Event(EventRecord),
}

#[derive(Default)]
pub struct MemoryProjection {
    pub calendars: BTreeMap<String, CalendarRecord>,
    pub events: BTreeMap<String, EventRecord>,
    pub payloads: BTreeMap<String, String>,
    pub operations: BTreeMap<String, StoredOperation>,
    pub checkpoints: BTreeMap<(String, String), String>,
}

pub struct CalendarService<D> {
    pub user_id: String,
    pub dav: D,
    pub projection: MemoryProjection,
}

impl<D: DavStore> CalendarService<D> {
    pub fn new(user_id: impl Into<String>, dav: D) -> Self {
        Self {
            user_id: user_id.into(),
            dav,
            projection: MemoryProjection::default(),
        }
    }

    pub fn list_calendars(&self) -> Vec<CalendarRecord> {
        let mut rows: Vec<_> = self
            .projection
            .calendars
            .values()
            .filter(|row| row.user_id == self.user_id && row.deleted_at.is_none())
            .cloned()
            .collect();
        rows.sort_by(|a, b| a.display_name.cmp(&b.display_name).then(a.id.cmp(&b.id)));
        rows
    }

    pub fn list_events(&self, calendar_id: Option<&str>) -> Vec<EventRecord> {
        let mut rows: Vec<_> = self
            .projection
            .events
            .values()
            .filter(|row| {
                row.user_id == self.user_id
                    && row.deleted_at.is_none()
                    && calendar_id.is_none_or(|id| row.calendar_id == id)
            })
            .cloned()
            .collect();
        rows.sort_by(|a, b| a.dtstart.cmp(&b.dtstart).then(a.id.cmp(&b.id)));
        rows
    }

    pub fn expand_window(
        &self,
        calendar_id: Option<&str>,
        start: Date,
        end: Date,
        limit: usize,
    ) -> CalendarResult<Vec<Occurrence>> {
        let mut occurrences = Vec::new();
        for event in self.list_events(calendar_id) {
            occurrences.extend(expand_event(&event, start, end, limit)?);
        }
        occurrences.sort_by(|a, b| {
            a.start_local
                .cmp(&b.start_local)
                .then(a.event_id.cmp(&b.event_id))
        });
        occurrences.truncate(limit);
        Ok(occurrences)
    }

    pub fn create_calendar(&mut self, command: CreateCalendar) -> CalendarResult<CalendarRecord> {
        let operation_id = parse_uuid("operationId", &command.operation_id)?;
        let id = parse_uuid("id", &command.id)?;
        let name = validate_calendar_name(&command.display_name)?;
        let request = format!(
            "create_calendar\nid={id}\nname={name}\ndescription={}\ncolor={}",
            command.description,
            command.color.clone().unwrap_or_default()
        );
        if let Some(replay) = self.replay(&operation_id, "calendar", &id, "create", &request)? {
            return unwrap_calendar(replay);
        }
        if let Some(existing) = self.projection.calendars.get(&id) {
            return Err(existing_identity_error(&self.user_id, existing));
        }
        if self.list_calendars().len() >= MAX_CALENDARS_PER_USER {
            return Err(CalendarError::LimitExceeded(format!(
                "A user may have at most {MAX_CALENDARS_PER_USER} calendars."
            )));
        }
        let href = format!("/{}/{id}/", sanitize_path(&self.user_id));
        let collection = self.dav.mkcalendar(&href, &name)?;
        let now = format_rfc3339_utc(unix_now());
        let record = CalendarRecord {
            id: id.clone(),
            user_id: self.user_id.clone(),
            uid: id.clone(),
            href: collection.href,
            etag: collection.ctag.clone(),
            display_name: name,
            description: command.description,
            color: command.color,
            ctag: Some(collection.ctag),
            sync_token: Some(collection.sync_token),
            revision: 1,
            created_at: now.clone(),
            updated_at: now,
            deleted_at: None,
        };
        self.projection.calendars.insert(id.clone(), record.clone());
        self.store_operation(operation_id, "calendar", id, "create", request, &record);
        Ok(record)
    }

    pub fn rename_calendar(
        &mut self,
        calendar_id: &str,
        command: RenameCalendar,
    ) -> CalendarResult<CalendarRecord> {
        let operation_id = parse_uuid("operationId", &command.operation_id)?;
        let id = parse_uuid("calendarId", calendar_id)?;
        let name = validate_calendar_name(&command.display_name)?;
        let request = format!(
            "rename_calendar\nid={id}\nrevision={}\netag={}\nname={name}",
            command.expected_revision,
            command.expected_etag.clone().unwrap_or_default()
        );
        if let Some(replay) = self.replay(&operation_id, "calendar", &id, "rename", &request)? {
            return unwrap_calendar(replay);
        }
        let current = self.live_calendar(&id, command.expected_revision)?;
        let expected = command
            .expected_etag
            .as_deref()
            .unwrap_or(current.etag.as_str());
        let collection = self
            .dav
            .set_display_name(&current.href, Some(expected), &name)?;
        let now = format_rfc3339_utc(unix_now());
        let mut record = current;
        record.display_name = name;
        record.etag = collection.ctag.clone();
        record.ctag = Some(collection.ctag);
        record.sync_token = Some(collection.sync_token);
        record.revision += 1;
        record.updated_at = now;
        self.projection.calendars.insert(id.clone(), record.clone());
        self.store_operation(operation_id, "calendar", id, "rename", request, &record);
        Ok(record)
    }

    pub fn delete_calendar(
        &mut self,
        calendar_id: &str,
        command: DeleteCommand,
    ) -> CalendarResult<CalendarRecord> {
        let operation_id = parse_uuid("operationId", &command.operation_id)?;
        let id = parse_uuid("calendarId", calendar_id)?;
        let request = format!(
            "delete_calendar\nid={id}\nrevision={}\netag={}",
            command.expected_revision,
            command.expected_etag.clone().unwrap_or_default()
        );
        if let Some(replay) = self.replay(&operation_id, "calendar", &id, "delete", &request)? {
            return unwrap_calendar(replay);
        }
        let current = self.live_calendar(&id, command.expected_revision)?;
        if self
            .list_events(Some(&id))
            .iter()
            .any(|event| event.deleted_at.is_none())
        {
            return Err(CalendarError::Conflict(
                "A calendar can be deleted only when it has no live events.".into(),
            ));
        }
        let expected = command
            .expected_etag
            .as_deref()
            .unwrap_or(current.etag.as_str());
        self.dav.delete_collection(&current.href, Some(expected))?;
        let now = format_rfc3339_utc(unix_now());
        let mut record = current;
        record.revision += 1;
        record.updated_at = now.clone();
        record.deleted_at = Some(now);
        self.projection.calendars.insert(id.clone(), record.clone());
        self.store_operation(operation_id, "calendar", id, "delete", request, &record);
        Ok(record)
    }

    pub fn create_event(&mut self, command: CreateEvent) -> CalendarResult<EventRecord> {
        let operation_id = parse_uuid("operationId", &command.operation_id)?;
        let id = parse_uuid("id", &command.id)?;
        let calendar_id = parse_uuid("calendarId", &command.calendar_id)?;
        let uid = command
            .uid
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .unwrap_or(id.as_str())
            .to_string();
        reject_attendees(&command.draft)?;
        let request = format!(
            "create_event\nid={id}\ncalendar={calendar_id}\nuid={uid}\n{}",
            command.draft.fingerprint()
        );
        if let Some(replay) = self.replay(&operation_id, "event", &id, "create", &request)? {
            return unwrap_event(replay);
        }
        if let Some(existing) = self.projection.events.get(&id) {
            return Err(existing_event_identity(&self.user_id, existing));
        }
        if self.projection.events.values().any(|event| {
            event.user_id == self.user_id && event.uid == uid && event.deleted_at.is_none()
        }) {
            return Err(CalendarError::Conflict(
                "An event with this UID already exists.".into(),
            ));
        }
        let calendar = self.live_calendar(&calendar_id, calendar_revision_wildcard())?;
        if self.list_events(None).len() >= MAX_EVENTS_PER_USER {
            return Err(CalendarError::LimitExceeded(format!(
                "A user may have at most {MAX_EVENTS_PER_USER} events."
            )));
        }
        let href = format!("{}{}.ics", calendar.href, sanitize_path(&uid));
        let document = new_event_document(&uid, &command.draft, unix_now())?;
        let body = serialize_ical(&document);
        let resource = self.dav.put(&href, &body, None, true)?;
        let now = format_rfc3339_utc(unix_now());
        let record = event_from_ical(
            &id,
            &self.user_id,
            &calendar_id,
            &resource.href,
            &resource.etag,
            1,
            &now,
            &now,
            &resource.body,
        )?;
        self.projection.events.insert(id.clone(), record.clone());
        self.projection.payloads.insert(id.clone(), resource.body);
        self.store_event_operation(operation_id, id, "create", request, &record);
        Ok(record)
    }

    pub fn update_event(
        &mut self,
        event_id: &str,
        command: UpdateEvent,
    ) -> CalendarResult<EventRecord> {
        let operation_id = parse_uuid("operationId", &command.operation_id)?;
        let id = parse_uuid("eventId", event_id)?;
        reject_attendees(&command.draft)?;
        let request = format!(
            "update_event\nid={id}\nrevision={}\netag={}\n{}",
            command.expected_revision,
            command.expected_etag.clone().unwrap_or_default(),
            command.draft.fingerprint()
        );
        if let Some(replay) = self.replay(&operation_id, "event", &id, "update", &request)? {
            return unwrap_event(replay);
        }
        let current = self.live_event(&id, command.expected_revision)?;
        let expected = command
            .expected_etag
            .as_deref()
            .unwrap_or(current.etag.as_str());
        let payload = self.projection.payloads.get(&id).cloned().ok_or_else(|| {
            CalendarError::Dav(format!(
                "missing iCalendar payload for event {}",
                current.id
            ))
        })?;
        let mut document = parse_ical(&payload)?;
        patch_event_document(&mut document, &command.draft, unix_now())?;
        let body = serialize_ical(&document);
        let resource = self.dav.put(&current.href, &body, Some(expected), false)?;
        let now = format_rfc3339_utc(unix_now());
        let record = event_from_ical(
            &id,
            &self.user_id,
            &current.calendar_id,
            &resource.href,
            &resource.etag,
            current.revision + 1,
            &current.created_at,
            &now,
            &resource.body,
        )?;
        self.projection.events.insert(id.clone(), record.clone());
        self.projection.payloads.insert(id.clone(), resource.body);
        self.store_event_operation(operation_id, id, "update", request, &record);
        Ok(record)
    }

    pub fn move_event(
        &mut self,
        event_id: &str,
        command: MoveEvent,
    ) -> CalendarResult<EventRecord> {
        let operation_id = parse_uuid("operationId", &command.operation_id)?;
        let id = parse_uuid("eventId", event_id)?;
        let calendar_id = parse_uuid("calendarId", &command.calendar_id)?;
        let request = format!(
            "move_event\nid={id}\nrevision={}\netag={}\ncalendar={calendar_id}",
            command.expected_revision,
            command.expected_etag.clone().unwrap_or_default()
        );
        if let Some(replay) = self.replay(&operation_id, "event", &id, "move", &request)? {
            return unwrap_event(replay);
        }
        let current = self.live_event(&id, command.expected_revision)?;
        let calendar = self.live_calendar(&calendar_id, calendar_revision_wildcard())?;
        let expected = command
            .expected_etag
            .as_deref()
            .unwrap_or(current.etag.as_str());
        let destination = format!("{}{}.ics", calendar.href, sanitize_path(&current.uid));
        let resource = self
            .dav
            .move_resource(&current.href, &destination, Some(expected))?;
        let now = format_rfc3339_utc(unix_now());
        let mut record = current;
        record.calendar_id = calendar_id;
        record.href = resource.href;
        record.etag = resource.etag;
        record.revision += 1;
        record.updated_at = now;
        self.projection.events.insert(id.clone(), record.clone());
        self.store_event_operation(operation_id, id, "move", request, &record);
        Ok(record)
    }

    pub fn delete_event(
        &mut self,
        event_id: &str,
        command: DeleteCommand,
    ) -> CalendarResult<EventRecord> {
        let operation_id = parse_uuid("operationId", &command.operation_id)?;
        let id = parse_uuid("eventId", event_id)?;
        let request = format!(
            "delete_event\nid={id}\nrevision={}\netag={}",
            command.expected_revision,
            command.expected_etag.clone().unwrap_or_default()
        );
        if let Some(replay) = self.replay(&operation_id, "event", &id, "delete", &request)? {
            return unwrap_event(replay);
        }
        let current = self.live_event(&id, command.expected_revision)?;
        let expected = command
            .expected_etag
            .as_deref()
            .unwrap_or(current.etag.as_str());
        self.dav.delete(&current.href, Some(expected))?;
        let now = format_rfc3339_utc(unix_now());
        let mut record = current;
        record.revision += 1;
        record.updated_at = now.clone();
        record.deleted_at = Some(now);
        self.projection.events.insert(id.clone(), record.clone());
        self.store_event_operation(operation_id, id, "delete", request, &record);
        Ok(record)
    }

    pub fn rebuild_from_dav(&mut self) -> CalendarResult<()> {
        let calendars: Vec<_> = self
            .projection
            .calendars
            .values()
            .filter(|row| row.user_id == self.user_id && row.deleted_at.is_none())
            .cloned()
            .collect();
        for calendar in calendars {
            let resources = match self.dav.list(&calendar.href) {
                Ok(rows) => rows,
                Err(CalendarError::NotFound(_)) => continue,
                Err(error) => return Err(error),
            };
            let seen: BTreeSet<String> = resources.iter().map(|r| r.href.clone()).collect();
            for resource in resources {
                let now = format_rfc3339_utc(unix_now());
                match event_from_ical(
                    &stable_event_id(&self.user_id, &resource.href),
                    &self.user_id,
                    &calendar.id,
                    &resource.href,
                    &resource.etag,
                    1,
                    &now,
                    &now,
                    &resource.body,
                ) {
                    Ok(mut record) => {
                        if let Some(existing) = self
                            .projection
                            .events
                            .values()
                            .find(|event| event.href == record.href)
                            .cloned()
                        {
                            record.id = existing.id;
                            record.revision = existing.revision + 1;
                            record.created_at = existing.created_at;
                        }
                        self.projection
                            .payloads
                            .insert(record.id.clone(), resource.body);
                        self.projection.events.insert(record.id.clone(), record);
                    }
                    Err(CalendarError::Parse(_)) => {
                        // Skip a single malformed remote resource; keep projecting others.
                    }
                    Err(error) => return Err(error),
                }
            }
            for event in self.projection.events.values_mut() {
                if event.calendar_id == calendar.id
                    && event.deleted_at.is_none()
                    && !seen.contains(&event.href)
                {
                    let now = format_rfc3339_utc(unix_now());
                    event.deleted_at = Some(now.clone());
                    event.updated_at = now;
                    event.revision += 1;
                }
            }
            self.projection.checkpoints.insert(
                (self.user_id.clone(), calendar.href),
                calendar.sync_token.unwrap_or_default(),
            );
        }
        Ok(())
    }

    fn live_calendar(&self, id: &str, expected_revision: i64) -> CalendarResult<CalendarRecord> {
        match self.projection.calendars.get(id) {
            Some(row) if row.user_id != self.user_id => {
                Err(CalendarError::NotFound("Calendar not found.".into()))
            }
            Some(row) if row.deleted_at.is_some() => Err(CalendarError::Gone(
                "This calendar has been deleted.".into(),
            )),
            Some(row) if expected_revision != i64::MIN && row.revision != expected_revision => {
                Err(CalendarError::StaleRevision {
                    expected: expected_revision,
                    actual: row.revision,
                })
            }
            Some(row) => Ok(row.clone()),
            None => Err(CalendarError::NotFound("Calendar not found.".into())),
        }
    }

    fn live_event(&self, id: &str, expected_revision: i64) -> CalendarResult<EventRecord> {
        match self.projection.events.get(id) {
            Some(row) if row.user_id != self.user_id => {
                Err(CalendarError::NotFound("Event not found.".into()))
            }
            Some(row) if row.deleted_at.is_some() => {
                Err(CalendarError::Gone("This event has been deleted.".into()))
            }
            Some(row) if row.revision != expected_revision => Err(CalendarError::StaleRevision {
                expected: expected_revision,
                actual: row.revision,
            }),
            Some(row) => Ok(row.clone()),
            None => Err(CalendarError::NotFound("Event not found.".into())),
        }
    }

    fn replay(
        &self,
        operation_id: &str,
        entity_type: &str,
        entity_id: &str,
        operation: &str,
        request: &str,
    ) -> CalendarResult<Option<StoredResult>> {
        let Some(stored) = self.projection.operations.get(operation_id) else {
            return Ok(None);
        };
        if stored.user_id != self.user_id
            || stored.entity_type != entity_type
            || stored.entity_id != entity_id
            || stored.operation != operation
            || stored.request_body != request
        {
            return Err(CalendarError::Conflict(
                "This operation id is already bound to a different request.".into(),
            ));
        }
        if stored.result_status != 200 {
            return Err(CalendarError::Conflict(
                "This operation id already produced a non-success result.".into(),
            ));
        }
        Ok(Some(stored.result_kind.clone()))
    }

    fn store_operation(
        &mut self,
        operation_id: String,
        entity_type: &str,
        entity_id: String,
        operation: &str,
        request: String,
        record: &CalendarRecord,
    ) {
        self.projection.operations.insert(
            operation_id.clone(),
            StoredOperation {
                operation_id,
                user_id: self.user_id.clone(),
                entity_type: entity_type.to_string(),
                entity_id,
                operation: operation.to_string(),
                request_body: request,
                result_status: 200,
                result_kind: StoredResult::Calendar(record.clone()),
            },
        );
    }

    fn store_event_operation(
        &mut self,
        operation_id: String,
        entity_id: String,
        operation: &str,
        request: String,
        record: &EventRecord,
    ) {
        self.projection.operations.insert(
            operation_id.clone(),
            StoredOperation {
                operation_id,
                user_id: self.user_id.clone(),
                entity_type: "event".into(),
                entity_id,
                operation: operation.to_string(),
                request_body: request,
                result_status: 200,
                result_kind: StoredResult::Event(record.clone()),
            },
        );
    }
}

fn calendar_revision_wildcard() -> i64 {
    i64::MIN
}

fn unwrap_calendar(result: StoredResult) -> CalendarResult<CalendarRecord> {
    match result {
        StoredResult::Calendar(row) => Ok(row),
        StoredResult::Event(_) => Err(CalendarError::Conflict(
            "This operation id is already bound to a different request.".into(),
        )),
    }
}

fn unwrap_event(result: StoredResult) -> CalendarResult<EventRecord> {
    match result {
        StoredResult::Event(row) => Ok(row),
        StoredResult::Calendar(_) => Err(CalendarError::Conflict(
            "This operation id is already bound to a different request.".into(),
        )),
    }
}

fn existing_identity_error(user_id: &str, existing: &CalendarRecord) -> CalendarError {
    if existing.user_id != user_id {
        CalendarError::Conflict("This identifier is already in use.".into())
    } else if existing.deleted_at.is_some() {
        CalendarError::Gone("A tombstoned calendar cannot be resurrected.".into())
    } else {
        CalendarError::Conflict("This identifier is already in use.".into())
    }
}

fn existing_event_identity(user_id: &str, existing: &EventRecord) -> CalendarError {
    if existing.user_id != user_id {
        CalendarError::Conflict("This identifier is already in use.".into())
    } else if existing.deleted_at.is_some() {
        CalendarError::Gone("A tombstoned event cannot be resurrected.".into())
    } else {
        CalendarError::Conflict("This identifier is already in use.".into())
    }
}

fn validate_calendar_name(name: &str) -> CalendarResult<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(CalendarError::InvalidRequest(
            "Calendar name is required.".into(),
        ));
    }
    if trimmed.chars().count() > 80 {
        return Err(CalendarError::LimitExceeded(
            "Calendar name may be at most 80 characters.".into(),
        ));
    }
    Ok(trimmed.to_string())
}

fn sanitize_path(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .collect()
}

fn reject_attendees(draft: &EventDraft) -> CalendarResult<()> {
    let haystack = format!(
        "{}\n{}\n{}",
        draft.summary, draft.description, draft.location
    );
    if haystack.to_ascii_uppercase().contains("ATTENDEE:") {
        return Err(CalendarError::InvalidRequest(
            "Attendee scheduling is not supported.".into(),
        ));
    }
    Ok(())
}

fn stable_event_id(user_id: &str, href: &str) -> String {
    let mut hash: u128 = 0x6d2b79f5a0c14e33;
    for byte in user_id.bytes().chain(href.bytes()) {
        hash = hash
            .wrapping_mul(0x100_0000_01b3)
            .wrapping_add(byte as u128);
    }
    format!(
        "{:08x}-{:04x}-4{:03x}-a{:03x}-{:012x}",
        (hash >> 96) as u32,
        (hash >> 80) as u16,
        ((hash >> 64) as u16) & 0x0fff,
        ((hash >> 48) as u16) & 0x0fff,
        hash & 0xffff_ffff_ffff
    )
}

/// Shared PowerSync replica columns. Client-only queue fields are local and must
/// not appear in sync streams or canonical PostgreSQL rows.
pub const POWERSYNC_CALENDAR_COLUMNS: &[&str] = &[
    "id",
    "user_id",
    "uid",
    "href",
    "etag",
    "display_name",
    "description",
    "color",
    "ctag",
    "sync_token",
    "revision",
    "created_at",
    "updated_at",
];

pub const POWERSYNC_EVENT_COLUMNS: &[&str] = &[
    "id",
    "user_id",
    "calendar_id",
    "uid",
    "href",
    "etag",
    "summary",
    "description",
    "location",
    "all_day",
    "dtstart",
    "dtend",
    "tzid",
    "rrule",
    "exdates",
    "revision",
    "created_at",
    "updated_at",
];

pub const CLIENT_ONLY_COLUMNS: &[&str] = &[
    "client_operation",
    "operation_id",
    "expected_revision",
    "expected_etag",
    "deleted_local",
    "client_payload",
];
