# ADR 0016: Persist Presentations as Activity artifacts and replay them in Workspace 1

- **Status:** Accepted
- **Date:** 2026-08-14
- **Owners:** Foyer Shell project

## Context

The presentation prototype can stream semantic slides and synchronized narration, but its Pi role
sessions, compiled slides, and synthesized audio are process-local. The Workspace 1 Overview and the
presentation prototype also remain separate application roots. Consequently a Presentation cannot
survive restart, appear in Activities, or replay without another model and synthesis run.

The product vision already distinguishes durable Activities from internal model sessions and makes
compiled presentations first-class artifacts. ADR 0007 establishes one SQLite owner for durable metadata
while keeping large artifact bodies outside the database. ADR 0008 requires future presentations to render
inside the existing `foyer-shell-workspace` toplevel without covering the stationary right toolbar.

## Decision

Foyer Shell uses the following user-facing and durable identities:

- an **Activity** is a durable objective or conversation;
- a **run** is one execution beneath an Activity;
- a **Presentation** is an immutable compiled Presentation artifact produced by a run; and
- reasoner, director, and status Pi sessions remain internal and may be in-memory.

The native caller assigns stable Activity, run, and presentation identifiers. It records the
versioned public event stream at the Pi sidecar boundary. Restoring an internal Pi conversation is
never required for replay.

Each presentation is stored below the Foyer Shell XDG data directory as a filesystem bundle containing a
versioned manifest, public source events, the compiled presentation, retained narration WAV files,
evidence metadata, and an asset directory. Bundle contents are immutable after completion. Writes
use a temporary file followed by an atomic rename; incomplete bundles remain inspectable and are
catalogued with their actual state.

`foyer-shell-storage` owns the searchable Presentation catalog in the shared SQLite database. It stores
only bounded metadata and the bundle path. The presentation bundle remains authoritative for replay and
is re-indexed idempotently, so rebuilding catalog rows does not alter historical presentation contents.

The audio sample cursor remains the playback clock. The narration runtime exposes typed
pause/resume/stop controls, retains the exact normalized PCM used for playback, and emits cue and
position events from that clock. Replay-current-slide and replay-from-beginning create a fresh
playback run from retained audio and reset cue-derived visual state before playback resumes.

The Activities Toolbar section lists saved Presentations. Selecting one closes the Panel and asks
the existing `foyer-shell-workspace` to replay it. Overview moves left while Presentation enters
from the right inside that same toplevel; the Toolbar remains stationary. Audio begins only after
Presentation is installed. Exit reverses the transition and restores the preserved Overview.

## Alternatives and deliberate exclusions

- Persisting Pi SDK session files would couple history and replay to one model runtime and is not
  the Activity contract.
- Storing compiled presentations or audio blobs in SQLite would make the state database the large-artifact
  transport and is excluded by ADR 0007.
- Recording a screen video is not the canonical presentation. A video export may be derived later,
  but deterministic replay uses semantic presentation data, retained assets, cues, and narration.
- Opening a second presentation toplevel would violate the Workspace 1 workspace host ownership in ADR
  0008 and create avoidable focus and recovery behavior.
- Arbitrary timeline editing, presentation mutation, cloud sync, and indefinite compatibility through
  silent schema repair are excluded from the first implementation.

## Consequences and risks

Presentation schemas, compiler versions, and narration formats become durable compatibility contracts.
Bundles therefore carry explicit versions and fail visibly when unsupported. Retaining PCM WAV
files uses more disk than lossy audio; bounded retention and optional lossless compression can be
added without changing the artifact identity or playback contract.

The Presentation renderer becomes reusable inside `foyer-shell`. It must remain independent of
storage I/O and Pi execution: typed controllers load bundles and deliver immutable values before
the GPUI view renders them.

## Validation criteria

- A completed presentation remains listed after Foyer Shell restarts and replays without a model or TTS
  request.
- Source events, compiled slides, narration, request, identities, versions, and status are present
  in the bundle and catalog metadata is rebuildable from its manifest.
- Pause freezes narration position and visual cues; resume continues from the same position.
- Replay slide resets that slide and its narration; replay beginning resets the complete presentation.
- Selecting an Activity Presentation closes the Panel and plays it in `foyer-shell-workspace` while the Toolbar
  remains fixed.
- Exit restores the existing Overview entity and does not create or destroy a Niri workspace.
- Missing, partial, or unsupported bundles degrade only the selected presentation.
- No model, filesystem, database, synthesis, or audio decoding operation runs in a GPUI render
  callback.

## Supersession

This ADR extends ADR 0007's typed SQLite persistence and ADR 0008's Workspace 1 workspace host. It does not
supersede either record. It implements the durable-presentation and Activities direction in the product
vision while retaining the reasoner/director/compiler boundary in `docs/architecture.md`.

ADR 0018 supersedes this record's former explanation, scene, foyer, canvas, and rail terminology
with Presentation, Overview, workspace host, and Toolbar.
