-- Rebuildable DAV projector state. Radicale remains the authority for calendar,
-- task, and contact resources. These tables may be truncated and rebuilt from
-- CalDAV/CardDAV. They are not a second writable store and are not published
-- to PowerSync.

CREATE TABLE dav_collection_checkpoints (
    user_id TEXT NOT NULL,
    collection_href TEXT NOT NULL,
    collection_kind TEXT NOT NULL,
    collection_id TEXT NOT NULL,
    display_name TEXT,
    sync_token TEXT,
    collection_etag TEXT,
    last_error TEXT,
    last_projected_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (user_id, collection_href),
    CONSTRAINT dav_collection_checkpoints_kind_chk
        CHECK (collection_kind IN ('calendar', 'task_list', 'address_book'))
);

CREATE INDEX dav_collection_checkpoints_user_idx
    ON dav_collection_checkpoints (user_id, collection_kind, collection_id);

CREATE TABLE dav_operation_bindings (
    operation_id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    entity_type TEXT NOT NULL,
    entity_id TEXT NOT NULL,
    operation TEXT NOT NULL,
    request_body JSONB NOT NULL,
    collection_href TEXT,
    resource_href TEXT,
    etag TEXT,
    dav_uid TEXT,
    result_status INTEGER NOT NULL,
    result_body JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX dav_operation_bindings_user_idx
    ON dav_operation_bindings (user_id, created_at DESC);

CREATE INDEX dav_operation_bindings_resource_idx
    ON dav_operation_bindings (user_id, resource_href);

DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'powersync_role') THEN
        REVOKE SELECT ON dav_collection_checkpoints FROM powersync_role;
        REVOKE SELECT ON dav_operation_bindings FROM powersync_role;
    END IF;
END
$$;
