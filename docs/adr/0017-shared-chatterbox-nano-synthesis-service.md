# ADR 0017: Share one Chatterbox Nano synthesis service across Foyer Shell clients

- **Status:** Accepted
- **Date:** 2026-08-14
- **Owners:** Foyer Shell project

## Context

Foyer Shell currently loads Kokoro inside each presentation narration runtime. That keeps the prototype
small, but it binds local synthesis to the slide narrator, duplicates a model if another process
needs speech, and does not provide the upcoming persistent system agent with a stable local voice
boundary. The system-wide agent, deliberate presentations, notifications that later gain spoken
delivery, and accessibility features should not each load or integrate a separate TTS model.

Local testing on the target six-core laptop found that Chatterbox Nano with the selected voice
reference is not a viable CPU replacement for Kokoro, but is comfortably faster than real time on
the RTX 2060. A warm long narration ran at approximately 0.24 real-time factor and used about
2.2--2.5 GiB of the 6 GiB GPU. The quantized Rust Parakeet service remains approximately 2.8 times
real time on CPU, so the two services can run concurrently without competing for the same primary
inference device.

ADR 0015 already makes Parakeet reusable: `foyer-shell-transcription` owns one model and exposes named
channels over `org.amazity.FoyerShell.Transcription1`. The `dictation` hotkey is only its first client;
the service is not owned by that UI gesture. Local synthesis needs the equivalent reusable
boundary, while preserving the presentation rule that the audio device sample cursor is the
authoritative visual clock.

## Decision

Foyer Shell adds an independently supervised `foyer-shell-tts` user service at
`org.amazity.FoyerShell.TextToSpeech1` and `/org/amazity/FoyerShell/TextToSpeech1`. It owns exactly one warm
Chatterbox Nano model and one prepared default voice profile. Requests include a bounded semantic
channel, text, narration style metadata, and style degree. The first channel is `presentation`;
later native agent and accessibility clients use their own fixed channel names without loading
another model.

The service serializes inference through one worker and returns bounded mono 24 kHz PCM16 bytes,
sample rate, and measured synthesis time. It does not own the speaker, global interruption policy,
or a presentation timeline. Callers own playback because deliberate presentations must retain their
exact CPAL device clock, cue resolution, pause/seek behavior, and immutable recorded narration.
The future agent audio controller may use the same synthesis client and playback primitives; it
must not bypass Foyer Shell policy merely because synthesis is shared.

Chatterbox's supported runtime is Python/PyTorch. A small repository-owned Python worker runs only
as a child of the Rust D-Bus service and communicates through a private framed stdin/stdout
protocol. The Rust process owns validation, D-Bus, child lifetime, bounded payloads, and failure
reporting. The Python environment is isolated below the Foyer Shell XDG data directory and installs a
pinned Chatterbox Git revision because the current package release does not yet expose Nano. Model
weights and the user-selected voice reference remain external data and are not committed.

CUDA is required for the Chatterbox default on the supported laptop. Failure to initialize CUDA,
load the model, load the voice reference, or produce valid PCM prevents the service from claiming
its D-Bus name. It never silently moves Nano to CPU, where measured latency is slower than real
time. Kokoro remains source-available only as an explicit development fallback during migration;
it is no longer the default installed Foyer Shell provider.

The protocol bounds channel names to 64 ASCII lowercase/digit/hyphen characters, input text to
4,096 Unicode scalar values, and returned PCM to 32 MiB. Synthesis audio is not persisted by the
service. A presentation that needs durable replay records the exact normalized PCM through the
existing presentation bundle path from ADR 0016.

## Alternatives and deliberate exclusions

- Loading Chatterbox separately inside Presentation playback and agent processes would duplicate
  several gigabytes of runtime state and couple model failures to unrelated processes.
- Keeping Kokoro as the local default is faster to load and much smaller, but retains the lower
  voice quality and narrator-only ownership that motivated this change.
- Moving Parakeet to CUDA would make both directions compete for the same 6 GiB GPU. Its current
  quantized CPU latency is sufficient, so CPU remains its supported default.
- Making the TTS service own all playback would centralize interruption policy prematurely and
  break the presentation's direct ownership of its sample-accurate visual clock.
- Returning paths to temporary WAV files would turn a shared filesystem directory into an IPC
  transport with cleanup and substitution races. The bounded D-Bus byte result is explicit.
- Arbitrary voice-reference paths in each request, unbounded cloning, model selection, and direct
  model access from agents are excluded. Voice profiles are deployment configuration, not request
  authority.

## Consequences and risks

All local clients share one expressive voice and one GPU allocation. Presentation code keeps its
existing playback and durable-recording semantics while replacing only synthesis. The future
system agent gains a typed local TTS boundary rather than depending on presentation internals.

The installed service now includes a substantial pinned Python/CUDA environment and model cache.
Startup is slower than Kokoro, so systemd starts the service with the graphical session and clients
report unavailability while it warms. The RTX 2060 has adequate measured headroom, but other GPU
consumers can still cause allocation failure; that failure remains visible and local to speech.

The Python worker protocol must tolerate library progress output without mistaking it for framed
PCM. Dependency upgrades require an explicit pin change and the same voice-quality, latency,
memory, malformed-output, restart, and concurrent-client validation.

## Validation criteria

- `foyer-shell-tts` loads one Chatterbox Nano model on CUDA, prepares the configured voice once, and owns
  `org.amazity.FoyerShell.TextToSpeech1` only after the worker reports ready.
- Presentation narration uses the service, preserves authored order, starts focus only from the
  CPAL playback event, and records the exact PCM used for durable replay.
- A second process can synthesize through a non-presentation channel while the same service and
  model remain resident.
- Chatterbox remains faster than real time during concurrent CPU Parakeet recognition on the
  supported laptop, without audio callback starvation or GPU exhaustion.
- Missing Python, dependencies, weights, voice reference, CUDA libraries, D-Bus, malformed worker
  frames, oversized text, oversized PCM, and worker exit produce bounded errors without taking
  down `foyer-shell` or `foyer-shell-transcription`.
- Explicit CUDA failure never falls back to Chatterbox CPU inference.
- No model inference, D-Bus call, file read, or synthesis operation runs from a GPUI render
  callback.

## Supersession

This ADR extends ADR 0015's reusable speech-service direction, ADR 0016's retained narration
artifacts, and `docs/architecture.md`'s audio-clock contract. It supersedes the local Kokoro default
described in `docs/architecture.md` and `crates/audio` without changing OpenRouter as an explicit
remote development option.

ADR 0018 supersedes this record's former Shell service namespace with Foyer Shell.
