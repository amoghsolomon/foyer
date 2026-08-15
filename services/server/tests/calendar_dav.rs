//! Calendar slice fixtures: iCalendar folding/escaping, all-day and TZID values,
//! RRULE/EXDATE expansion, DST transitions, unknown-property preservation, and
//! conditional DAV create/update/move/delete with operation idempotency.

use foyer_server::calendar;
use foyer_server::calendar::{
    CalendarService, CreateCalendar, CreateEvent, Date, DateTime, DeleteCommand, EventDraft,
    MemoryDav, MoveEvent, RenameCalendar, Time, UpdateEvent, encode_exdates, escape_text,
    expand_event, fold_ical_line, local_to_unix, new_event_document, parse_ical, parse_rrule,
    parse_uuid, patch_event_document, serialize_ical, unescape_text, unfold_ical, unix_to_local,
};

fn uuid(n: u8) -> String {
    format!("00000000-0000-4000-a000-0000000000{n:02}")
}

fn service() -> CalendarService<MemoryDav> {
    CalendarService::new("dev-user", MemoryDav::new())
}

fn personal(svc: &mut CalendarService<MemoryDav>) -> calendar::CalendarRecord {
    svc.create_calendar(CreateCalendar {
        operation_id: uuid(1),
        id: uuid(2),
        display_name: "Personal".into(),
        description: "Default".into(),
        color: Some("#88AAFF".into()),
    })
    .expect("calendar")
}

fn timed_draft() -> EventDraft {
    EventDraft {
        summary: "Standup".into(),
        description: "Daily notes".into(),
        location: "Kitchen".into(),
        all_day: false,
        dtstart: "20260302T100000".into(),
        dtend: Some("20260302T103000".into()),
        tzid: Some("America/New_York".into()),
        rrule: None,
        exdates: Vec::new(),
    }
}

#[test]
fn folds_and_unfolds_rfc5545_lines() {
    let long = format!(
        "DESCRIPTION:{}",
        "The quick brown fox jumps over the lazy dog. ".repeat(6)
    );
    let folded = fold_ical_line(&long);
    assert!(folded.contains("\r\n "));
    assert!(
        folded
            .lines()
            .all(|line| line.len() <= calendar::ICAL_FOLD_OCTETS)
    );
    let unfolded = unfold_ical(&format!("{folded}\r\n"));
    assert_eq!(unfolded.trim_end(), long.trim_end());
}

#[test]
fn escapes_and_unescapes_text_values() {
    let raw = "Line one\nLine two; still one value, and a backslash\\";
    let escaped = escape_text(raw);
    assert_eq!(
        escaped,
        "Line one\\nLine two\\; still one value\\, and a backslash\\\\"
    );
    assert_eq!(unescape_text(&escaped), raw);
}

#[test]
fn parses_folded_escaped_description_losslessly() {
    let ical = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:fold-1\r\nDTSTART;VALUE=DATE:20260315\r\nSUMMARY:Folded\r\nDESCRIPTION:Line one\\nA very long description that must be folded across se\r\n veral seventy-five octet lines while remaining one DESCRIPTION value\\; yes.\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    let document = parse_ical(ical).expect("parse");
    let event = document.vevent().expect("vevent");
    assert_eq!(
        event.text("DESCRIPTION").unwrap(),
        "Line one\nA very long description that must be folded across several seventy-five octet lines while remaining one DESCRIPTION value; yes."
    );
}

#[test]
fn all_day_values_use_date_not_utc_midnight() {
    let mut draft = timed_draft();
    draft.all_day = true;
    draft.dtstart = "2026-03-15".into();
    draft.dtend = Some("20260316".into());
    draft.tzid = None;
    let document = new_event_document("all-day-1", &draft, 1_700_000_000).expect("document");
    let event = document.vevent().unwrap();
    let start = event.property("DTSTART").unwrap();
    assert_eq!(start.param("VALUE"), Some("DATE"));
    assert_eq!(start.value, "20260315");
    assert!(start.param("TZID").is_none());
    assert_eq!(event.property("DTEND").unwrap().value, "20260316");
}

#[test]
fn tzid_is_preserved_on_timed_dtstart() {
    let document = new_event_document("tz-1", &timed_draft(), 1_700_000_000).expect("document");
    let start = document.vevent().unwrap().property("DTSTART").unwrap();
    assert_eq!(start.param("TZID"), Some("America/New_York"));
    assert_eq!(start.value, "20260302T100000");
    assert!(!start.value.ends_with('Z'));
}

#[test]
fn unknown_properties_survive_minimally_destructive_patch() {
    let ical = "BEGIN:VCALENDAR\r\nPRODID:-//Other//Client//EN\r\nVERSION:2.0\r\nBEGIN:VTIMEZONE\r\nTZID:America/New_York\r\nX-LIC-LOCATION:America/New_York\r\nEND:VTIMEZONE\r\nBEGIN:VEVENT\r\nUID:preserve-1\r\nDTSTAMP:20260301T120000Z\r\nDTSTART;TZID=America/New_York:20260302T100000\r\nDTEND;TZID=America/New_York:20260302T110000\r\nSUMMARY:Original\r\nX-FOYER-COLOR:indigo\r\nPRIORITY:5\r\nCATEGORIES:Work,Deep\r\nBEGIN:VALARM\r\nACTION:DISPLAY\r\nDESCRIPTION:Ping\r\nTRIGGER:-PT10M\r\nEND:VALARM\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    let mut document = parse_ical(ical).expect("parse");
    let mut draft = timed_draft();
    draft.summary = "Patched title".into();
    draft.description = "Kept body\nwith newline".into();
    patch_event_document(&mut document, &draft, 1_700_000_100).expect("patch");
    let serialized = serialize_ical(&document);
    assert!(serialized.contains("X-FOYER-COLOR:indigo"));
    assert!(serialized.contains("PRIORITY:5"));
    assert!(serialized.contains("CATEGORIES:Work,Deep"));
    assert!(serialized.contains("BEGIN:VTIMEZONE"));
    assert!(serialized.contains("X-LIC-LOCATION:America/New_York"));
    assert!(serialized.contains("BEGIN:VALARM"));
    assert!(serialized.contains("TRIGGER:-PT10M"));
    assert!(serialized.contains("SUMMARY:Patched title"));
    assert!(serialized.contains("DESCRIPTION:Kept body\\nwith newline"));
    let reparsed = parse_ical(&serialized).expect("reparse");
    assert_eq!(
        reparsed.vevent().unwrap().text("X-FOYER-COLOR").unwrap(),
        "indigo"
    );
}

#[test]
fn rrule_weekly_excludes_exdate() {
    let rule = parse_rrule("FREQ=WEEKLY;BYDAY=MO").expect("rrule");
    assert_eq!(rule.ical(), "FREQ=WEEKLY;BYDAY=MO");
    let event = calendar::EventRecord {
        id: uuid(3),
        user_id: "dev-user".into(),
        calendar_id: uuid(2),
        uid: "weekly-1".into(),
        href: "/dev-user/cal/weekly-1.ics".into(),
        etag: "\"1\"".into(),
        summary: "Weekly".into(),
        description: String::new(),
        location: String::new(),
        all_day: false,
        dtstart: "20260302T100000".into(),
        dtend: Some("20260302T103000".into()),
        tzid: Some("America/New_York".into()),
        rrule: Some("FREQ=WEEKLY;BYDAY=MO".into()),
        exdates: encode_exdates(&["20260309T100000".into()]),
        revision: 1,
        created_at: String::new(),
        updated_at: String::new(),
        deleted_at: None,
    };
    let window_start = Date::new(2026, 3, 1).unwrap();
    let window_end = Date::new(2026, 3, 31).unwrap();
    let occurrences = expand_event(&event, window_start, window_end, 20).expect("expand");
    let days: Vec<_> = occurrences
        .iter()
        .map(|item| item.recurrence_id[..8].to_string())
        .collect();
    assert_eq!(days, vec!["20260302", "20260316", "20260323", "20260330"]);
    assert!(!days.contains(&"20260309".to_string()));
}

#[test]
fn dst_spring_forward_keeps_local_wall_time() {
    let local_before = DateTime {
        date: Date::new(2026, 3, 2).unwrap(),
        time: Time::new(10, 0, 0).unwrap(),
    };
    let local_after = DateTime {
        date: Date::new(2026, 3, 9).unwrap(),
        time: Time::new(10, 0, 0).unwrap(),
    };
    let before = local_to_unix("America/New_York", local_before)
        .unwrap()
        .unwrap();
    let after = local_to_unix("America/New_York", local_after)
        .unwrap()
        .unwrap();
    assert_eq!(before, 1_772_463_600); // 2026-03-02 15:00:00 UTC
    assert_eq!(after, 1_773_064_800); // 2026-03-09 14:00:00 UTC
    assert_eq!(after - before, 7 * 86_400 - 3600);
    let back = unix_to_local("America/New_York", after).unwrap();
    assert_eq!(back.time.hour, 10);
}

#[test]
fn dst_fall_back_uses_first_occurrence() {
    let local = DateTime {
        date: Date::new(2026, 11, 1).unwrap(),
        time: Time::new(1, 30, 0).unwrap(),
    };
    let unix = local_to_unix("America/New_York", local).unwrap().unwrap();
    // First 01:30 is still EDT = UTC-4 => 05:30 UTC.
    assert_eq!(unix, 1_793_511_000);
    let event = calendar::EventRecord {
        id: uuid(3),
        user_id: "dev-user".into(),
        calendar_id: uuid(2),
        uid: "dst-1".into(),
        href: "/x/dst-1.ics".into(),
        etag: "\"1\"".into(),
        summary: "Overnight".into(),
        description: String::new(),
        location: String::new(),
        all_day: false,
        dtstart: "20261025T013000".into(),
        dtend: Some("20261025T020000".into()),
        tzid: Some("America/New_York".into()),
        rrule: Some("FREQ=WEEKLY;BYDAY=SU".into()),
        exdates: "[]".into(),
        revision: 1,
        created_at: String::new(),
        updated_at: String::new(),
        deleted_at: None,
    };
    let items = expand_event(
        &event,
        Date::new(2026, 10, 25).unwrap(),
        Date::new(2026, 11, 8).unwrap(),
        8,
    )
    .expect("expand");
    assert_eq!(items.len(), 3);
    assert_eq!(items[1].recurrence_id, "20261101T013000");
    assert_eq!(items[1].start_unix, Some(1_793_511_000));
}

#[test]
fn bounded_expansion_stops_inside_the_requested_window() {
    let event = calendar::EventRecord {
        id: uuid(3),
        user_id: "dev-user".into(),
        calendar_id: uuid(2),
        uid: "daily-1".into(),
        href: "/x/daily-1.ics".into(),
        etag: "\"1\"".into(),
        summary: "Daily".into(),
        description: String::new(),
        location: String::new(),
        all_day: true,
        dtstart: "20200101".into(),
        dtend: Some("20200102".into()),
        tzid: None,
        rrule: Some("FREQ=DAILY".into()),
        exdates: "[]".into(),
        revision: 1,
        created_at: String::new(),
        updated_at: String::new(),
        deleted_at: None,
    };
    let items = expand_event(
        &event,
        Date::new(2026, 3, 1).unwrap(),
        Date::new(2026, 3, 7).unwrap(),
        512,
    )
    .expect("expand");
    assert_eq!(items.len(), 7);
    assert_eq!(items.first().unwrap().recurrence_id, "20260301");
    assert_eq!(items.last().unwrap().recurrence_id, "20260307");
}

#[test]
fn rejects_an_oversized_expansion_window() {
    let event = calendar::EventRecord {
        id: uuid(3),
        user_id: "dev-user".into(),
        calendar_id: uuid(2),
        uid: "daily-2".into(),
        href: "/x/daily-2.ics".into(),
        etag: "\"1\"".into(),
        summary: "Daily".into(),
        description: String::new(),
        location: String::new(),
        all_day: true,
        dtstart: "20200101".into(),
        dtend: None,
        tzid: None,
        rrule: Some("FREQ=DAILY".into()),
        exdates: "[]".into(),
        revision: 1,
        created_at: String::new(),
        updated_at: String::new(),
        deleted_at: None,
    };
    let error = expand_event(
        &event,
        Date::new(2020, 1, 1).unwrap(),
        Date::new(2023, 1, 1).unwrap(),
        10,
    )
    .unwrap_err();
    assert!(error.to_string().contains("at most"));
}

#[test]
fn create_update_move_delete_use_conditional_dav() {
    let mut svc = service();
    let calendar = personal(&mut svc);
    let created = svc
        .create_event(CreateEvent {
            operation_id: uuid(3),
            id: uuid(4),
            calendar_id: calendar.id.clone(),
            uid: Some("standup-1".into()),
            draft: timed_draft(),
        })
        .expect("create");
    assert_eq!(created.uid, "standup-1");
    assert_eq!(created.revision, 1);
    assert!(created.href.ends_with("standup-1.ics"));
    assert_eq!(created.tzid.as_deref(), Some("America/New_York"));

    let mut draft = timed_draft();
    draft.summary = "Standup (moved)".into();
    draft.description = "Bring notes\nand coffee".into();
    let updated = svc
        .update_event(
            &created.id,
            UpdateEvent {
                operation_id: uuid(5),
                expected_revision: created.revision,
                expected_etag: Some(created.etag.clone()),
                draft,
            },
        )
        .expect("update");
    assert_eq!(updated.summary, "Standup (moved)");
    assert_eq!(updated.description, "Bring notes\nand coffee");
    assert_ne!(updated.etag, created.etag);
    assert_eq!(updated.revision, 2);

    let work = svc
        .create_calendar(CreateCalendar {
            operation_id: uuid(6),
            id: uuid(7),
            display_name: "Work".into(),
            description: String::new(),
            color: None,
        })
        .expect("work calendar");
    let moved = svc
        .move_event(
            &created.id,
            MoveEvent {
                operation_id: uuid(8),
                expected_revision: updated.revision,
                expected_etag: Some(updated.etag.clone()),
                calendar_id: work.id.clone(),
            },
        )
        .expect("move");
    assert_eq!(moved.calendar_id, work.id);
    assert!(moved.href.contains(&work.id));
    assert_eq!(svc.list_events(Some(&calendar.id)).len(), 0);
    assert_eq!(svc.list_events(Some(&work.id)).len(), 1);

    let deleted = svc
        .delete_event(
            &created.id,
            DeleteCommand {
                operation_id: uuid(9),
                expected_revision: moved.revision,
                expected_etag: Some(moved.etag.clone()),
            },
        )
        .expect("delete");
    assert!(deleted.deleted_at.is_some());
    assert!(svc.list_events(None).is_empty());
}

#[test]
fn stale_etag_and_revision_fail_visibly() {
    let mut svc = service();
    let calendar = personal(&mut svc);
    let created = svc
        .create_event(CreateEvent {
            operation_id: uuid(3),
            id: uuid(4),
            calendar_id: calendar.id.clone(),
            uid: None,
            draft: timed_draft(),
        })
        .expect("create");
    let stale_revision = svc.update_event(
        &created.id,
        UpdateEvent {
            operation_id: uuid(5),
            expected_revision: 99,
            expected_etag: Some(created.etag.clone()),
            draft: timed_draft(),
        },
    );
    assert!(matches!(
        stale_revision,
        Err(calendar::CalendarError::StaleRevision { expected: 99, .. })
    ));

    svc.update_event(
        &created.id,
        UpdateEvent {
            operation_id: uuid(6),
            expected_revision: created.revision,
            expected_etag: Some(created.etag.clone()),
            draft: {
                let mut draft = timed_draft();
                draft.summary = "Changed".into();
                draft
            },
        },
    )
    .expect("first writer");
    let stale_etag = svc.update_event(
        &created.id,
        UpdateEvent {
            operation_id: uuid(7),
            expected_revision: created.revision + 1,
            expected_etag: Some("\"not-the-etag\"".into()),
            draft: timed_draft(),
        },
    );
    assert!(matches!(
        stale_etag,
        Err(calendar::CalendarError::StaleEtag { .. })
    ));
}

#[test]
fn operation_ids_are_idempotent_and_bind_arguments() {
    let mut svc = service();
    let calendar = personal(&mut svc);
    let first = svc
        .create_event(CreateEvent {
            operation_id: uuid(3),
            id: uuid(4),
            calendar_id: calendar.id.clone(),
            uid: Some("op-1".into()),
            draft: timed_draft(),
        })
        .expect("create");
    let retry = svc
        .create_event(CreateEvent {
            operation_id: uuid(3),
            id: uuid(4),
            calendar_id: calendar.id.clone(),
            uid: Some("op-1".into()),
            draft: timed_draft(),
        })
        .expect("retry");
    assert_eq!(first, retry);

    let rebound = svc.create_event(CreateEvent {
        operation_id: uuid(3),
        id: uuid(4),
        calendar_id: calendar.id.clone(),
        uid: Some("op-1".into()),
        draft: {
            let mut draft = timed_draft();
            draft.summary = "Different".into();
            draft
        },
    });
    assert!(matches!(rebound, Err(calendar::CalendarError::Conflict(_))));
}

#[test]
fn cannot_resurrect_tombstone_or_delete_nonempty_calendar() {
    let mut svc = service();
    let calendar = personal(&mut svc);
    let created = svc
        .create_event(CreateEvent {
            operation_id: uuid(3),
            id: uuid(4),
            calendar_id: calendar.id.clone(),
            uid: None,
            draft: timed_draft(),
        })
        .expect("create");
    let nonempty = svc.delete_calendar(
        &calendar.id,
        DeleteCommand {
            operation_id: uuid(10),
            expected_revision: calendar.revision,
            expected_etag: Some(calendar.etag.clone()),
        },
    );
    assert!(matches!(
        nonempty,
        Err(calendar::CalendarError::Conflict(_))
    ));

    svc.delete_event(
        &created.id,
        DeleteCommand {
            operation_id: uuid(11),
            expected_revision: created.revision,
            expected_etag: Some(created.etag.clone()),
        },
    )
    .expect("delete event");
    let resurrect = svc.create_event(CreateEvent {
        operation_id: uuid(12),
        id: uuid(4),
        calendar_id: calendar.id.clone(),
        uid: None,
        draft: timed_draft(),
    });
    assert!(matches!(resurrect, Err(calendar::CalendarError::Gone(_))));
}

#[test]
fn rename_is_idempotent_and_uuid_validated() {
    let mut svc = service();
    let calendar = personal(&mut svc);
    let renamed = svc
        .rename_calendar(
            &calendar.id,
            RenameCalendar {
                operation_id: uuid(20),
                expected_revision: calendar.revision,
                expected_etag: Some(calendar.etag.clone()),
                display_name: "Home".into(),
            },
        )
        .expect("rename");
    assert_eq!(renamed.display_name, "Home");
    let retry = svc
        .rename_calendar(
            &calendar.id,
            RenameCalendar {
                operation_id: uuid(20),
                expected_revision: calendar.revision,
                expected_etag: Some(calendar.etag.clone()),
                display_name: "Home".into(),
            },
        )
        .expect("retry");
    assert_eq!(renamed, retry);
    assert!(parse_uuid("id", "not-a-uuid").is_err());
}

#[test]
fn projector_rebuilds_normalized_rows_from_dav() {
    let mut svc = service();
    let calendar = personal(&mut svc);
    svc.create_event(CreateEvent {
        operation_id: uuid(3),
        id: uuid(4),
        calendar_id: calendar.id.clone(),
        uid: Some("rebuild-1".into()),
        draft: timed_draft(),
    })
    .expect("create");
    svc.projection.events.clear();
    svc.rebuild_from_dav().expect("rebuild");
    let rebuilt = svc.list_events(None);
    assert_eq!(rebuilt.len(), 1);
    assert_eq!(rebuilt[0].uid, "rebuild-1");
    assert_eq!(rebuilt[0].summary, "Standup");
}

#[test]
fn monthly_nth_weekday_and_count() {
    let event = calendar::EventRecord {
        id: uuid(3),
        user_id: "dev-user".into(),
        calendar_id: uuid(2),
        uid: "nth".into(),
        href: "/x/nth.ics".into(),
        etag: "\"1\"".into(),
        summary: "Second Tuesday".into(),
        description: String::new(),
        location: String::new(),
        all_day: true,
        dtstart: "20260310".into(),
        dtend: Some("20260311".into()),
        tzid: None,
        rrule: Some("FREQ=MONTHLY;COUNT=4;BYDAY=2TU".into()),
        exdates: "[]".into(),
        revision: 1,
        created_at: String::new(),
        updated_at: String::new(),
        deleted_at: None,
    };
    let items = expand_event(
        &event,
        Date::new(2026, 3, 1).unwrap(),
        Date::new(2026, 8, 1).unwrap(),
        16,
    )
    .expect("expand");
    let days: Vec<_> = items
        .iter()
        .map(|item| item.recurrence_id.clone())
        .collect();
    assert_eq!(days, vec!["20260310", "20260414", "20260512", "20260609"]);
}
