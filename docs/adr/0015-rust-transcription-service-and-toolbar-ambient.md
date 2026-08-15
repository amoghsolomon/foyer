# ADR 0015: Rust transcription service and audio-reactive toolbar ambient

- **Status:** Accepted
- **Date:** 2026-08-14
- **Owners:** Foyer Shell project

## Context

The previous transcription prototype split microphone capture and NVIDIA Parakeet inference into a
Python/NeMo service while a GNOME Shell extension owned its hotkey, centered transcript preview,
clipboard, paste gesture, and notification. Foyer Shell now runs on Niri, and its persistent toolbar and
ordinary UI surfaces are owned by the GPUI desktop process.

Dictation must become useful to more than one hotkey workflow. Future agent tasks need to request
speech recognition without taking ownership of the microphone, loading another 600-million
parameter model, or learning a UI-specific protocol. The existing software-engineering 6-gram
language model is also valuable: replacing the Python runtime must not silently reduce recognition
quality by dropping it.

The old 6-gram is a NeMo `NGramGPULanguageModel` checkpoint. Its runtime representation is a suffix
tree with token arcs, backoff arcs, and natural-log weights. That representation is simple and
stable, but the `.nemo` container embeds PyTorch serialization and is not an appropriate Rust
runtime contract. The Parakeet Unified ONNX encoder and recurrent decoder are usable from Rust,
but the upstream Rust decoder is greedy and does not apply the 6-gram scores.

## Decision

### Service and reusable boundary

Foyer Shell will add an independently supervised `foyer-shell-transcription` Rust user service. It owns the
default microphone, in-memory audio for the active recording, one lazily loaded Parakeet Unified
ONNX model, and one 6-gram scorer. It exposes a versioned, typed session-bus interface at
`org.amazity.FoyerShell.Transcription1` and `/org/amazity/FoyerShell/Transcription1`.

Clients start a named channel and receive an opaque session ID. They stop or cancel only that
session. The first channel is `dictation`; later tasks may introduce channels with different
post-processing or authorization policy without loading another recognition model. The service
publishes monotonically versioned snapshots containing the session, state, bounded waveform
telemetry, result, and error. Waveform data is display telemetry, not an audio transport.

Only one microphone session may be active initially. Captured samples and decoder state remain in
memory and are discarded after completion or cancellation. The service does not upload or persist
audio or transcripts.

### Rust inference and 6-gram fusion

The runtime uses Parakeet Unified ONNX sessions from Rust. During recurrent transducer decoding it
computes the same backed-off full-vocabulary score vector represented by NeMo's suffix tree and
adds `alpha * lm_score` to each non-blank acoustic logit before selecting a token. The initial alpha
is `0.2`, matching the previous service. Blank logits are not modified and the language-model state
advances only when a non-blank token is emitted.

The runtime reads a small Foyer Shell-owned binary format containing validated fixed-width arrays for
the suffix tree. A migration tool converts the trusted existing `.nemo` checkpoint into that
format. This converter may use the existing NeMo/PyTorch environment because it is an offline
asset-migration step; neither Python, PyTorch, nor NeMo is part of the installed service or request
path. The binary header records format version, vocabulary size, order, state count, and arc count.
Malformed or mismatched models fail closed rather than falling back to un-fused recognition.

Model files remain external data under the user's XDG data directory and are not committed to the
repository. Configuration may override their locations for development and tests.

### Hotkey, paste, and presentation

Niri binds `Super+Alt+R` to the semantic command `foyer-shell transcription toggle`. The first
press starts the `dictation` channel. The second press stops that same session and begins
recognition. A completed non-empty transcript is copied as UTF-8 and pasted into the application
that retained focus. Foyer Shell then creates a normal notification whose body shows the pasted text.
Paste failure produces an error notification and is never reported as success.

There is no transcript popover, centered preview, confirmation surface, or editable intermediate
result. Completion uses the existing notification surface and persistence path from ADR 0004 and
ADR 0007.

The existing ambient band inside the stationary GPUI right toolbar is the only recording visual. It
is fully hidden while transcription is inactive. During recording and recognition it consumes the
latest bounded waveform samples and RMS level, so recorded audio changes its shape and energy
rather than merely toggling a canned animation. Visibility, RMS, and waveform samples use bounded
attack and release smoothing: the band builds gradually when capture begins and sinks gradually
after completion, cancellation, or error. It never opens another window and never displays
transcript text.

## Alternatives and deliberate exclusions

- Keeping the Python/NeMo service preserves its decoder but retains a large interpreter and
  framework runtime in the interactive path and does not establish a reusable Rust boundary.
- Using upstream Rust greedy decoding without the 6-gram is excluded because it silently discards
  a requested and previously deployed accuracy feature.
- Sending raw audio over D-Bus is excluded. It expands the privacy and memory boundary and makes
  visualization consumers accidental audio owners.
- Rendering the old centered transcript preview is excluded. The focused application plus the
  completion notification are the only dictation result surfaces.
- Opening a separate visualizer process or Wayland toplevel is excluded. Transcription reuses the
  established toolbar ambient and must not add another surface to the compositor overview.
- Running inference in `foyer-shell` is excluded because model initialization, GPU failure, or
  a long utterance must not block or terminate the persistent desktop shell.
- Arbitrary shell commands in the D-Bus or local control protocol are excluded. Channels and
  actions remain fixed semantic values.

## Consequences and risks

The model is loaded once and can be shared by later task-specific clients. Microphone ownership,
privacy, state transitions, and language-model behavior have one testable implementation. The
desktop retains control of user-visible paste, notification, and toolbar presentation policy.

The one-time ONNX and 6-gram conversion requires substantial temporary disk space. Quantized CPU
execution is the portable default. CUDA is opt-in and depends on a compatible ONNX Runtime
provider; model load errors must be visible and must not degrade to an un-fused recognizer.

Synthetic key injection is a privileged desktop capability. The implementation is limited to the
fixed Shift+Insert gesture after a successful clipboard write and does not expose key codes or
commands through IPC. A dedicated `foyer-shell-ydotoold` systemd user service owns that capability. It
runs in keyboard-only mode, uses a mode-`0600` socket under the user's runtime directory, and
relies on the session user's existing `/dev/uinput` access rather than a root daemon. The desktop
service depends on it and receives the private socket path through `YDOTOOL_SOCKET`.

## Validation criteria

- The installed service and request path contain no Python, PyTorch, or NeMo process.
- Starting a second session while one is active is rejected; stale session IDs cannot stop a newer
  recording.
- Audio remains in memory and is released after stop, cancel, success, and failure.
- A known token sequence produces Rust 6-gram scores and next states equal to the exported NeMo
  reference fixture within floating-point tolerance.
- Decoder tests prove that LM scores can change a non-blank choice, never modify blank, and advance
  LM state only for emitted tokens.
- The toolbar ambient is invisible while idle, changes when supplied different waveform fixtures,
  fades in on start, and fades out after the active state ends without opening another window.
- First hotkey press starts capture; the second stops it; successful text is pasted once and shown
  in one notification with no preview surface.
- `foyer-shell-ydotoold.service` is active, its runtime socket is owned by the session user with mode
  `0600`, and the desktop process points `YDOTOOL_SOCKET` at that socket.
- Service, model-load, microphone, clipboard, and paste failures leave Foyer Shell responsive and create
  actionable error state.

## Supersession

This ADR extends ADR 0004's notification path, ADR 0007's persistence path, and ADR 0006's Toolbar.
It supersedes ADR 0006 only where that record says the ambient band does not encode service state:
the band now represents an active transcription session using bounded waveform telemetry.

ADR 0018 supersedes this record's former Shell and rail namespaces with Foyer Shell and Toolbar.
