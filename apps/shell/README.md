# Foyer Shell

Foyer Shell is an agent-native desktop environment built with GPUI on top of the Niri Wayland
compositor. It owns the visual and interaction layer while Niri remains authoritative for outputs,
workspaces, windows, focus, and compositor mechanics.

The product name is **Foyer Shell**. It lives in `apps/shell` within the Foyer monorepo; runtime
packages and identifiers use `foyer-shell`.

## Interface model

Workspace 1 is reserved for Foyer Shell. One maximized GPUI toplevel fills the usable area beside
the stationary right-edge Toolbar and shows one of two views:

- **Overview** is the default home view.
- **Presentation** is a narrated, deterministic view that replaces Overview inside the same
  toplevel. Leaving Presentation restores the preserved Overview.

The **Toolbar** opens a full-height **Panel** immediately to its left. Search, Agenda, Tasks,
Notes, Contacts, Bookmarks, Activities, notifications, audio, network, Bluetooth, display, and power are Panel sections.
The StatusNotifier ellipsis uses a compact **Tray popover** instead. “Niri overview” refers only to
Niri's compositor-wide zoomed-out workspace/window mode.

Notes, Tasks, Calendar, Contacts, and Bookmarks read hosted Foyer Server data through
`foyer-shell-personal`. That crate owns one PowerSync replica and worker; domain crates keep
normalized models and controllers. Start the local stack with `make notes-dev` and export
`FOYER_DEV_TOKEN` (and optionally `FOYER_API_BASE_URL`) before launching Foyer Shell. The replica
file lives outside `foyer-shell-storage`. Building the native SQLite bindings requires Clang.

See [the product vision](../../docs/product-vision.md),
[Presentation architecture](../../docs/architecture.md), and
[ADR 0018](../../docs/adr/0018-foyer-shell-product-and-presentation-terminology.md).

## Presentation pipeline

The supported Presentation path is native GPUI:

1. A bounded Luna xhigh reasoner gathers evidence from the selected project and local SearXNG.
2. A separate Luna low/priority director authors semantic slides without choosing coordinates,
   typography, timings, focus offsets, or animation policy.
3. `foyer-shell-presentation` validates and repairs the stream, assigns stable ids, derives focus
   order, and compiles deterministic slides.
4. `foyer-shell-presentation-player` renders the Presentation and navigates immutable published
   slides.
5. The shared `foyer-shell-tts` service synthesizes Chatterbox Nano PCM while the player-owned CPAL
   device clock drives narration cues and visual focus.
6. Completed Presentations are stored as immutable bundles and can replay without another model or
   synthesis request.

The former Bevy Stage and PixiJS renderer/web-host experiments have been removed.

## Configuration

Create `.env` in the repository root:

```dotenv
OPENROUTER_API_KEY=
FOYER_SHELL_MODEL=openai-codex/gpt-5.6-luna
FOYER_SHELL_REASONER_THINKING_LEVEL=xhigh
FOYER_SHELL_SEARXNG_URL=http://127.0.0.1:8888
FOYER_SHELL_TTS_BACKEND=chatterbox
FOYER_SHELL_TTS_VOICE=tifa
FOYER_SHELL_TTS_DEVICE=cuda
FOYER_SHELL_TTS_THREADS=4

# Optional deployment overrides:
# FOYER_SHELL_TTS_REFERENCE=/path/to/clean-voice-reference.wav
# FOYER_SHELL_TTS_PYTHON=/path/to/chatterbox-venv/bin/python
# FOYER_SHELL_TTS_WORKER=/path/to/worker.py

# OpenRouter remains an explicit remote development option:
# FOYER_SHELL_TTS_BACKEND=openrouter
# FOYER_SHELL_TTS_MODEL=openai/gpt-4o-mini-tts-2025-12-15
# FOYER_SHELL_TTS_VOICE=alloy
# FOYER_SHELL_TTS_ENDPOINT=https://openrouter.ai/api/v1/audio/speech
```

The local `.env` is ignored by Git. The sidecar uses the Codex OAuth linked in Pi; the OpenRouter
credential is used only by the optional remote speech backend. Chatterbox Nano requires CUDA and
does not silently fall back to CPU. Kokoro remains an opt-in development comparison when
`foyer-shell-audio` is built with its `kokoro` feature.

## Run

Start local SearXNG and the desktop:

```bash
docker compose -f infra/searxng/compose.yml up -d
cargo run --release -p foyer-shell
```

Run the standalone Presentation player when developing that subsystem independently:

```bash
cargo run --release -p foyer-shell-presentation-player
```

The desktop expects a Niri session. It opens one Toolbar per output, reconciles Workspace 1 and
output state from Niri IPC, and keeps notification and OSD surfaces non-focus-stealing.

### Hosted Agenda and Tasks

Agenda and Tasks now render hosted Calendar and Tasks snapshots. The normalized
`foyer-shell-agenda` types remain the presentation boundary for upcoming items and source
visibility. The Evolution Data Server bridge stays in `foyer-shell-agenda` only as unused
transitional code and is no longer started by the desktop.

### Dictation

`Super+Alt+R` starts or stops the independent Rust transcription service. During capture, bounded
waveform telemetry drives the Toolbar ambient. On completion, Parakeet Unified ONNX decoding uses
the software-engineering 6-gram, then Foyer Shell pastes the transcript and reports it through the
normal notification path. Audio and transcripts are not persisted by the service.

Default assets:

```text
~/.local/share/foyer-shell/transcription/parakeet-unified-en-0.6b-onnx/
  encoder.onnx (or encoder.int8.onnx)
  encoder.onnx.data (or encoder.int8.onnx.data)
  decoder_joint.onnx (or decoder_joint.int8.onnx)
  tokenizer.model
~/.local/share/foyer-shell/transcription/software-engineering-unified-6gram.sng
```

`FOYER_SHELL_TRANSCRIPTION_MODEL_DIR`, `FOYER_SHELL_TRANSCRIPTION_NGRAM`,
`FOYER_SHELL_TRANSCRIPTION_NGRAM_ALPHA`, and
`FOYER_SHELL_TRANSCRIPTION_EXECUTION_PROVIDER=cpu|cuda` override those defaults.

The trusted legacy NeMo model can be converted once outside the installed runtime:

```bash
python tools/export_ngram.py old-language-model.nemo software-engineering-unified-6gram.sng
```

## Semantic desktop commands

Niri keybindings and other local clients use the fixed control protocol:

```bash
foyer-shell search toggle
foyer-shell agenda toggle
foyer-shell tasks toggle
foyer-shell notes toggle
foyer-shell contacts toggle
foyer-shell bookmarks toggle
foyer-shell notifications toggle
foyer-shell audio toggle
foyer-shell network toggle
foyer-shell bluetooth toggle
foyer-shell display toggle
foyer-shell tray toggle
foyer-shell power toggle
foyer-shell session lock
foyer-shell transcription toggle
foyer-shell volume raise
foyer-shell volume lower
foyer-shell volume toggle-mute
foyer-shell microphone raise
foyer-shell microphone lower
foyer-shell microphone toggle-mute
foyer-shell brightness raise
foyer-shell brightness lower
```

The protocol does not accept arbitrary commands or shell text.

## Install the current development build

```bash
cargo build --release -p foyer-shell
cargo build --release -p foyer-shell-transcription
cargo build --release -p foyer-shell-tts
install -Dm755 target/release/foyer-shell ~/.local/bin/foyer-shell
install -Dm755 target/release/foyer-shell-transcription ~/.local/bin/foyer-shell-transcription
install -Dm755 target/release/foyer-shell-tts ~/.local/bin/foyer-shell-tts

install -Dm755 contrib/tts/worker.py ~/.local/share/foyer-shell/tts/worker.py
install -d ~/.local/share/foyer-shell/tts/voices
uv venv --python 3.11 ~/.local/share/foyer-shell/tts/venv
uv pip install --python ~/.local/share/foyer-shell/tts/venv/bin/python -r contrib/tts/requirements.txt

install -Dm644 sidecar/package.json ~/.local/share/foyer-shell/sidecar/package.json
install -Dm644 sidecar/package-lock.json ~/.local/share/foyer-shell/sidecar/package-lock.json
install -Dm644 sidecar/src/main.mjs ~/.local/share/foyer-shell/sidecar/src/main.mjs
install -Dm644 sidecar/src/protocol.mjs ~/.local/share/foyer-shell/sidecar/src/protocol.mjs
install -Dm644 sidecar/src/filesystem.mjs ~/.local/share/foyer-shell/sidecar/src/filesystem.mjs
install -Dm644 sidecar/src/search.mjs ~/.local/share/foyer-shell/sidecar/src/search.mjs
npm ci --omit=dev --prefix ~/.local/share/foyer-shell/sidecar

install -Dm600 .env ~/.config/foyer-shell/environment
install -Dm644 contrib/systemd/foyer-shell.service ~/.config/systemd/user/foyer-shell.service
install -Dm644 contrib/systemd/foyer-shell-transcription.service ~/.config/systemd/user/foyer-shell-transcription.service
install -Dm644 contrib/systemd/foyer-shell-tts.service ~/.config/systemd/user/foyer-shell-tts.service
install -Dm644 contrib/systemd/foyer-shell-ydotoold.service ~/.config/systemd/user/foyer-shell-ydotoold.service
systemctl --user daemon-reload
systemctl --user enable --now foyer-shell-ydotoold.service foyer-shell-transcription.service foyer-shell-tts.service foyer-shell.service
```

The first process that resolves Foyer Shell's XDG roots moves an existing `amazity-shell`
directory to `foyer-shell` when the destination does not exist. Saved Presentation bundles and the
schema-v4 catalog receive bounded one-time migrations. Old binaries, units, D-Bus names, commands,
and environment variables are not retained as aliases; remove installed `shell-*` user units after
switching the session to the new units.

The complete Niri session configuration is [contrib/niri/config.kdl](contrib/niri/config.kdl), and
the smaller mergeable binding set is
[contrib/niri/foyer-shell-bindings.kdl](contrib/niri/foyer-shell-bindings.kdl). Keep an emergency
terminal binding independent of Foyer Shell and validate changes with:

```bash
niri validate -c contrib/niri/config.kdl
```

The Overview-styled secure lock configuration is in [contrib/gtklock](contrib/gtklock). GTKLock
owns the Wayland session lock and PAM boundary; swaylock remains the secure fallback.

## Workspace structure

- `crates/desktop`: `foyer-shell`, Workspace 1 host, Overview, Toolbar, Panel, notifications, and
  OSD surfaces.
- `crates/presentation`: deterministic Presentation compiler, durable bundles, and replay state.
- `crates/presentation-player`: GPUI Presentation renderer and playback controller.
- `crates/presentation-ui`: Presentation-specific editor, chart, prompt, and tree components.
- `crates/protocol`: versioned slide, narration, and observable-work contracts.
- `crates/pi-bridge` and `sidecar`: supervised Pi SDK roles and their JSONL boundary.
- `crates/audio` and `crates/tts`: synthesis clients, shared TTS service, PCM playback, and cues.
- `crates/transcription`: reusable Parakeet service, client, 6-gram, and paste boundary.
- `crates/agenda`: normalized agenda/task snapshots; desktop now projects hosted Calendar and Tasks.
- `crates/personal`: shared PowerSync replica, worker, and connector for hosted personal data.
- `crates/notes`, `crates/tasks`, `crates/calendar`, `crates/contacts`, `crates/bookmarks`:
  hosted domain models and controllers.
- `crates/services`: typed audio, network, Bluetooth, display, power, media, tray, and notification
  adapters.
- `crates/storage`: SQLite migrations and typed durable repositories.
- `crates/niri`: reconnecting Niri state and semantic compositor commands.
- `crates/paths`: canonical XDG roots and the one-time product-name migration.
- `crates/ui`: shared fixed visual tokens and components.

## Checks

```bash
npm test --prefix sidecar
cargo fmt --all --check
cargo test --workspace
cargo run -p foyer-shell-pi-bridge --example smoke
cargo run --release -p foyer-shell-audio --example narration_smoke
```
