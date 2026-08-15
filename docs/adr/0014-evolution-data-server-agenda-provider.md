# ADR 0014: Evolution Data Server agenda provider

- **Status:** Superseded
- **Superseded by:** ADR 0020
- **Date:** 2026-08-14
- **Owners:** Foyer Shell project

## Context

Foyer Shell reserves a stable Agenda section and the product vision requires calendar and task state to
be available to both native presentation and, later, the persistent agent. Google Calendar and
Google Tasks require account authentication, remote synchronization, offline caching, recurrence,
and conflict handling. Reimplementing those concerns inside Foyer Shell would add credentials and a
provider-specific synchronization engine to the trusted application.

Evolution Data Server (EDS) already provides public calendar and task APIs, Google backends,
D-Bus-activated services, and an offline cache without requiring GNOME Shell. Its public GObject
APIs are available through `libecal` and GObject introspection, but maintained Rust bindings are not
part of the current workspace and distribution development headers need not be installed on the
target machine.

Agenda reads also establish a future account-mutation boundary. The model must not gain raw EDS,
iCalendar, D-Bus, database, or credential access merely because native UI can display agenda data.

## Decision

Foyer Shell adds a `foyer-shell-agenda` crate that owns normalized calendar/task domain types, an immutable
snapshot, and a typed semantic controller. It runs independently of GPUI and publishes bounded
state through the same snapshot/update pattern used by other Foyer Shell services. The initial provider
is EDS.

The first implementation invokes a small, versioned GObject-introspection bridge with PyGObject. The
bridge uses only public `ESourceRegistry` and `ECalClient` APIs, discovers enabled calendar and task
sources, expands calendar instances in a bounded window, and emits normalized JSON. It is optional:
when Python, PyGObject, EDS, its user bus, or configured sources are absent, only Agenda becomes unavailable.
The Rust provider protocol is replaceable by a compiled native bridge without changing domain,
storage, presentation, or future capability contracts.

EDS remains authoritative for remote synchronization, cached components, source capabilities, and
credentials. Foyer Shell never reads EDS private cache files and never copies credentials into its own
database. `foyer-shell-storage` persists only Foyer Shell-owned agenda preferences, initially per-source
visibility.

The initial calendar Agenda and Tasks surfaces are read-only. Agenda shows calendar sources and
upcoming event instances. The toolbar slot reserved as Reminders by ADR 0006 becomes Tasks and shows
task-list sources and open tasks; reminder behavior can later be expressed within the Tasks domain
without mixing tasks into the calendar view. Future user and agent mutations must enter through one
capability-broker operation contract and then use the agenda controller. MCP or agent tools may
read normalized snapshots but must never call the provider bridge directly. Writes, deletion,
attendee notification policy, recurrence scope, idempotency, approval, execution, and audit will be
specified with the capability broker before mutation commands are exposed.

No WebKit view is embedded for normal operation. Account enrollment remains owned by a suitable
EDS/GOA account setup surface and is deliberately separate from agenda synchronization.

## Alternatives and deliberate exclusions

- Direct Google Calendar and Tasks clients would make Foyer Shell own OAuth tokens, provider-specific
  synchronization, caching, and conflict behavior and are excluded.
- Reading EDS SQLite or cache files would rely on private formats and bypass EDS consistency and is
  excluded.
- Adding agenda operations to the presentation reasoner sidecar would cross the wrong lifetime and
  trust boundary and is excluded.
- A permanently embedded Python, GJS, or WebKit runtime inside `foyer-shell` is unnecessary.
  PyGObject is an optional helper process in the first slice.
- Calendar and task mutation is deferred until the native capability broker can validate,
  authorize, approve, execute, and audit the normalized operation.

## Consequences and risks

Foyer Shell gains useful Google and local calendar/task data without owning credentials or a new sync
engine. The same typed agenda boundary can later serve native UI and broker-controlled tools.
Source visibility survives restarts while event and task bodies remain in EDS.

Python, PyGObject, and introspection metadata are runtime requirements for the first provider. Distributions may
package these separately, so unavailability must be clear and local to Agenda. Introspection API
shape and recurrence/timezone behavior require fixture and live-account validation. Polling is used
for the first slice; a persistent `ECalClientView` change stream may replace it behind the provider
boundary when lower latency is needed.

## Validation criteria

- With configured EDS sources, Agenda shows bounded upcoming event instances and Tasks shows open
  tasks from enabled sources without blocking GPUI rendering.
- All-day, timed, recurring, completed, date-only, missing-field, and multi-source objects normalize
  to stable typed records without exposing raw iCalendar.
- Source visibility is stored in Foyer Shell SQLite through typed storage commands and takes effect in
  the agenda snapshot after restart.
- Missing Python/PyGObject, missing EDS, user-bus failure, source failure, and malformed bridge output produce a
  bounded Agenda unavailable/error state without affecting other Foyer Shell services.
- No account credential or event/task body is written to Foyer Shell SQLite.
- The public agenda controller contains no unrestricted query, raw D-Bus, command execution, or raw
  iCalendar operation.

## Supersession

This record supersedes ADR 0006 only where that record reserves and labels the third utility toolbar
slot as Reminders. Its stable identity and placement are retained, but the shipped surface is Tasks.

ADR 0020 supersedes EDS as the long-term provider with the hosted Foyer Server and a replaceable
replicated client cache. The existing EDS bridge remains only as transitional implementation code
until that client is complete.
