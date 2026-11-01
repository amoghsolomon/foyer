# Foyer single-thread agent

An exact-version upstream Pi Durable 1.4.0 host. It has one persistent conversation, no tools,
no memory/retrieval/scheduler/subagents, and no default provider. See ADR 0028 and
`contracts/openapi/foyer-agent-v1.yaml`. The existing Presentation sidecar is independent.

Run `npm ci --ignore-scripts` and `npm test`. Tests use Pi's faux provider and SQLite, including
an actual worker kill and reopen; they do not require credentials or call paid APIs.
For local testing on the Fedora host set `TMPDIR` to a directory on `/mnt/storage`.

Production uses Node >=22.19.0, one writer, a private writable `FOYER_AGENT_DATA_DIR`, and loopback
port 3584. Rust needs `FOYER_AGENT_URL=http://127.0.0.1:3584/` and `FOYER_AGENT_OWNER_ID`.
Never expose the sidecar directly. It trusts its pod's loopback boundary and has no independent auth.

Without provider configuration, reads and health checks work but submitting returns 503 with
`provider_not_configured`. Later configure all of `FOYER_AGENT_PROVIDER`, `FOYER_AGENT_MODEL` and
the provider's private environment key. Supported adapters: openai/OPENAI_API_KEY,
anthropic/ANTHROPIC_API_KEY, openrouter/OPENROUTER_API_KEY. Partial configuration fails startup.
No personal CLI/OAuth sessions or global Pi credentials are read or copied.

The adapter persists content hashes before Pi admission, then uses Pi request IDs to recover lost
receipts. A different payload under the same ID is 409. A new input while busy is 409. Admission
metadata is bounded at 10000 distinct request IDs; no history is silently discarded.

Stop the writer before archiving/restoring the complete SQLite directory. Keep the SQLite DB and
WAL files together; never copy only a live `thread.sqlite`. `writer.lock` is a lease, not content;
a restarted writer waits for a dead process's stale lease. Recreate deployment and ReadWriteOnce
PVC prevent intentional concurrent writers. This store is additional canonical data beyond the
existing PostgreSQL/Radicale backup bundle and must be included in recovery procedures.
