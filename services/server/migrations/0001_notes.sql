CREATE TABLE notes_folders (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    parent_id TEXT REFERENCES notes_folders (id),
    name TEXT NOT NULL,
    position INTEGER NOT NULL,
    revision BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL,
    deleted_at TIMESTAMPTZ
);

CREATE INDEX notes_folders_user_live_idx
    ON notes_folders (user_id, parent_id, position, name, id)
    WHERE deleted_at IS NULL;

CREATE TABLE notes (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    folder_id TEXT NOT NULL REFERENCES notes_folders (id),
    title TEXT NOT NULL,
    body TEXT NOT NULL,
    revision BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL,
    deleted_at TIMESTAMPTZ
);

CREATE INDEX notes_user_live_idx
    ON notes (user_id, folder_id, updated_at DESC, id)
    WHERE deleted_at IS NULL;

CREATE TABLE notes_operations (
    operation_id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    entity_type TEXT NOT NULL,
    entity_id TEXT NOT NULL,
    operation TEXT NOT NULL,
    result_status INTEGER NOT NULL,
    result_body JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX notes_operations_user_idx
    ON notes_operations (user_id, created_at DESC);

DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_publication WHERE pubname = 'powersync') THEN
        CREATE PUBLICATION powersync FOR TABLE notes_folders, notes;
    END IF;
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'powersync_role') THEN
        GRANT SELECT ON notes_folders, notes TO powersync_role;
    END IF;
END
$$;
