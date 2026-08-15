# ADR 0022: Use Radicale as DAV authority for agenda and contacts

- **Status:** Accepted
- **Date:** 2026-08-15
- **Owners:** Foyer project

## Context

ADR 0020 requires a standards-capable CalDAV/CardDAV authority for calendar, tasks, and contacts,
while ADR 0021 validates PowerSync as the replaceable transport used by first-party clients. The
next base applications must not make Foyer Server reimplement iCalendar recurrence, VTODO, vCard,
DAV preconditions, or interoperability semantics. They also must not give DAV credentials to the
Android or Foyer Shell render processes or create separate client-specific authorities.

Radicale is a small self-hosted CalDAV/CardDAV server with explicit support for VEVENT, VTODO, and
vCard resources. Its official container supports pinned releases and persistent filesystem storage.
Radicale remains independently replaceable because Foyer consumes it only through DAV protocols
and standard resource formats.

## Decision

The Compose deployment pins Radicale 3.7.3 as the canonical authority for:

- task lists and tasks represented by CalDAV collections and VTODO resources;
- calendars and events represented by CalDAV collections and VEVENT resources; and
- address books and contacts represented by CardDAV collections and vCard resources.

Foyer Server owns a bounded DAV adapter and the service credential. Client applications never
receive that credential and never read Radicale storage. Semantic Foyer API commands validate the
authenticated user and operation identifier, translate the normalized command to a conditional DAV
mutation, and return only after Radicale accepts the authoritative write.

PostgreSQL stores rebuildable, user-scoped projections of DAV collections and resources. A projector
uses WebDAV sync tokens and ETags to discover both Foyer-originated and external DAV-client changes,
normalizes supported fields, and advances its checkpoint only after a complete projection
transaction. Projection rows are not a second authority and may be deleted and rebuilt from
Radicale. PowerSync replicates those projection rows to the separate Android and Foyer Shell replica
databases established by ADR 0021.

Conflicts are decided by DAV preconditions. Client commands carry the last observed ETag or normalized
revision; stale writes fail visibly and never overwrite a newer DAV resource. Stable iCalendar UIDs
and vCard UIDs identify resources independently of their current DAV href. Raw iCalendar and vCard
payloads remain inside the server/DAV boundary; versioned Foyer contracts expose normalized values.

Bookmarks remain Foyer Server/PostgreSQL canonical and use the same semantic-command and PowerSync
transport pattern as Notes. Mail and embeddings are outside the base-app milestone.

## Initial bounded feature surface

- Tasks include list membership, title, Markdown-capable description, due date/time, priority,
  completion state, and deletion. They preserve unknown VTODO properties during Foyer mutations.
- Contacts include structured display/name fields, multiple email addresses and phone numbers,
  organization, title, postal addresses, birthday, notes, and deletion. They preserve unknown vCard
  properties during Foyer mutations.
- Calendar includes multiple calendars, one-off and recurring events, all-day values, IANA time zones,
  location, description, recurrence rules, exclusions, and deletion. Recurrence expansion is bounded
  to the requested view window and preserves the authored master resource.
- Bookmarks include nested folders, URL, title, description, tags, favorite/archive state, ordering,
  and deletion. Only HTTP and HTTPS URLs are accepted in the initial slice.

## Alternatives and deliberate exclusions

- Making PostgreSQL authoritative for contacts, calendar, or tasks would require Foyer to own complex
  standards semantics and would break interoperability with ordinary DAV clients.
- Connecting Android and Foyer Shell directly to Radicale would duplicate DAV synchronization,
  credential, parsing, retry, and conflict logic in Kotlin and Rust.
- Treating PostgreSQL projections as writable canonical rows is excluded. They are written only by
  the projector after observing authoritative DAV state.
- Radicale filesystem access from Foyer Server is excluded; all reads and writes use CalDAV/CardDAV.
- Attendee scheduling, invitations, free/busy publication, shared collections, contact photos, and
  arbitrary WebDAV files are outside this initial milestone.

## Consequences and risks

Foyer gains standards-backed agenda and contact data while both clients retain one simple replicated
read model. Existing DAV clients can edit the same authority, and Foyer observes those changes on the
next sync-token reconciliation.

The server now contains a protocol adapter and projector that must preserve unknown properties,
handle invalid remote resources without stopping other collections, and avoid acknowledging a write
before its DAV precondition succeeds. Radicale data and PostgreSQL projection backups have different
recovery roles: Radicale is restored as authority; projections are rebuilt.

## Validation criteria

- Compose starts pinned Radicale with private networking and persistent storage; only the reverse
  proxy exposes production DAV endpoints.
- Standard DAV clients can create and edit VEVENT, VTODO, and vCard resources that appear in both
  Foyer replicas after projection.
- Foyer offline writes upload through semantic APIs, mutate Radicale conditionally, and appear in an
  independent DAV client without direct client credentials.
- Stale ETags, duplicate operation identifiers with different arguments, malformed remote resources,
  service outages, and projector restarts fail or recover without silent overwrite or checkpoint loss.
- Deleting all PostgreSQL DAV projections and checkpoints rebuilds equivalent normalized client state
  from Radicale.
- Calendar fixtures cover recurrence, exclusions, all-day values, daylight-saving transitions, and
  bounded expansion. Contact and task fixtures preserve supported and unknown DAV properties.
- No DAV request, XML/iCalendar/vCard parse, database operation, or sync work runs from a GPUI render
  callback.

## Supersession

This ADR selects the standards-capable DAV service left open by ADR 0020 and extends ADR 0021's
replicated-client pattern. It does not change the authority map in ADR 0020.
