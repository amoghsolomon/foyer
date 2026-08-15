ALTER TABLE notes_operations
    ADD COLUMN request_body JSONB;

UPDATE notes_operations
SET request_body = '{}'::JSONB
WHERE request_body IS NULL;

ALTER TABLE notes_operations
    ALTER COLUMN request_body SET NOT NULL;
