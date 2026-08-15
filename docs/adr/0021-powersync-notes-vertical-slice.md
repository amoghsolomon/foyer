# ADR 0021: Validate PowerSync Open Edition for the notes vertical slice

- **Status:** Accepted
- **Date:** 2026-08-15
- **Owners:** Foyer project

## Context

ADR 0020 made Foyer Server PostgreSQL the authority for notes and required a replaceable sync
engine that replicates user-scoped records into a client database distinct from Foyer Shell-owned
SQLite. This slice is the first real product surface that must prove that map: folders, lossless
Markdown notes, offline writes, and a shared Android and Foyer Shell replica.

The former Android notes path still calls the deleted Cloudflare `/api/notes` service. Reusing that
contract, giving clients PostgreSQL credentials, or writing the PowerSync replica into
`foyer-shell-storage` would violate ADR 0019, ADR 0020, and ADR 0007.

PowerSync Open Edition can replicate PostgreSQL into per-client SQLite and already has an official
Kotlin SDK. Its Rust client is published as an alpha crate. Selecting it as a production dependency
without a bounded validation would lock the monorepo to an unproven native client and to an
externally supervised replication service.

## Decision

Foyer accepts PowerSync Open Edition as the replaceable replication transport for this notes slice
only. The validation criteria in this record have been met. This does not select PowerSync for
every future personal-data surface; each additional surface must preserve the same server-authority
and replaceable-transport boundary.

### Authority and upload flow

1. Clients may create, rename, move, and delete folders and notes while offline using
   client-generated UUIDs, operation IDs, and expected revisions.
2. Pending writes upload only to the Foyer Server API. Foyer Server is the only mutation and
   validation boundary.
3. The server applies writes idempotently to canonical PostgreSQL rows, including explicit
   tombstones.
4. PowerSync replicates only the current user's non-secret folder and note projections from those
   canonical rows into a client replica SQLite database.
5. Clients never receive PostgreSQL credentials and never write to PostgreSQL.

### Ownership

| Concern | Owner |
| --- | --- |
| Folder and note semantics, revisions, tombstones, idempotency | `foyer-server` |
| Canonical rows | PostgreSQL on the Compose network |
| Replication transport and bucket storage | PowerSync Open Edition (external service container) |
| Android replica and upload connector | Android notes adapter using the official Kotlin SDK |
| Foyer Shell replica and upload connector | `foyer-shell-notes`, a bounded crate separate from `foyer-shell-storage` |
| Wire schemas | `contracts/openapi/foyer-v1.yaml` |

### PowerSync licensing and source availability

PowerSync Open Edition is source-available. The service code is published at
`https://github.com/powersync-ja/powersync-service` and distributed as
`journeyapps/powersync-service`. Open Edition is sufficient for a personal self-hosted deployment
and for this validation. It is not OSI-approved open source in every artifact, and the Enterprise
edition is out of scope. The service remains an external container Foyer does not vendor.

### External service container

Local Compose runs PowerSync as its own service with checked-in configuration. PostgreSQL stays
private to the Compose network. PowerSync may use a separate PostgreSQL database on that same
private instance for bucket storage so this slice does not add MongoDB. Caddy remains optional;
development clients can reach Foyer Server and PowerSync on localhost-only published ports.

### Conflict policy

- Create, rename/move, and delete are semantic commands with a client `operationId`.
- Updates, moves, and deletes require `expectedRevision`. A mismatch is a conflict, not last-write-wins.
- A tombstoned folder or note cannot be resurrected by a stale write or a reused identifier.
- Folder deletion is rejected while live child folders or notes remain.
- Missing, foreign, or cyclic parents are rejected.
- Cross-user reads and writes are rejected.

### Native Rust client and isolation seam

The official `powersync` crate remains pre-release. Foyer Shell therefore isolates it in
`foyer-shell-notes`: the SDK owns a separate replica database on a dedicated Tokio worker thread,
and GPUI receives immutable snapshots over channels. Render callbacks never perform network or
SQLite work, and `foyer-shell-storage` never opens the replica.

The monorepo is pinned to Rust 1.97.1, satisfying the SDK's minimum compiler requirement. Clang is
a Shell build prerequisite because the SDK's SQLite bindings use bindgen. The earlier temporary
HTTP provider has been removed; all Shell reads now come from the native replica and all uploads go
through Foyer Server's semantic mutation endpoints.

PowerSync PATCH entries contain only columns whose values changed. Each adapter therefore persists
the complete semantic update payload in a client-only replica column, salted with the operation id,
in the same SQLite transaction as the optimistic row change. Uploads never infer an omitted title
or body from a partial CRUD entry. The payload is local queue metadata and is absent from sync
streams and canonical PostgreSQL rows.

Removing PowerSync later means deleting the Compose service, client connectors, and this ADR's
transport selection. The Foyer API, PostgreSQL schema, and OpenAPI contract remain.

### Development authentication

This slice uses one visibly development-only user and bearer token. The server refuses to start in
development when that token, user id, or PowerSync JWT secret is missing. Outside development the
static token is ignored and authentication fails closed. Production identity is a later decision.
The local HS256 key is supplied to PowerSync as a static symmetric JWK with an explicit key id;
remote JWKS discovery is reserved for a future asymmetric production identity provider.

## Alternatives and deliberate exclusions

- A REST-only notes API without a replica would force each client to invent offline queues and
  would not exercise ADR 0020's sync-engine seam.
- Writing clients directly to PostgreSQL is excluded.
- Rebuilding the deleted Cloudflare notes API is excluded.
- Merging the replica into `foyer.db` or `foyer-shell.sqlite3` is excluded.
- Last-write-wins without expected revisions is excluded.
- pgvector embeddings, agents, mail, DAV, calendar, contacts, and production auth are excluded
  from this slice.

## Consequences and risks

Android and Foyer Shell can share one notes authority and operate from local replicas. The product
gains an explicit, removable PowerSync seam instead of a silent permanent dependency.

Risks:

- PowerSync's pre-release Rust SDK may introduce source or replica-format churn during upgrades.
- The Open Edition service adds a second database consumer, logical replication, and JWT
  verification that can fail independently of Foyer Server.
- Soft-deleted rows must be filtered from streams so clients observe tombstones without syncing
  operation or credential tables.
- A successful Android integration does not by itself prove the Rust client.

## Validation criteria

This ADR may move from Proposed to Accepted only when all of the following are true:

1. Compose starts PostgreSQL, Foyer Server, and PowerSync Open Edition with one documented command,
   PostgreSQL unpublished on the host, and a localhost development path that does not require
   Caddy.
2. Foyer Server applies folder and note mutations idempotently, rejects the invalid cases listed
   above, and remains the only writer to canonical rows.
3. PowerSync streams contain only the authenticated user's non-secret folder and note projections.
   Credentials, operation/audit internals, embeddings, and Foyer Shell tables are absent.
4. Android uses the official Kotlin PowerSync SDK against a replica database distinct from Room
   `foyer.db`, reads reactively, and uploads offline writes through Foyer Server.
5. Foyer Shell reads notes from immutable snapshots produced off the render thread. Its native
   PowerSync replica is not `foyer-shell-storage`, and the SDK compiles and syncs in this repository.
6. Integration tests cover create, lossless Markdown, idempotent retry, stale revision, invalid and
   cyclic parents, nonempty folder deletion, note move, tombstone propagation, user isolation,
   restart persistence, and full replica rebuild without production secrets.
7. The replica can be deleted and rebuilt from server state without database credentials.

### Validation evidence (2026-08-15)

- The Compose stack reached healthy state with PostgreSQL unpublished and the API and PowerSync
  bound to loopback. PowerSync created its logical-replication slot and loaded only the two notes
  streams.
- The Rust integration suite executed against a real Testcontainers PostgreSQL instance. A live
  Compose API pass additionally covered exact retry binding, cross-user operation-id isolation,
  lossless Markdown, hierarchy conflicts, moves, tombstones, and sync credentials.
- Android used its distinct `foyer-notes-powersync.db` replica to create a note while Foyer Server
  and PowerSync were stopped. The note and stable operation id survived a force-stop and APK
  reinstall, remained visible offline, and uploaded automatically after both services returned.
- The monorepo was upgraded to Rust 1.97.1 and the native `powersync` 0.0.7 SDK compiled with Clang.
  A fresh Shell replica rebuilt all canonical notes under Niri, and the Notes overlay opened from
  immutable native-replica snapshots without touching `foyer-shell-storage`.
- A live native Shell round trip replicated server state, preserved exact Markdown, uploaded a
  semantic create through Foyer Server, and drained its durable CRUD queue. A second write was made
  with Foyer Server and PowerSync stopped; the write and operation id survived a worker restart and
  uploaded automatically after reconnection.
- Deleting that note through Foyer Server produced a tombstone that removed it from the running
  native Shell replica. The server, Android, and native Shell integration coverage together exercise
  every conflict, isolation, persistence, tombstone, and rebuild case listed in criterion 6.
- Cross-client UI validation created nested folders and lossless Markdown on Android, observed the
  exact trailing newline in Foyer Server and the native Shell replica, then performed a body-only
  native Shell update that appeared reactively in the open Android detail screen. A focused Android
  device test also exercised body-only update upload using the durable semantic payload.

## Supersession

This ADR implements the notes portion of ADR 0020. It does not replace ADR 0020's broader
personal-data map or ADR 0007's Foyer Shell storage boundary.
