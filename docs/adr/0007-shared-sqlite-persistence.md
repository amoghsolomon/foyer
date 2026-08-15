# ADR 0007: Shared SQLite persistence behind typed repositories

- **Status:** Accepted
- **Date:** 2026-08-14
- **Owners:** Foyer Shell project

## Context

Foyer Shell now needs durable notification history, and the product vision also calls for persistent
reminders, Activities, approvals, profile state, schedules, and artifact metadata. Letting each
surface invent its own file format or database connection would duplicate migration, retention,
failure, and concurrency policy. Letting GPUI views execute SQL would also move storage latency and
schema knowledge into the render boundary.

ADR 0004 deliberately excluded notification persistence while the notification protocol and
ambient presentation were being proven. That protocol is now stable enough for history to become
the first durable repository.

## Decision

Foyer Shell will use one application-owned SQLite database through the `foyer-shell-storage` crate. The
default path is `$XDG_DATA_HOME/foyer-shell/foyer-shell.sqlite3`, falling back to
`$HOME/.local/share/foyer-shell/foyer-shell.sqlite3`. `FOYER_SHELL_DATABASE_PATH` is an explicit development
and test override.

One storage worker thread owns the SQLite connection and serializes commands. Product code uses
typed controllers and immutable snapshots; GPUI views never receive a connection, execute SQL, or
perform filesystem I/O. The crate owns schema migrations, SQLite pragmas, bounded payloads,
retention, and unavailable-state reporting. SQLite is bundled through `rusqlite` so the deployed
schema does not depend on the host distribution's SQLite version.

The first schema version contains notification history and per-process notification sessions. A
session identifier disambiguates Freedesktop notification IDs, which restart from small integers
when the daemon process restarts, while still allowing `replaces_id` to update one durable record
inside a session.

Notification history will:

- store the sanitized application name, summary, body, urgency, receipt time, and read state;
- update and move a replacement notification to the newest position;
- retain at most 500 records and at most 30 days;
- remain after ambient cards expire or are evicted;
- mark an item read when the user dismisses its card;
- mark all items read when the history panel is opened; and
- expose typed delete-one and clear-all operations.

Future persistent features add typed repositories and migrations to this crate. They do not share
notification tables, expose a generic key-value API, or allow models and plugins to query the
database directly. Sensitive credentials and large artifact bodies remain outside this database;
the database may hold references and metadata only when their owning feature defines the contract.

## Alternatives and deliberate exclusions

- JSON or one-file-per-feature storage is simpler initially but lacks atomic migrations,
  concurrent read/write policy, indexes, and relational integrity for Activities and schedules.
- A database connection in `crates/desktop` would reduce code now but would couple GPUI rendering
  to schema and I/O.
- An ORM or asynchronous SQL runtime adds generated models and executor complexity before the
  schema warrants it. The typed repository boundary provides the useful abstraction.
- A standalone storage daemon would improve process isolation but adds IPC and supervision before
  multiple processes need database access.
- Notification actions, replies, images, search, cloud sync, and indefinite retention are not part
  of this slice.

## Consequences and risks

Foyer Shell gains one migration and concurrency boundary suitable for later durable product state. A
single worker keeps blocking SQLite work away from GPUI and makes snapshots deterministic. The
cost is a new foundational crate and an application database that needs careful forward-only
migrations and bounded growth.

Database corruption, unwritable data directories, or migration failures must not prevent the Toolbar,
Search, controls, or live notification cards from running. Storage publishes an unavailable
snapshot and retries opening the database. Commands received while storage is unavailable may be
dropped and must never block the UI.

## Validation criteria

- A notification remains in history after Foyer Shell restarts.
- Replacement updates one record within a daemon session, while a restarted daemon cannot overwrite
  an older session's record with a reused protocol ID.
- Opening notification history clears unread state, and dismissing a live card clears that item's
  unread state without deleting history.
- Delete-one and clear-all survive restart.
- Retention removes records older than 30 days and keeps no more than 500.
- Database work does not run from a GPUI render callback.
- An unavailable database degrades only persistent features and reports its state.

## Supersession

This ADR supersedes ADR 0004 only where it deliberately excluded notification persistence and
history. ADR 0004's protocol, sanitization, ambient-surface, focus, and expiry decisions remain in
force.

ADR 0018 gives the database its Foyer Shell filename and renames persisted explanation records to
Presentations. Existing data is migrated forward once; the persistence boundary decided here is unchanged.

ADR 0020 keeps this database authoritative for Foyer Shell-owned state while allowing a separately
owned, replaceable SQLite replica for hosted personal data. Sync-engine tables and migrations do not
enter the Foyer Shell database or its storage worker.
