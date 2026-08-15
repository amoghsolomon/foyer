//! Bounded recurrence expansion from normalized event rows.

use std::collections::BTreeSet;

use chrono::{
    Datelike, Duration, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Timelike, Weekday,
};
use chrono_tz::Tz;

use crate::{Event, Occurrence};

pub const MAX_EXPANSION_INSTANCES: usize = 512;
pub const MAX_WINDOW_DAYS: i64 = 366 * 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Freq {
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ByDay {
    pub weekday: Weekday,
    pub nth: Option<i32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecurrenceRule {
    pub freq: Freq,
    pub interval: u32,
    pub count: Option<u32>,
    pub until: Option<NaiveDate>,
    pub by_day: Vec<ByDay>,
    pub by_month_day: Vec<i32>,
    pub by_month: Vec<u32>,
    pub by_set_pos: Vec<i32>,
    pub week_start: Weekday,
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
            parts.push(format!("UNTIL={}", until.format("%Y%m%d")));
        }
        if !self.by_day.is_empty() {
            let days = self
                .by_day
                .iter()
                .map(|day| match day.nth {
                    Some(n) => format!("{n}{}", weekday_ical(day.weekday)),
                    None => weekday_ical(day.weekday).to_string(),
                })
                .collect::<Vec<_>>()
                .join(",");
            parts.push(format!("BYDAY={days}"));
        }
        parts.join(";")
    }
}

pub fn parse_rrule(value: &str) -> Result<RecurrenceRule, String> {
    let mut freq = None;
    let mut interval = 1u32;
    let mut count = None;
    let mut until = None;
    let mut by_day = Vec::new();
    let mut by_month_day = Vec::new();
    let mut by_month = Vec::new();
    let mut by_set_pos = Vec::new();
    let mut week_start = Weekday::Mon;
    for part in value.split(';').filter(|part| !part.is_empty()) {
        let (key, raw) = part
            .split_once('=')
            .ok_or_else(|| format!("invalid RRULE part {part}"))?;
        match key.to_ascii_uppercase().as_str() {
            "FREQ" => {
                freq = Some(match raw {
                    "DAILY" => Freq::Daily,
                    "WEEKLY" => Freq::Weekly,
                    "MONTHLY" => Freq::Monthly,
                    "YEARLY" => Freq::Yearly,
                    other => return Err(format!("unsupported FREQ {other}")),
                });
            }
            "INTERVAL" => {
                interval = raw.parse().map_err(|_| format!("invalid INTERVAL {raw}"))?;
                if interval == 0 {
                    return Err("INTERVAL must be at least 1".into());
                }
            }
            "COUNT" => count = Some(raw.parse().map_err(|_| format!("invalid COUNT {raw}"))?),
            "UNTIL" => {
                until = Some(parse_date_token(raw)?);
            }
            "BYDAY" => {
                for token in raw.split(',') {
                    by_day.push(parse_byday(token)?);
                }
            }
            "BYMONTHDAY" => {
                for token in raw.split(',') {
                    by_month_day.push(
                        token
                            .parse()
                            .map_err(|_| format!("invalid BYMONTHDAY {token}"))?,
                    );
                }
            }
            "BYMONTH" => {
                for token in raw.split(',') {
                    by_month.push(
                        token
                            .parse()
                            .map_err(|_| format!("invalid BYMONTH {token}"))?,
                    );
                }
            }
            "BYSETPOS" => {
                for token in raw.split(',') {
                    by_set_pos.push(
                        token
                            .parse()
                            .map_err(|_| format!("invalid BYSETPOS {token}"))?,
                    );
                }
            }
            "WKST" => week_start = parse_weekday(raw)?,
            other => return Err(format!("unsupported RRULE part {other}")),
        }
    }
    Ok(RecurrenceRule {
        freq: freq.ok_or_else(|| "RRULE requires FREQ".to_string())?,
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

pub fn parse_exdates(raw: &str) -> Vec<String> {
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

pub fn recurrence_summary(rrule: Option<&str>) -> String {
    let Some(raw) = rrule.filter(|value| !value.is_empty()) else {
        return "Does not repeat".into();
    };
    let Ok(rule) = parse_rrule(raw) else {
        return raw.to_string();
    };
    let unit = match (rule.freq, rule.interval) {
        (Freq::Daily, 1) => "Daily".into(),
        (Freq::Weekly, 1) => "Weekly".into(),
        (Freq::Monthly, 1) => "Monthly".into(),
        (Freq::Yearly, 1) => "Yearly".into(),
        (Freq::Daily, n) => format!("every {n} days"),
        (Freq::Weekly, n) => format!("every {n} weeks"),
        (Freq::Monthly, n) => format!("every {n} months"),
        (Freq::Yearly, n) => format!("every {n} years"),
    };
    let days = if rule.by_day.is_empty() {
        String::new()
    } else {
        format!(
            " on {}",
            rule.by_day
                .iter()
                .map(|day| weekday_name(day.weekday))
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    format!("{unit}{days}")
}

pub fn expand_event(
    event: &Event,
    window_start: NaiveDate,
    window_end: NaiveDate,
    limit: usize,
) -> Result<Vec<Occurrence>, String> {
    if window_end < window_start {
        return Err("expansion window end must not precede start".into());
    }
    if (window_end - window_start).num_days() > MAX_WINDOW_DAYS {
        return Err(format!(
            "expansion window may be at most {MAX_WINDOW_DAYS} days"
        ));
    }
    let cap = limit.clamp(1, MAX_EXPANSION_INSTANCES);
    let seed = parse_start(event)?;
    let duration = event_duration(event, seed)?;
    let excluded = parse_exdates(&event.exdates)
        .into_iter()
        .map(|raw| parse_date_token(&raw))
        .collect::<Result<BTreeSet<_>, _>>()?;
    let dates = if event.rrule.as_deref().is_none_or(str::is_empty) {
        if seed.date >= window_start && seed.date <= window_end {
            vec![seed.date]
        } else {
            Vec::new()
        }
    } else {
        expand_rule(
            &parse_rrule(event.rrule.as_deref().unwrap())?,
            seed.date,
            window_start,
            window_end,
            cap * 4,
        )?
    };
    let mut out = Vec::new();
    for date in dates {
        if excluded.contains(&date) || date < window_start || date > window_end {
            continue;
        }
        if let Some(item) = occurrence(event, seed, date, duration)? {
            out.push(item);
        }
        if out.len() == cap {
            break;
        }
    }
    Ok(out)
}

#[derive(Clone, Copy)]
struct Seed {
    date: NaiveDate,
    time: NaiveTime,
}

fn parse_start(event: &Event) -> Result<Seed, String> {
    if event.all_day || event.dtstart.len() == 8 {
        return Ok(Seed {
            date: parse_date_token(&event.dtstart)?,
            time: NaiveTime::from_hms_opt(0, 0, 0).unwrap(),
        });
    }
    let dt = parse_local_datetime(&event.dtstart)?;
    Ok(Seed {
        date: dt.date(),
        time: dt.time(),
    })
}

fn event_duration(event: &Event, seed: Seed) -> Result<Duration, String> {
    let Some(end) = event.dtend.as_deref().filter(|value| !value.is_empty()) else {
        return Ok(if event.all_day {
            Duration::days(1)
        } else {
            Duration::hours(1)
        });
    };
    if event.all_day {
        return Ok(parse_date_token(end)? - seed.date);
    }
    Ok(parse_local_datetime(end)? - NaiveDateTime::new(seed.date, seed.time))
}

fn occurrence(
    event: &Event,
    seed: Seed,
    date: NaiveDate,
    duration: Duration,
) -> Result<Option<Occurrence>, String> {
    let local = NaiveDateTime::new(date, seed.time);
    let (start_ms, start_local) = if event.all_day {
        (None, date.to_string())
    } else {
        let tz = zone(event.tzid.as_deref())?;
        let Some(zoned) = tz.from_local_datetime(&local).earliest() else {
            return Ok(None);
        };
        (
            Some(zoned.timestamp_millis()),
            local.format("%Y%m%dT%H%M%S").to_string(),
        )
    };
    let end_ms = start_ms.map(|start| start + duration.num_milliseconds());
    let end_local = if event.all_day {
        event.dtend.clone()
    } else {
        end_ms.and_then(|ms| {
            let tz = zone(event.tzid.as_deref()).unwrap_or(chrono_tz::UTC);
            tz.timestamp_millis_opt(ms)
                .single()
                .map(|dt| dt.naive_local().format("%Y%m%dT%H%M%S").to_string())
        })
    };
    Ok(Some(Occurrence {
        event_id: event.id.clone(),
        calendar_id: event.calendar_id.clone(),
        uid: event.uid.clone(),
        summary: event.summary.clone(),
        description: event.description.clone(),
        location: event.location.clone(),
        all_day: event.all_day,
        tzid: event.tzid.clone(),
        start_ms,
        end_ms,
        start_local,
        end_local,
        recurrence_id: if event.all_day {
            date.format("%Y%m%d").to_string()
        } else {
            local.format("%Y%m%dT%H%M%S").to_string()
        },
        is_recurring: event
            .rrule
            .as_deref()
            .is_some_and(|value| !value.is_empty()),
    }))
}

pub fn zone(tzid: Option<&str>) -> Result<Tz, String> {
    match tzid {
        None | Some("") | Some("UTC") | Some("Z") | Some("Etc/UTC") => Ok(chrono_tz::UTC),
        Some(name) => name
            .parse()
            .map_err(|_| format!("unsupported IANA TZID {name}")),
    }
}

fn expand_rule(
    rule: &RecurrenceRule,
    seed: NaiveDate,
    window_start: NaiveDate,
    window_end: NaiveDate,
    safety: usize,
) -> Result<Vec<NaiveDate>, String> {
    let mut dates = Vec::new();
    let mut emitted = 0u32;
    let mut cursor = seed;
    let mut iterations = 0usize;
    let hard = (safety.max(16) * 8).max(64);
    while iterations < hard && dates.len() < safety {
        iterations += 1;
        let mut set = period_candidates(rule, seed, cursor);
        if !rule.by_month.is_empty() {
            set.retain(|date| rule.by_month.contains(&date.month()));
        }
        if !rule.by_month_day.is_empty() {
            set.retain(|date| matches_month_day(*date, &rule.by_month_day));
        }
        if !rule.by_day.is_empty() && rule.freq != Freq::Weekly {
            set.retain(|date| matches_byday(*date, &rule.by_day));
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
            if let Some(until) = rule.until
                && date > until
            {
                return Ok(dates);
            }
            emitted += 1;
            if let Some(count) = rule.count
                && emitted > count
            {
                return Ok(dates);
            }
            if date >= window_start && date <= window_end {
                dates.push(date);
            }
        }
        let Some(next) = advance(rule, cursor) else {
            break;
        };
        cursor = next;
        if cursor > window_end + Duration::days(400) {
            break;
        }
    }
    Ok(dates)
}

fn period_candidates(rule: &RecurrenceRule, seed: NaiveDate, cursor: NaiveDate) -> Vec<NaiveDate> {
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
            if rule.by_day.iter().any(|day| day.nth.is_some()) {
                rule.by_day
                    .iter()
                    .filter_map(|day| nth_weekday(cursor.year(), cursor.month(), day))
                    .collect()
            } else if !rule.by_month_day.is_empty() {
                rule.by_month_day
                    .iter()
                    .filter_map(|day| month_day(cursor.year(), cursor.month(), *day))
                    .collect()
            } else {
                cursor
                    .with_day(seed.day())
                    .map(|date| vec![date])
                    .unwrap_or_default()
            }
        }
        Freq::Yearly => {
            if !rule.by_month.is_empty() {
                rule.by_month
                    .iter()
                    .filter_map(|month| NaiveDate::from_ymd_opt(cursor.year(), *month, seed.day()))
                    .collect()
            } else {
                seed.with_year(cursor.year())
                    .map(|date| vec![date])
                    .unwrap_or_default()
            }
        }
    }
}

fn weekly_byday(cursor: NaiveDate, seed: NaiveDate, rule: &RecurrenceRule) -> Vec<NaiveDate> {
    let week_start = week_start_date(cursor, rule.week_start);
    let seed_week = week_start_date(seed, rule.week_start);
    let weeks = (week_start - seed_week).num_days() / 7;
    if weeks.rem_euclid(rule.interval as i64) != 0 {
        return Vec::new();
    }
    rule.by_day
        .iter()
        .map(|day| {
            let delta = (weekday_index(day.weekday) - weekday_index(rule.week_start)).rem_euclid(7);
            week_start + Duration::days(delta)
        })
        .collect()
}

fn week_start_date(date: NaiveDate, week_start: Weekday) -> NaiveDate {
    let delta = (weekday_index(date.weekday()) - weekday_index(week_start)).rem_euclid(7);
    date - Duration::days(delta)
}

fn matches_month_day(date: NaiveDate, days: &[i32]) -> bool {
    let last = date
        .with_day(1)
        .and_then(|d| d.with_month(d.month() + 1))
        .map(|d| (d - Duration::days(1)).day() as i32)
        .unwrap_or_else(|| last_day(date.year(), date.month()) as i32);
    days.iter()
        .any(|day| date.day() as i32 == if *day > 0 { *day } else { last + *day + 1 })
}

fn matches_byday(date: NaiveDate, days: &[ByDay]) -> bool {
    days.iter().any(|spec| {
        date.weekday() == spec.weekday
            && spec
                .nth
                .is_none_or(|_| nth_weekday(date.year(), date.month(), spec) == Some(date))
    })
}

fn nth_weekday(year: i32, month: u32, spec: &ByDay) -> Option<NaiveDate> {
    let nth = spec.nth?;
    if nth == 0 {
        return None;
    }
    if nth > 0 {
        let first = NaiveDate::from_ymd_opt(year, month, 1)?;
        let delta = (weekday_index(spec.weekday) - weekday_index(first.weekday())).rem_euclid(7);
        first
            .checked_add_signed(Duration::days(delta + (nth as i64 - 1) * 7))
            .filter(|date| date.month() == month)
    } else {
        let last = NaiveDate::from_ymd_opt(year, month, last_day(year, month))?;
        let delta = (weekday_index(last.weekday()) - weekday_index(spec.weekday)).rem_euclid(7);
        last.checked_sub_signed(Duration::days(delta - (nth as i64 + 1) * 7))
            .filter(|date| date.month() == month)
    }
}

fn month_day(year: i32, month: u32, day: i32) -> Option<NaiveDate> {
    let last = last_day(year, month) as i32;
    let resolved = if day > 0 { day } else { last + day + 1 };
    NaiveDate::from_ymd_opt(year, month, resolved as u32)
}

fn last_day(year: i32, month: u32) -> u32 {
    NaiveDate::from_ymd_opt(year, month, 1)
        .and_then(|_| {
            if month == 12 {
                NaiveDate::from_ymd_opt(year + 1, 1, 1)
            } else {
                NaiveDate::from_ymd_opt(year, month + 1, 1)
            }
            .map(|next| (next - Duration::days(1)).day())
        })
        .unwrap_or(28)
}

fn apply_set_pos(dates: &[NaiveDate], positions: &[i32]) -> Vec<NaiveDate> {
    let len = dates.len() as i32;
    positions
        .iter()
        .filter_map(|pos| {
            let index = if *pos > 0 { *pos - 1 } else { len + *pos };
            dates.get(index as usize).copied()
        })
        .collect()
}

fn advance(rule: &RecurrenceRule, cursor: NaiveDate) -> Option<NaiveDate> {
    match rule.freq {
        Freq::Daily => Some(cursor + Duration::days(rule.interval as i64)),
        Freq::Weekly => Some(cursor + Duration::weeks(rule.interval as i64)),
        Freq::Monthly => add_months(cursor, rule.interval as i32),
        Freq::Yearly => add_months(cursor, rule.interval as i32 * 12),
    }
}

fn add_months(date: NaiveDate, months: i32) -> Option<NaiveDate> {
    let total = date.year() * 12 + date.month() as i32 - 1 + months;
    let year = total.div_euclid(12);
    let month = (total.rem_euclid(12) + 1) as u32;
    date.with_year(year)?
        .with_month(month)?
        .with_day(date.day())
}

fn parse_byday(token: &str) -> Result<ByDay, String> {
    if token.len() < 2 {
        return Err(format!("invalid BYDAY {token}"));
    }
    let day = parse_weekday(&token[token.len() - 2..])?;
    let nth = if token.len() == 2 {
        None
    } else {
        Some(
            token[..token.len() - 2]
                .parse()
                .map_err(|_| format!("invalid BYDAY {token}"))?,
        )
    };
    if matches!(nth, Some(0)) {
        return Err("BYDAY occurrence cannot be 0".into());
    }
    Ok(ByDay { weekday: day, nth })
}

fn parse_weekday(token: &str) -> Result<Weekday, String> {
    match token {
        "MO" => Ok(Weekday::Mon),
        "TU" => Ok(Weekday::Tue),
        "WE" => Ok(Weekday::Wed),
        "TH" => Ok(Weekday::Thu),
        "FR" => Ok(Weekday::Fri),
        "SA" => Ok(Weekday::Sat),
        "SU" => Ok(Weekday::Sun),
        other => Err(format!("unknown weekday {other}")),
    }
}

fn weekday_ical(day: Weekday) -> &'static str {
    match day {
        Weekday::Mon => "MO",
        Weekday::Tue => "TU",
        Weekday::Wed => "WE",
        Weekday::Thu => "TH",
        Weekday::Fri => "FR",
        Weekday::Sat => "SA",
        Weekday::Sun => "SU",
    }
}

fn weekday_name(day: Weekday) -> &'static str {
    match day {
        Weekday::Mon => "Monday",
        Weekday::Tue => "Tuesday",
        Weekday::Wed => "Wednesday",
        Weekday::Thu => "Thursday",
        Weekday::Fri => "Friday",
        Weekday::Sat => "Saturday",
        Weekday::Sun => "Sunday",
    }
}

fn weekday_index(day: Weekday) -> i64 {
    day.num_days_from_monday() as i64
}

fn parse_date_token(raw: &str) -> Result<NaiveDate, String> {
    let digits: String = raw.chars().filter(|c| c.is_ascii_digit()).take(8).collect();
    NaiveDate::parse_from_str(&digits, "%Y%m%d").map_err(|_| format!("invalid date {raw}"))
}

fn parse_local_datetime(raw: &str) -> Result<NaiveDateTime, String> {
    let compact: String = raw
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == 'T' || *c == 'Z')
        .collect();
    if compact.ends_with('Z') {
        NaiveDateTime::parse_from_str(&compact, "%Y%m%dT%H%M%SZ")
            .map_err(|_| format!("invalid date-time {raw}"))
    } else if compact.contains('T') {
        NaiveDateTime::parse_from_str(&compact[..15.min(compact.len())], "%Y%m%dT%H%M%S")
            .map_err(|_| format!("invalid date-time {raw}"))
    } else {
        Ok(parse_date_token(&compact)?.and_hms_opt(0, 0, 0).unwrap())
    }
}

pub fn format_stamp(date: NaiveDate, time: Option<NaiveTime>, all_day: bool) -> String {
    if all_day {
        date.format("%Y%m%d").to_string()
    } else {
        NaiveDateTime::new(
            date,
            time.unwrap_or(NaiveTime::from_hms_opt(0, 0, 0).unwrap()),
        )
        .format("%Y%m%dT%H%M%S")
        .to_string()
    }
}

pub fn format_when(event: &Event) -> String {
    let date = parse_date_token(&event.dtstart)
        .map(|d| d.to_string())
        .unwrap_or_else(|_| event.dtstart.clone());
    if event.all_day {
        format!("{date} · All day")
    } else {
        let time = parse_local_datetime(&event.dtstart)
            .map(|dt| format!("{:02}:{:02}", dt.hour(), dt.minute()))
            .unwrap_or_default();
        let zone = event
            .tzid
            .as_deref()
            .filter(|tz| !tz.is_empty() && *tz != "UTC")
            .unwrap_or("UTC");
        format!("{date} · {time} · {zone}")
    }
}
