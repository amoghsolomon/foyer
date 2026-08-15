# ADR 0020: Host personal data behind Foyer Server and replicated clients

- **Status:** Accepted
- **Date:** 2026-08-14
- **Owners:** Foyer project

## Context

ADR 0014 selected Evolution Data Server as Foyer Shell's initial read-only calendar and task
provider so the desktop would not own Google credentials, synchronization, recurrence, or conflicts.
The product direction now excludes Google services and needs the same mail, notes, bookmarks, tasks,
calendar, contacts, Activities, and retrieval substrate on Android and Foyer Shell.

The former Android backend mixed persistent agent execution, authentication, personal-data tables,
custom synchronization, memory, search, and Cloudflare-specific storage in one abandoned JavaScript
service. Reusing it would retain D1, Vectorize, R2, Durable Object, Flue, and Firebase assumptions.
Implementing another desktop-only provider would duplicate synchronization and prevent the clients
from sharing one personal-data authority.

OpenAPI describes requests but does not itself provide offline replication, checkpoints, tombstones,
or conflict handling. A sync engine can provide the transport while Foyer-owned domain code retains
semantic validation, capability policy, and domain-specific conflict decisions.

## Decision

Foyer adds an independently deployable Rust service, `foyer-server`, hosted on the user's server.
It exposes versioned OpenAPI queries and semantic commands and coordinates a replaceable sync engine
that replicates bounded user-scoped records into local client databases.

The initial authority map is:

| Domain | Authority |
| --- | --- |
| Mailbox contents and delivery | Purelymail through IMAP and authenticated SMTP submission |
| Calendar, tasks, and contacts | A standards-capable CalDAV/CardDAV service selected during implementation |
| Notes, bookmarks, Activities, profile memory, and operation records | Foyer Server PostgreSQL |
| Search documents, chunks, lexical indexes, and embeddings | Rebuildable projections derived by Foyer Server |
| Desktop preferences, notifications, approvals, and Presentation catalog | Existing Foyer Shell SQLite repositories |
| Client replica and pending offline operations | A separate replaceable sync-engine database on each client |

PostgreSQL is not exposed to clients or models. The server's pgvector embeddings are derived indexes,
never the canonical copy of an item. Each chunk records its source identity, source revision or hash,
location, and embedding-model version. Retrieval combines lexical and semantic evidence and returns
traceable source locations.

Client mutations use stable client-generated operation identifiers, entity identifiers, expected
revisions, and tombstones. The sync engine transports records and pending operations; the Foyer API
validates ownership, authorization, idempotency, recurrence scope, attendee or delivery policy, and
domain conflicts. A model cannot call the database, sync service, DAV provider, IMAP, or SMTP
directly. Consequential operations still pass through the immutable capability and approval boundary.

Foyer Shell replaces the EDS provider behind its normalized agenda/controller boundary with a hosted
personal-data client. The existing EDS bridge may remain temporarily while the new provider is built,
but it is no longer the architectural authority. Android likewise migrates from its legacy custom
Cloudflare API to the versioned Foyer contract.

ADR 0007's Foyer Shell database remains authoritative for application-owned desktop state. The sync
engine owns a different SQLite database with independent migrations and connection lifetime; it
publishes normalized immutable snapshots to views and may be deleted and rebuilt from the server.

The server, PostgreSQL, reverse proxy, sync engine, DAV service, background worker, and backup job
are deployable with Docker Compose on one personal VPS. Only the reverse proxy exposes public ports.
Mail submission uses Purelymail's authenticated submission service rather than running an SMTP
delivery server on the VPS.

The exact sync engine, DAV implementation, authentication implementation, embedding model, object
storage, and backup target require bounded validation before their production selections are
recorded. They must remain replaceable behind the contracts above.

## Alternatives and deliberate exclusions

- Continuing with EDS would keep desktop calendar reads working but would not provide shared
  Android data, notes, bookmarks, contacts, memory, or a Google-independent server authority.
- Rebuilding the deleted Cloudflare service would retain platform-specific state and the old memory
  model that is being replaced.
- A REST API without a sync engine would make Foyer implement generic replication mechanics as
  application business logic.
- Treating pgvector as canonical storage would make model-specific derived values authoritative and
  complicate re-indexing, deletion, and provenance.
- Synchronizing server credentials, provider tokens, raw database access, or embeddings to clients
  is excluded.
- Operating an SMTP delivery server on the VPS is excluded; Purelymail remains the mail provider.

## Consequences and risks

Android and Foyer Shell gain one normalized personal-data substrate and can operate from local
replicas while offline. Server-side retrieval can span permitted mail, notes, bookmarks, agenda,
contacts, Activities, and artifacts without injecting raw histories into every agent turn.

The server becomes a sensitive single-user trust boundary containing personal content and provider
credentials. It requires TLS, per-device revocation, least-privilege service credentials, encrypted
off-host backups, restore tests, retention, bounded ingestion, audit records, and visible degraded
states. Self-hosting transfers availability and upgrade responsibility to the product owner.

Sync does not eliminate semantic conflicts. Notes, recurrence, contact fields, task completion,
deletion, and concurrent client writes require explicit policies and fixtures. The selected engine's
Rust and Android client maturity must be proven before it becomes a production dependency.

## Validation criteria

- Android and Foyer Shell can rebuild their personal-data replicas from the same user-scoped server
  state without database credentials.
- Offline operations retry idempotently and cannot mutate another user, replace their arguments
  after approval, or resurrect a tombstoned item silently.
- Calendar recurrence, tasks, contacts, and mail remain interoperable through their provider
  protocols while Foyer consumes normalized records.
- Purelymail IMAP ingestion and SMTP submission failures degrade mail only and never report delivery
  without a provider result.
- Deleting or changing a source invalidates its derived chunks, and every search result links to a
  real source revision and location.
- Foyer Shell-owned SQLite state and sync-engine replica tables remain in separate ownership and
  migration domains.
- PostgreSQL, DAV credentials, IMAP/SMTP credentials, embedding workers, and backup administration
  are unreachable from GPUI rendering and model tools.
- A Compose deployment restarts individual services without making PostgreSQL publicly reachable,
  and encrypted off-host backups pass a restore test.

## Supersession

This ADR supersedes ADR 0014's selection of Evolution Data Server as the agenda provider. It retains
ADR 0014's normalized domain/controller boundary and prohibition on direct model/provider access.
It extends ADR 0007 with a separately owned replaceable client replica and implements the connected
personal-data direction in the product vision.

ADR 0022 selects Radicale as the standards-capable CalDAV/CardDAV authority left open here and defines
the rebuildable projection boundary used by Foyer Server and PowerSync clients.

ADR 0023 selects manually enrolled device signing keys for production authentication. ADR 0024
selects portable, locally encrypted canonical bundles for S3-compatible off-host backups.
ADR 0025 selects CI-published OCI images and explicit operator promotion for production delivery.
