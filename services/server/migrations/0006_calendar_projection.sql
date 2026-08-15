-- Rebuildable CalDAV projections. Radicale remains the authority; these rows may be
-- deleted and reconstructed from DAV. PowerSync may replicate only the non-secret
-- calendar and event columns (never raw iCalendar or operation/checkpoint tables).

CREATE TABLE calendar_calendars (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    uid TEXT NOT NULL,
    href TEXT NOT NULL,
    etag TEXT NOT NULL,
    display_name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    color TEXT,
    ctag TEXT,
    sync_token TEXT,
    revision BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL,
    deleted_at TIMESTAMPTZ
);

CREATE UNIQUE INDEX calendar_calendars_user_href_idx
    ON calendar_calendars (user_id, href);

CREATE INDEX calendar_calendars_user_live_idx
    ON calendar_calendars (user_id, display_name, id)
    WHERE deleted_at IS NULL;

CREATE TABLE calendar_events (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    calendar_id TEXT NOT NULL REFERENCES calendar_calendars (id),
    uid TEXT NOT NULL,
    href TEXT NOT NULL,
    etag TEXT NOT NULL,
    summary TEXT NOT NULL,
    description TEXT NOT NULL,
    location TEXT NOT NULL DEFAULT '',
    all_day BOOLEAN NOT NULL,
    dtstart TEXT NOT NULL,
    dtend TEXT,
    tzid TEXT,
    rrule TEXT,
    exdates TEXT NOT NULL DEFAULT '[]',
    revision BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL,
    deleted_at TIMESTAMPTZ
);

CREATE UNIQUE INDEX calendar_events_user_uid_live_idx
    ON calendar_events (user_id, uid)
    WHERE deleted_at IS NULL;

CREATE UNIQUE INDEX calendar_events_user_href_idx
    ON calendar_events (user_id, href);

CREATE INDEX calendar_events_user_live_idx
    ON calendar_events (user_id, calendar_id, dtstart, id)
    WHERE deleted_at IS NULL;

-- Server-only raw payload used for minimally destructive patches. Not a PowerSync stream.
CREATE TABLE calendar_event_payloads (
    event_id TEXT PRIMARY KEY REFERENCES calendar_events (id) ON DELETE CASCADE,
    ical_text TEXT NOT NULL
);

CREATE TABLE calendar_operations (
    operation_id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    entity_type TEXT NOT NULL,
    entity_id TEXT NOT NULL,
    operation TEXT NOT NULL,
    request_body TEXT NOT NULL,
    result_status INTEGER NOT NULL,
    result_body TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX calendar_operations_user_idx
    ON calendar_operations (user_id, created_at DESC);

CREATE TABLE calendar_projection_checkpoints (
    user_id TEXT NOT NULL,
    calendar_href TEXT NOT NULL,
    sync_token TEXT,
    projected_at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (user_id, calendar_href)
);

DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_publication WHERE pubname = 'powersync') THEN
        CREATE PUBLICATION powersync FOR TABLE calendar_calendars, calendar_events;
    ELSE
        BEGIN
            ALTER PUBLICATION powersync ADD TABLE calendar_calendars, calendar_events;
        EXCEPTION
            WHEN duplicate_object THEN NULL;
        END;
    END IF;
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'powersync_role') THEN
        GRANT SELECT ON calendar_calendars, calendar_events TO powersync_role;
    END IF;
END
$$;
