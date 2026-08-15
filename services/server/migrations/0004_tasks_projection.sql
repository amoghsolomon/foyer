-- Rebuildable CalDAV projections for task lists and VTODO resources.
-- Radicale remains the authority. These rows may be deleted and rebuilt.

CREATE TABLE task_lists (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    name TEXT NOT NULL,
    position INTEGER NOT NULL,
    href TEXT NOT NULL,
    etag TEXT,
    ctag TEXT,
    sync_token TEXT,
    revision BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL,
    deleted_at TIMESTAMPTZ
);

CREATE UNIQUE INDEX task_lists_user_href_live_idx
    ON task_lists (user_id, href)
    WHERE deleted_at IS NULL;

CREATE INDEX task_lists_user_live_idx
    ON task_lists (user_id, position, name, id)
    WHERE deleted_at IS NULL;

CREATE TABLE tasks (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    list_id TEXT NOT NULL REFERENCES task_lists (id),
    title TEXT NOT NULL,
    description TEXT NOT NULL,
    due_at TIMESTAMPTZ,
    due_local TEXT,
    due_time_zone TEXT,
    due_all_day BOOLEAN NOT NULL DEFAULT FALSE,
    priority INTEGER NOT NULL DEFAULT 0,
    completed BOOLEAN NOT NULL DEFAULT FALSE,
    completed_at TIMESTAMPTZ,
    position INTEGER NOT NULL,
    href TEXT NOT NULL,
    etag TEXT NOT NULL,
    revision BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL,
    deleted_at TIMESTAMPTZ
);

CREATE UNIQUE INDEX tasks_user_href_live_idx
    ON tasks (user_id, href)
    WHERE deleted_at IS NULL;

CREATE INDEX tasks_user_live_idx
    ON tasks (user_id, list_id, position, title, id)
    WHERE deleted_at IS NULL;

CREATE TABLE tasks_operations (
    operation_id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    entity_type TEXT NOT NULL,
    entity_id TEXT NOT NULL,
    operation TEXT NOT NULL,
    request_body JSONB NOT NULL,
    result_status INTEGER NOT NULL,
    result_body JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX tasks_operations_user_idx
    ON tasks_operations (user_id, created_at DESC);

CREATE TABLE tasks_dav_checkpoints (
    user_id TEXT NOT NULL,
    collection_href TEXT NOT NULL,
    sync_token TEXT,
    projected_at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (user_id, collection_href)
);

DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_publication WHERE pubname = 'powersync') THEN
        CREATE PUBLICATION powersync FOR TABLE task_lists, tasks;
    ELSE
        BEGIN
            ALTER PUBLICATION powersync ADD TABLE task_lists;
        EXCEPTION WHEN duplicate_object THEN NULL;
        END;
        BEGIN
            ALTER PUBLICATION powersync ADD TABLE tasks;
        EXCEPTION WHEN duplicate_object THEN NULL;
        END;
    END IF;
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'powersync_role') THEN
        GRANT SELECT ON task_lists, tasks TO powersync_role;
    END IF;
END
$$;
