CREATE TABLE bookmarks_folders (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    parent_id TEXT REFERENCES bookmarks_folders (id),
    name TEXT NOT NULL,
    position INTEGER NOT NULL,
    revision BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL,
    deleted_at TIMESTAMPTZ
);

CREATE INDEX bookmarks_folders_user_live_idx
    ON bookmarks_folders (user_id, parent_id, position, name, id)
    WHERE deleted_at IS NULL;

CREATE TABLE bookmarks (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    folder_id TEXT NOT NULL REFERENCES bookmarks_folders (id),
    url TEXT NOT NULL,
    title TEXT NOT NULL,
    description TEXT NOT NULL,
    tags JSONB NOT NULL,
    favorite BOOLEAN NOT NULL,
    archived BOOLEAN NOT NULL,
    position INTEGER NOT NULL,
    revision BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL,
    deleted_at TIMESTAMPTZ
);

CREATE INDEX bookmarks_user_live_idx
    ON bookmarks (user_id, folder_id, archived, favorite DESC, position, title, id)
    WHERE deleted_at IS NULL;

CREATE TABLE bookmarks_operations (
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

CREATE INDEX bookmarks_operations_user_idx
    ON bookmarks_operations (user_id, created_at DESC);

DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_publication WHERE pubname = 'powersync') THEN
        CREATE PUBLICATION powersync FOR TABLE bookmarks_folders, bookmarks;
    ELSE
        BEGIN
            ALTER PUBLICATION powersync ADD TABLE bookmarks_folders, bookmarks;
        EXCEPTION
            WHEN duplicate_object THEN
                NULL;
        END;
    END IF;
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'powersync_role') THEN
        GRANT SELECT ON bookmarks_folders, bookmarks TO powersync_role;
    END IF;
END
$$;
