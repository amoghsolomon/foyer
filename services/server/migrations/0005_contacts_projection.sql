-- Rebuildable CardDAV projections. Radicale remains the authority; these rows may be
-- deleted and reconstructed from WebDAV sync tokens and ETags.

CREATE TABLE contacts_address_books (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    uid TEXT NOT NULL,
    href TEXT NOT NULL,
    etag TEXT,
    display_name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    sync_token TEXT,
    ctag TEXT,
    revision BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL,
    deleted_at TIMESTAMPTZ,
    UNIQUE (user_id, uid),
    UNIQUE (user_id, href)
);

CREATE INDEX contacts_address_books_user_live_idx
    ON contacts_address_books (user_id, display_name, id)
    WHERE deleted_at IS NULL;

CREATE TABLE contacts (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    address_book_id TEXT NOT NULL REFERENCES contacts_address_books (id),
    uid TEXT NOT NULL,
    href TEXT NOT NULL,
    etag TEXT NOT NULL,
    display_name TEXT NOT NULL,
    given_name TEXT NOT NULL DEFAULT '',
    family_name TEXT NOT NULL DEFAULT '',
    additional_names TEXT NOT NULL DEFAULT '',
    honorific_prefix TEXT NOT NULL DEFAULT '',
    honorific_suffix TEXT NOT NULL DEFAULT '',
    organization TEXT NOT NULL DEFAULT '',
    job_title TEXT NOT NULL DEFAULT '',
    birthday TEXT,
    notes TEXT NOT NULL DEFAULT '',
    emails JSONB NOT NULL DEFAULT '[]'::JSONB,
    phones JSONB NOT NULL DEFAULT '[]'::JSONB,
    addresses JSONB NOT NULL DEFAULT '[]'::JSONB,
    revision BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL,
    deleted_at TIMESTAMPTZ,
    UNIQUE (user_id, uid),
    UNIQUE (user_id, href)
);

CREATE INDEX contacts_user_live_idx
    ON contacts (user_id, address_book_id, display_name, id)
    WHERE deleted_at IS NULL;

CREATE TABLE contacts_operations (
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

CREATE INDEX contacts_operations_user_idx
    ON contacts_operations (user_id, created_at DESC);

CREATE TABLE contacts_projection_checkpoints (
    user_id TEXT NOT NULL,
    address_book_id TEXT NOT NULL,
    href TEXT NOT NULL,
    sync_token TEXT,
    projected_at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (user_id, address_book_id)
);

DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_publication WHERE pubname = 'powersync') THEN
        CREATE PUBLICATION powersync FOR TABLE contacts_address_books, contacts;
    ELSIF NOT EXISTS (
        SELECT 1
        FROM pg_publication_tables
        WHERE pubname = 'powersync'
          AND tablename = 'contacts_address_books'
    ) THEN
        ALTER PUBLICATION powersync ADD TABLE contacts_address_books, contacts;
    END IF;
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'powersync_role') THEN
        GRANT SELECT ON contacts_address_books, contacts TO powersync_role;
    END IF;
END
$$;
