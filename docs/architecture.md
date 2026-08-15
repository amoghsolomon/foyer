# Foyer Shell Presentation architecture

This document describes Foyer Shell's Presentation subsystem. A Presentation is a read-only,
semantic, narrated view within Workspace 1; it is not the whole desktop environment. No model
operates pixel-level drawing tools.

## Pipeline

```text
user request
    |
    v
Luna xhigh reasoner -- bounded read-only project/SearXNG tools --> evidence briefing
    |                         |
    | observable events      | completed evidence
    v                         v
Luna low/priority status   Luna low/priority director
narrator                    (no tools, semantic content only)
    |                         |
presence text + speech        v
                       semantic slide stream
                                |
                                v
                   deterministic Rust compiler
                   ids / validation / focus / cues
                                |
                                v
                     GPUI renderer + audio clock
```

The repository-owned Node sidecar imports Pi's SDK directly and never starts Pi's TUI, CLI, or RPC
mode. Its dependency versions are locked in `sidecar/package-lock.json`; no global Pi installation
participates. Each role has an isolated in-memory session, prompt, history, tool set, and thinking
level, while sharing the linked Codex authentication runtime.

Development resolves the sidecar from the active workspace. Installed Foyer Shell resolves the packaged
copy below the XDG data directory (`foyer-shell/sidecar`); `FOYER_SHELL_SIDECAR_ENTRYPOINT` may select an
explicit deployment path. The bridge validates that entrypoint before spawning Node, and startup
failure is returned as a terminal public event so the player cannot remain in a false running state.

The reasoner runs Luna at xhigh effort. It may inspect the active project through root-confined
read-only tools or query local SearXNG when current references or an image materially help. A hard
budget permits eight total tool calls and no more than two web calls. Its output is an internal
evidence briefing, not a slide plan.

The director runs Luna at low effort with OpenAI priority service tier. It receives the user request
and the evidence briefing as untrusted factual material, has no tools, and streams semantic slide
records as soon as each is authored. It chooses content, narrative progression, slide axes, and
semantic compositions; it does not choose pixel geometry, animation timing, typography, focus cue
offsets, or final identifiers.

The status narrator also runs Luna low with priority service tier. It sees only a bounded ledger of
observable phase changes, tool names, arguments, and public results—never private reasoning. It may
emit a few short, grounded spoken updates before the first slide arrives. Deterministic visual-only
status text updates immediately between those utterances, and all disposable presence stops when
the presentation begins.

## Presentation compiler

The native compiler in `crates/presentation` is the trust boundary between model-authored semantics and the
renderer. It:

- normalizes slide axes and composition precedence;
- creates stable ids and repairs missing or duplicate ids;
- validates bento blocks, chart/tree payloads, graph topology, files, and code ranges;
- derives graph traversal and code walkthrough order;
- rebuilds narration focus targets and character-offset cues in visual reading order; and
- clamps all payload sizes to renderer-supported limits.

This keeps the director prompt small and makes malformed or partially authored records safe and
deterministic. The renderer never relies on the model to focus the right object at the right time.

## Layout and interaction

Bento slides contain 1–7 semantic cards. Rust selects a layout from a varied set of bounded 9×9
templates using stable slide identity. Every template preserves authored left-to-right,
top-to-bottom reading order; no mirrored variant can reverse narration order. Cards never receive
model-authored coordinates.

Graphs occupy the full content area and always flow left to right. Rust validates topology and
traversal while GPUI owns layering, camera fitting, drag/pan interaction, smooth Bézier routing,
arrowheads, focus dimming, and follow-path animation. Code compositions occupy the full slide and
follow their compiled semantic line-range order.

Only outgoing and incoming slides render during transitions. Slides are immutable after
publication, and the minimap/arrow keys navigate already-presented material.

## Synchronization

For narrated beats, the audio sample position is the presentation clock. Character anchors resolve
to cue positions within prepared audio, whose exact duration is authoritative. Seeking or pausing
audio therefore seeks or pauses matching visual cues.

Local speech synthesis comes from the independently supervised shared Chatterbox Nano service.
It owns one warm GPU model and prepared voice while presentation clients request bounded PCM over
D-Bus. OpenRouter remains an explicit remote development option. Synthesized beats are pipelined
through a bounded prepared-audio queue while one persistent CPAL stream plays the current buffer.
Each beat publishes a device-clock `PlaybackStarted` event, so matching visual focus cannot run
early. Other native clients, including the future system agent, may reuse synthesis without owning
the presentation timeline.

The renderer accepts immutable compiled values and only interpolates prepared visual state at frame
time. Pi sessions, filesystem and web tools, compilation, speech synthesis, and persistence all run
away from the render thread.
