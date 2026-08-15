# Foyer Shell product vision

## Status of this document

This document records the current product direction for Foyer Shell. It is intended to give independent
implementation tasks enough shared context to work in parallel without turning the system into a
collection of unrelated agent demos.

The architecture is not frozen. Interfaces should be designed so individual services and model
providers can change without redefining the user experience.

## Product thesis

Foyer Shell is an agent-native primary desktop environment built with GPUI on top of the Niri Wayland
compositor. It is not a chat application, a conventional desktop with an assistant panel, or an
agent that happens to have unrestricted mouse and keyboard control.

The user expresses an intent, normally through local speech transcription or text. A persistent
main agent built with the Pi SDK understands the intent, uses bounded tools, delegates focused work
to task agents when useful, and returns either a direct result, a desktop action, or a deliberate
visual presentation. Foyer Shell is the visual and interaction layer that makes this work understandable
without making every agent action interruptive.

The core rule is:

> The model authors intent and semantics. Deterministic native code owns permissions, execution,
> layout, animation, focus, workspace policy, and recovery.

This extends the boundary already proven by the current presentation prototype: a reasoner gathers
evidence, a director authors semantic content, Rust compiles and repairs it, GPUI renders it, and
audio supplies an authoritative presentation clock.

## Existing foundations

### Foyer Shell presentation prototype

This repository already contains a working native presentation path:

- Pi SDK sessions for reasoner, director, and status roles;
- bounded project and SearXNG tools;
- semantic slide, graph, code, chart, tree, and narration contracts;
- a deterministic Rust presentation compiler;
- GPUI rendering and navigation; and
- synchronized synthesized narration.

This deliberate presentation system remains important, but it is only one mode of the future desktop. It
should not be invoked for ordinary operations such as opening a file, checking mail, or adding a
calendar event.

### Local transcription

`/home/user/Projects/personal/transcription-shell` contains a functional local Parakeet
transcription service with VAD, technical-term biasing, a glossary, and a D-Bus boundary. The
GNOME-specific extension is replaceable; the inference service should remain an independent
process and become an input provider for Foyer Shell.

The desktop must distinguish two voice intents clearly:

- **Dictation:** insert the transcription into the focused application.
- **Agent command:** submit the transcription to the persistent main agent.

The gestures, sounds, and visual states for these modes must make the destination unambiguous.

### Foyer Android

`apps/android` is the first-party Foyer Android launcher. Its constrained mobile UI is not the desktop
design, but both clients share several product concepts:

- a persistent personal agent;
- a calm daily briefing;
- durable Activities with isolated histories;
- scheduled definitions and independent runs;
- retrieval across Activities instead of injecting every history into every turn; and
- immutable, short-lived pending actions that execute only after explicit confirmation.

## Desktop model

### Niri and Workspace 1

Niri owns compositor mechanics: outputs, input, windows, focus, workspaces, and spatial movement.
Foyer Shell is the primary desktop environment presented through Niri.

Workspace 1 is reserved for Foyer Shell:

- one maximized Foyer Shell toplevel occupies the usable area beside the persistent right-edge
  Toolbar;
- that toplevel shows either Overview or Presentation;
- Ordinary application windows cannot be placed there.
- Workspace 1 is the agent's home and the user's stable return point.
- Other workspaces contain normal applications and visible agent work.
- Foyer Shell observes workspace and focus state but does not replace Niri's compositor responsibilities.

The first Niri integration must retain an emergency terminal shortcut, a recovery path when Foyer Shell
crashes, and the ability to choose the existing GNOME session at login until the environment is
reliable.

### Overview

Overview is the default view within Workspace 1. When no Presentation is active, it provides a
spacious, calm home rather than a dense widget dashboard. It may show:

- a brief welcome or current-context statement;
- the next relevant agenda items;
- active agents and background operations;
- pending approvals;
- recently completed meaningful work; and
- pinned Activities, files, or artifacts.

Overview can also host transient state. While the agent searches, indexes, converts, or coordinates
work, Foyer Shell can show restrained semantic animations. These disappear when they are no longer
useful instead of accumulating as permanent UI.

### Levels of interruption

The agent should propose an appropriate presentation level, while application policy has final
authority:

1. **Silent:** perform a safe, unsurprising operation without changing context.
2. **Ambient:** show lightweight activity when Workspace 1 is already visible.
3. **Notify:** expose a result or question beside the right toolbar without switching workspaces.
4. **Offer a context switch:** ask whether the user wants to move to Foyer Shell or to a result.
5. **Direct:** play a deliberate compiled visual presentation.
6. **Take over:** switch workspaces or control an application only within an explicitly permitted
   action.

The director is reserved for complex explanations, comparisons, walkthroughs, and durable Presentations.
It must not appear merely to dramatize ordinary tool calls.

## Agent model

### Persistent identity, bounded context

There is one persistent main Pi agent with a durable identity. Persistence does not mean retaining
one infinitely growing model conversation. Application-owned storage is authoritative, and each
turn receives a bounded context assembled from relevant state.

Memory should be separated by purpose:

- **Profile memory:** stable user preferences, environment facts, and interaction conventions.
- **Semantic memory:** curated facts learned from completed work.
- **Activity history:** full messages, tool calls, approvals, results, and failures.
- **Agenda state:** tasks, reminders, calendar items, and scheduled Activities.
- **Artifact catalog:** files, reports, recordings, and compiled presentations.
- **Procedural memory:** reusable tools, skills, and learned workflows.

Only a small curated profile should be present on every turn. The agent retrieves other memories,
Activities, and artifacts when relevant. Raw histories are never silently concatenated into the
main context.

### Activities

An Activity is a durable objective or conversation beneath the main agent. It can contain:

- an isolated conversation history;
- a saved, versioned job definition;
- an optional schedule;
- independent executions and results;
- child task-agent runs; and
- produced artifacts or presentations.

Interactive conversation, scheduled execution, and a task-agent run are distinct concepts. A
schedule executes a saved immutable definition, not whichever message happens to be most recent.

### Task agents

The main agent can delegate focused tasks to temporary agents. Task agents:

- begin with fresh, explicitly supplied context;
- have a clear objective and bounded iteration/resource budget;
- receive only a subset of the parent's capabilities;
- cannot grant themselves or descendants additional capabilities;
- cannot write shared long-term memory directly;
- report structured progress and a final result to the main agent; and
- may receive a visible Niri workspace when browser or desktop interaction should be inspectable.

Visible task agents should have a stable identity, objective, status, workspace, and controls to
pause, take over, resume, or terminate them. Visiting their workspace must not itself interrupt
their work.

Durable work that must survive a main-agent turn or process restart should be represented as an
Activity run, not only as an in-memory child-agent call.

## Capability and permission model

Pi and its task agents must not receive unrestricted Niri IPC, shell, filesystem, input, or account
credentials. They request semantic operations from a native capability broker. Example operations
include:

```text
search_files(query, filters)
present_results(items)
open_artifact(path, preferred_app)
offer_workspace_switch(reason, target)
launch_visible_task(agent_id, application)
inspect_accessibility_tree(window_id)
record_window(window_id, options)
send_email(draft_id)
create_calendar_event(event)
```

The broker owns validation, authorization, credentials, workspace policy, execution, audit records,
and recovery. Tool output and external documents are untrusted data and can never authorize later
actions.

### Permission classes

Initial policy should distinguish at least:

- read-only local inspection;
- reversible writes inside a granted task scope;
- external communication and account mutations;
- desktop control and recording;
- deletion, publishing, purchases, credentials, and system configuration; and
- unattended or scheduled execution.

Read-only operations should normally proceed without interruption. Consequential actions require a
Foyer Shell-owned approval according to policy. Particularly destructive actions must remain explicit
even if similar actions were approved previously.

### Approval flow

An approval request contains an immutable normalized action:

```text
action id
requesting agent and Activity
exact operation and arguments
affected resources or recipients
human-readable consequence
reversibility
permission class
creation and expiry time
```

Foyer Shell presents the request through spoken audio and/or buttons beside the right toolbar. Approval may be given
by button, keyboard, or a voice response captured during the active approval window. An unrelated
ambient "yes" must never grant permission.

Approval executes the exact stored action once. The agent cannot replace its arguments after
confirmation, approvals expire, and timeout or ambiguity fails closed. Completion and failure are
added to the audit history.

Scheduled work operates inside an explicit saved capability envelope. If it encounters an action
outside that envelope, it pauses or denies the action rather than implicitly escalating.

## Tools and services

The goal is a capable general desktop agent. Expected capabilities include:

- local file watching, metadata extraction, lexical search, and vector search;
- PDF parsing and generation;
- office document reading, conversion, and editing;
- image, audio, and video inspection and transformation, including FFmpeg;
- SearXNG and web retrieval;
- email, calendar, reminders, and tasks;
- isolated browser automation;
- application launching and Niri workspace operations;
- accessibility-tree inspection and control;
- screen or window capture and demo recording; and
- scheduling and background execution.

Desktop control should prefer semantic accessibility interfaces, then application-specific
adapters, with screenshot-based visual control as a fallback. Browser agents should use isolated
profiles or contexts and may be exposed in dedicated visible workspaces.

Tool breadth must not produce different permission and interaction behavior for every integration.
All tools use the shared capability, approval, event, and artifact protocols.

## File intelligence

File intelligence is a foundational service rather than a single agent tool. It should:

- watch configured roots and update incrementally;
- preserve file identity across ordinary renames where possible;
- extract text, structure, metadata, thumbnails, and provenance;
- support exact, lexical, metadata, and semantic retrieval;
- understand common source, PDF, office, image, audio, and video formats;
- exclude secrets and user-configured paths;
- make every result traceable to a real file and extracted location; and
- expose opening a result at the most relevant location when the target application supports it.

A representative interaction is: "Find the document where I discussed deployment constraints."
The agent searches, inspects likely matches, presents compact evidence when useful, and can open the
selected artifact in another workspace.

## Durable compiled presentations

Complex, deliberate presentations produced by the existing Foyer Shell pipeline are first-class durable
artifacts. Ordinary operational UI and simple search results are not automatically saved as presentations.

A Presentation should be persisted as a filesystem bundle similar to:

```text
presentations/<presentation-id>/
├── manifest.json
├── source-events.jsonl
├── compiled-presentation.json
├── evidence.json
├── narration/
├── assets/
└── thumbnail.webp
```

The exact schema remains to be designed. It must retain the originating Activity, request, semantic
source, evidence provenance, role/model metadata, compiler version, assets, and narration timing.

Requirements:

- A completed presentation replays deterministically without another model call.
- Presentation titles, narration, blocks, and evidence can be indexed and retrieved later.
- Reopening a presentation does not mutate its historical contents.
- Asking for a section to be explained again creates a linked supplementary or revised presentation using
  the retained source and evidence.
- Schema/compiler migrations are explicit; old sessions remain inspectable or renderable through a
  compatibility path.

While a Presentation plays, it replaces Overview within the Workspace 1 host. Exiting it restores
the preserved Overview state.

## Runtime and recovery boundaries

The eventual runtime should separate at least these responsibilities, even if some initially share
a process:

- **Niri:** compositor and spatial mechanics.
- **Foyer Shell UI:** GPUI Workspace 1 host, Overview, Presentation, Toolbar, Panel, notifications,
  and approvals.
- **Agent daemon:** Pi sessions, main-agent lifecycle, delegation, Activities, and scheduling.
- **Capability broker:** permission evaluation and privileged action execution.
- **State store:** authoritative events, Activities, memory metadata, audit history, and indexes.
- **Transcription service:** microphone, VAD, Parakeet inference, and transcript events.
- **Index service:** filesystem observation, extraction, and search.
- **Workers:** browser, document, media, and other isolated tool execution.

Failure expectations:

- Niri and a recovery terminal remain usable if Foyer Shell exits.
- Foyer Shell can restart without cancelling agent work.
- The agent daemon can restart and reconstruct Activities, approvals, and durable runs.
- A crashed tool worker cannot corrupt the main event store.
- Pending approvals fail closed across ambiguous restarts.
- Long-running work reports heartbeats and supports cancellation.
- Rendering and model/tool work never block GPUI's render thread.

## Shared system protocol

Parallel work should converge on a versioned protocol rather than communicate through UI-specific
assumptions. The protocol needs event families for:

- transcription and input intent;
- main-agent turns and streamed public output;
- observable tool phases and progress;
- task-agent spawn, progress, result, cancellation, and recovery;
- capability requests and results;
- approval requests and decisions;
- Niri workspace/window observations and requested transitions;
- notifications and context-switch offers;
- Activity and schedule lifecycle;
- artifacts and index updates; and
- deliberate presentation authoring, compilation, playback, and persistence.

Private chain-of-thought is never a protocol payload. Status UI is derived from observable state,
declared intent, tool arguments safe for display, and public results.

Every durable event should have stable identity, timestamp, owning Activity/task, schema version,
and enough idempotency information to survive retries.

## Representative end-to-end workflows

The system should be tested across all major capability classes through narrow complete workflows:

1. **Retrieve:** find and open a local document at a relevant passage.
2. **Transform:** convert an office document or edit media and return the artifact.
3. **Connected action:** inspect email/calendar and complete one approved write.
4. **Browser:** delegate research to an isolated, optionally visible browser workspace.
5. **Desktop control:** launch, inspect, exercise, and record an application through accessibility
   with visual fallback.
6. **Proactive work:** execute a scheduled Activity and surface its result.
7. **Deliberate presentation:** compile, narrate, save, retrieve, and replay a complex Foyer Shell presentation.

These are not a claim that only one workflow matters. Together they exercise the shared substrate:
memory, retrieval, delegation, permissions, visibility, workspace control, interruption, durability,
artifact handoff, and failure recovery.

## Parallel workstreams

The following tasks can proceed independently as long as they preserve the boundaries above.

### 1. General Foyer Shell and Overview

Own the GPUI application shell, Toolbar, Workspace 1 state, Overview, visual task/approval
representations, navigation, settings, and Presentation entry/exit. Do not embed agent execution
or privileged Niri actions in view code.

### 2. Niri desktop integration

Own session startup, application/window rules, Workspace 1 reservation, focus/workspace observation,
semantic workspace commands, visible agent workspaces, crash recovery, and a safe bootstrap from the
current GNOME environment.

### 3. Persistent Pi agent and Activities

Own the agent daemon, bounded context assembly, Activity/event persistence, durable identity,
retrieval interfaces, scheduling, task-agent delegation, cancellation, restart recovery, and public
progress events. Reuse Pi through an explicit SDK/process boundary rather than binding desktop state
to a particular CLI session.

### 4. Capability and permission broker

Own typed semantic operations, policy evaluation, immutable pending actions, approval lifecycle,
credential boundaries, audit history, scheduled capability envelopes, and safe execution adapters.
The broker, not the model or UI, is the authority for side effects.

### 5. Transcription integration

Own reuse of the Parakeet D-Bus service, Niri/Foyer Shell hotkeys, recording indicators, dictation versus
agent-command routing, active approval capture, audio-device behavior, and failure recovery. Keep
inference outside the GPUI and compositor processes.

### 6. File indexing and artifact services

Own filesystem observation, extraction pipelines, lexical/vector indexes, provenance, result
opening, secret/path exclusions, and the artifact catalog. Provide stable semantic APIs rather than
exposing database-specific queries to the model.

### 7. Browser and desktop-use workers

Own browser-profile isolation, visible task workspaces, accessibility inspection, input/control
adapters, capture/recording, pause/takeover/resume, and safe worker isolation. All side effects still
flow through broker-issued capabilities.

### 8. Durable deliberate presentations

Own session bundle schemas, evidence and asset storage, deterministic replay, thumbnails, indexing,
compiler compatibility, and linked follow-up presentations. Preserve the current semantic
director/compiler boundary.

### 9. Shared protocol and state store

Own versioned message contracts, event identities, idempotency, durable storage, migrations, and
integration fixtures. This workstream should coordinate schemas rather than absorb the business
logic of every service.

## Non-goals and guardrails

- Foyer Shell is not a model-operated pixel canvas.
- Niri is not replaced by a custom compositor.
- The main agent does not receive arbitrary host authority merely because the desktop is personal.
- Persistent memory is not an unbounded transcript injected on every turn.
- Task agents do not silently inherit all secrets or gain new permissions.
- Ordinary actions do not automatically trigger narrated presentations.
- Overview does not become an attention-hungry feed or widget grid.
- Browser screenshots are not the default interface when semantic browser or accessibility data
  exists.
- A model response is never itself proof that an external action succeeded.

## Near-term integration target

The first integrated milestone should demonstrate the product loop, even if each tool is narrow:

1. Start a recoverable Niri session with Foyer Shell reserved on Workspace 1.
2. Invoke local transcription in agent-command mode.
3. Submit the transcript to a persistent Pi agent.
4. Search an indexed local document collection read-only.
5. Show transient search state and grounded results in Overview.
6. Offer a context switch and open the chosen file in another workspace.
7. Request an immutable approval for one representative write action.
8. Approve or cancel it by a toolbar-adjacent action and by bounded voice capture.
9. Run one visible browser task agent in a dedicated workspace.
10. Produce, save, retrieve, and deterministically replay one deliberate narrated presentation.

This milestone is broad intentionally: it validates the common desktop-agent architecture before
the tool catalog expands.
