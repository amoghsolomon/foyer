-- Manually enrolled device keys and one-shot authentication challenges.
-- These tables are canonical, operator-administered, and must never be
-- published to PowerSync.

CREATE TABLE foyer_users (
    id TEXT PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL
);

CREATE TABLE device_keys (
    device_key_id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES foyer_users (id),
    label TEXT NOT NULL,
    public_jwk JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL,
    last_seen_at TIMESTAMPTZ,
    revoked_at TIMESTAMPTZ
);

CREATE INDEX device_keys_user_idx ON device_keys (user_id);
CREATE INDEX device_keys_live_idx
    ON device_keys (user_id, created_at)
    WHERE revoked_at IS NULL;

CREATE TABLE auth_challenges (
    challenge_id TEXT PRIMARY KEY,
    device_key_id TEXT NOT NULL,
    user_id TEXT NOT NULL REFERENCES foyer_users (id),
    signing_payload BYTEA NOT NULL,
    payload_sha256 BYTEA NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    consumed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX auth_challenges_outstanding_idx
    ON auth_challenges (device_key_id, expires_at)
    WHERE consumed_at IS NULL;

CREATE INDEX auth_challenges_expires_idx ON auth_challenges (expires_at);

CREATE TABLE auth_audit_events (
    id BIGSERIAL PRIMARY KEY,
    event_type TEXT NOT NULL,
    user_id TEXT,
    device_key_id TEXT,
    challenge_id TEXT,
    created_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX auth_audit_events_created_idx
    ON auth_audit_events (created_at DESC);

DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'powersync_role') THEN
        REVOKE SELECT ON foyer_users FROM powersync_role;
        REVOKE SELECT ON device_keys FROM powersync_role;
        REVOKE SELECT ON auth_challenges FROM powersync_role;
        REVOKE SELECT ON auth_audit_events FROM powersync_role;
    END IF;
END
$$;
