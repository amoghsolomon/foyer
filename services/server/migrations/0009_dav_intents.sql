-- Bind DAV requests before external side effects; survives a rollback of the projection transaction.
CREATE TABLE dav_mutation_intents (
 operation_id TEXT PRIMARY KEY,
 user_id TEXT NOT NULL,
 entity_type TEXT NOT NULL,
 entity_id TEXT NOT NULL,
 operation TEXT NOT NULL,
 request_body JSONB NOT NULL,
 created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
DO $$ BEGIN
 IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'powersync_role') THEN
  REVOKE ALL ON dav_mutation_intents FROM powersync_role;
 END IF;
END $$;
