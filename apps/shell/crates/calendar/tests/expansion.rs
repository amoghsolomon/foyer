use chrono::{NaiveDate, TimeZone};
use chrono_tz::America::New_York;
use foyer_shell_calendar::{Event, encode_exdates, expand_event, parse_rrule, recurrence_summary};

fn sample(
    all_day: bool,
    dtstart: &str,
    dtend: Option<&str>,
    tzid: Option<&str>,
    rrule: Option<&str>,
    exdates: &str,
) -> Event {
    Event {
        id: "00000000-0000-4000-a000-000000000003".into(),
        calendar_id: "00000000-0000-4000-a000-000000000002".into(),
        uid: "sample".into(),
        href: "/cal/sample.ics".into(),
        etag: "\"1\"".into(),
        summary: "Standup".into(),
        description: String::new(),
        location: String::new(),
        all_day,
        dtstart: dtstart.into(),
        dtend: dtend.map(ToString::to_string),
        tzid: tzid.map(ToString::to_string),
        rrule: rrule.map(ToString::to_string),
        exdates: exdates.into(),
        revision: 1,
    }
}

#[test]
fn weekly_rrule_skips_exdate() {
    let event = sample(
        false,
        "20260302T100000",
        Some("20260302T103000"),
        Some("America/New_York"),
        Some("FREQ=WEEKLY;BYDAY=MO"),
        &encode_exdates(&["20260309T100000".into()]),
    );
    let days = expand_event(
        &event,
        NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
        NaiveDate::from_ymd_opt(2026, 3, 31).unwrap(),
        20,
    )
    .unwrap()
    .into_iter()
    .map(|item| item.recurrence_id[..8].to_string())
    .collect::<Vec<_>>();
    assert_eq!(days, vec!["20260302", "20260316", "20260323", "20260330"]);
}

#[test]
fn all_day_values_do_not_invent_a_timezone() {
    let event = sample(true, "20260315", Some("20260316"), None, None, "[]");
    let items = expand_event(
        &event,
        NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
        NaiveDate::from_ymd_opt(2026, 3, 31).unwrap(),
        8,
    )
    .unwrap();
    assert_eq!(items.len(), 1);
    assert!(items[0].all_day);
    assert_eq!(items[0].recurrence_id, "20260315");
    assert!(items[0].start_ms.is_none());
}

#[test]
fn tzid_keeps_local_wall_time_across_dst() {
    let event = sample(
        false,
        "20260302T100000",
        Some("20260302T110000"),
        Some("America/New_York"),
        Some("FREQ=WEEKLY;BYDAY=MO"),
        "[]",
    );
    let items = expand_event(
        &event,
        NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
        NaiveDate::from_ymd_opt(2026, 3, 15).unwrap(),
        8,
    )
    .unwrap();
    assert_eq!(items.len(), 2);
    let before = New_York
        .with_ymd_and_hms(2026, 3, 2, 10, 0, 0)
        .unwrap()
        .timestamp_millis();
    let after = New_York
        .with_ymd_and_hms(2026, 3, 9, 10, 0, 0)
        .unwrap()
        .timestamp_millis();
    assert_eq!(items[0].start_ms, Some(before));
    assert_eq!(items[1].start_ms, Some(after));
    assert_eq!(after - before, 7 * 86_400_000 - 3_600_000);
}

#[test]
fn fall_back_uses_the_first_offset() {
    let first = New_York
        .from_local_datetime(
            &chrono::NaiveDateTime::parse_from_str("20261101T013000", "%Y%m%dT%H%M%S").unwrap(),
        )
        .earliest()
        .unwrap();
    let later = New_York
        .from_local_datetime(
            &chrono::NaiveDateTime::parse_from_str("20261101T013000", "%Y%m%dT%H%M%S").unwrap(),
        )
        .latest()
        .unwrap();
    assert!(first.timestamp() < later.timestamp());
}

#[test]
fn bounded_daily_expansion_stays_inside_the_window() {
    let event = sample(
        true,
        "20200101",
        Some("20200102"),
        None,
        Some("FREQ=DAILY"),
        "[]",
    );
    let items = expand_event(
        &event,
        NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
        NaiveDate::from_ymd_opt(2026, 3, 7).unwrap(),
        512,
    )
    .unwrap();
    assert_eq!(items.len(), 7);
    assert_eq!(items.first().unwrap().recurrence_id, "20260301");
    assert_eq!(items.last().unwrap().recurrence_id, "20260307");
}

#[test]
fn monthly_nth_weekday_honors_count() {
    let event = sample(
        true,
        "20260310",
        Some("20260311"),
        None,
        Some("FREQ=MONTHLY;COUNT=4;BYDAY=2TU"),
        "[]",
    );
    let days = expand_event(
        &event,
        NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
        NaiveDate::from_ymd_opt(2026, 8, 1).unwrap(),
        16,
    )
    .unwrap()
    .into_iter()
    .map(|item| item.recurrence_id)
    .collect::<Vec<_>>();
    assert_eq!(days, vec!["20260310", "20260414", "20260512", "20260609"]);
    assert_eq!(
        parse_rrule("FREQ=MONTHLY;COUNT=4;BYDAY=2TU").unwrap().count,
        Some(4)
    );
    assert_eq!(
        recurrence_summary(Some("FREQ=WEEKLY;BYDAY=MO")),
        "Weekly on Monday"
    );
}
