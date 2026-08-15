# ADR 0010: Independent daily-driver system service adapters

- **Status:** Accepted
- **Date:** 2026-08-14
- **Owners:** Foyer Shell project

## Context

ADR 0003 proved typed command-backed audio, network, and brightness controls with one polling
worker. The unified right-edge panel now needs device selection, microphone and application audio,
Wi-Fi discovery and credentials, Bluetooth pairing and connection, battery state, and richer
diagnostics. Wi-Fi scans and Bluetooth operations can take seconds; serializing them with volume
and brightness commands would make immediate controls appear broken.

These integrations also cross different trust boundaries. Wi-Fi passwords belong to
NetworkManager, not Foyer Shell persistence. Bluetooth pairing may fail or require an authentication
method the first implementation cannot satisfy. Application audio nodes and nearby radios are
ephemeral system facts rather than durable Foyer Shell records.

## Decision

`foyer-shell-services` retains one public typed `Snapshot` and `Controller`, but uses independent worker
threads for Audio, Network, Bluetooth, Display, and Power. A small aggregator publishes complete
snapshots to GPUI. Slow work in one domain cannot block commands or event handling in another domain, and
views continue to render immutable state and send semantic commands only.

### Event-driven refinement

Idle profiling on the target laptop found that the command-backed refresh timers were no longer an
acceptable implementation detail for an always-running shell. The desktop consumed approximately
one quarter of a logical CPU while idle and launched 17.5 successful helper processes per second.
Network refreshes accounted for most sampled CPU because each four-second pass started one `nmcli`
process for every saved Wi-Fi profile; Bluetooth and audio timers created the next largest sources
of process churn.

Audio, Network, and Bluetooth therefore rebuild snapshots from event notifications instead of fixed
refresh timers. NetworkManager and BlueZ changes arrive through persistent system-bus signal
subscriptions. BlueZ device and adapter reads use one ObjectManager snapshot rather than one
`bluetoothctl info` child per device. Saved NetworkManager Wi-Fi settings are read over D-Bus, so the
number of host profiles cannot multiply `nmcli` process launches. PulseAudio changes arrive through
one supervised `pactl subscribe` stream; client-only events produced by snapshot queries are ignored
to prevent a feedback loop. Event bursts are coalesced before one authoritative snapshot rebuild.
NetworkManager notifications are filtered to properties represented by the shell snapshot, wireless
access-point additions/removals, and saved-connection changes. Periodic signal strength, byte
statistics, and unrelated virtual-interface lifecycle notifications are ignored because rebuilding
the Wi-Fi snapshot for them recreates polling-like helper churn. Connection, Wi-Fi device, radio,
and connectivity changes remain event-driven refresh triggers.

Semantic mutations may retain their fixed command backends, and an explicit panel refresh may still
request discovery. Event-stream loss uses bounded exponential reconnect backoff and never restores a
periodic success-path poll. This refinement changes replaceable adapter internals already anticipated
by this ADR; it does not change the public typed state or command boundary.

The first daily-driver backends are:

| Domain | Read and mutation boundary |
| --- | --- |
| Audio | PipeWire's Pulse compatibility API through machine-readable `pactl` JSON |
| Network | NetworkManager through fixed `nmcli` argument arrays |
| Bluetooth | BlueZ through fixed non-interactive `bluetoothctl` commands |
| Display | backlight sysfs reads and fixed `brightnessctl` commands |
| Battery | UPower aggregate display-device data |
| Session | fixed `swaylock` and `systemctl` actions retained from ADR 0005 |

Audio exposes playback and capture devices, default selection, output and microphone volume/mute,
active playback streams, and active capture clients. Per-application controls apply only to live
streams and are not persisted.

Network exposes Wi-Fi radio state, scans, access points, connection, disconnection, saved-profile
reuse, and forgetting. A password is held only in a masked GPUI input for the active prompt, passed
to an `nmcli --ask` child over standard input, and cleared immediately after submission or cancel.
It is never logged, added to command arguments, or written to the Foyer Shell SQLite database.
NetworkManager remains responsible for connection profiles and their secrets.

Bluetooth exposes controller power, bounded discovery, remembered and nearby devices,
connect/disconnect, removal, and non-interactive pairing suitable for Just Works devices. Pairing
errors remain visible in the panel. ADR 0012 supersedes this record's deferral of display-based
passkey confirmation, PIN entry, and service authorization with a Foyer Shell-owned BlueZ Agent1.

Battery state comes from UPower. Power-profile selection appears only when
`powerprofilesctl` is installed and reports supported profiles. Missing optional services degrade
only their own sections.

Sliders update their visual value optimistically during interaction, send the final semantic
command on release, and reconcile with the next authoritative snapshot. Backend failures remain
visible inline rather than silently becoming UI state.

Do Not Disturb is a durable Foyer Shell preference in `foyer-shell-storage`. It suppresses ambient cards for
non-critical notifications while retaining every notification in history. Critical notifications
remain visible. No other system-service state is copied into Foyer Shell persistence.

## Alternatives and deliberate exclusions

- One expanded polling worker has less code but lets radio scans stall media and backlight input.
- Native PipeWire, libnm, BlueZ, and UPower bindings could become event-driven replacements, but
  adding all four at once would substantially increase native/API surface before the interactions
  stabilize.
- Foyer Shell will not store Wi-Fi or Bluetooth credentials in SQLite.
- Persistent application-volume rules, audio effects, enterprise Wi-Fi, hotspots, VPN editing,
  Bluetooth file transfer, night light, color management, accessibility, printers, users, and a
  general settings extension system are outside this milestone.
- Output mode, scale, and transform mutation remain Niri configuration decisions; the panel may
  report current geometry without inventing a second persistence owner.

## Consequences and risks

The shell becomes usable for ordinary laptop audio, capture, Wi-Fi, Bluetooth, brightness, battery,
and session operation without expanding into a full distribution control center. Independent
workers isolate slow and unavailable services, at the cost of several subprocess-backed parsers
and a merged snapshot that can contain facts sampled at slightly different times.

Command output compatibility remains a risk. Parsers use machine-readable formats where the
installed tools provide them and have focused fixtures. Native event-driven reads may continue to
supersede individual command-backed reads without changing GPUI's typed contract.

The measured event-driven refinement performs that migration for steady-state observation while
retaining narrow command-backed mutations. Idle resource use no longer scales with the number of
saved Wi-Fi profiles or remembered Bluetooth devices. The event subscriptions add reconnect and
burst-coalescing behavior that must be validated when NetworkManager, BlueZ, or PulseAudio restarts.

## Validation criteria

- Dragging output, microphone, and brightness sliders updates immediately and round-trips through
  the real service.
- Selecting an audio sink or source changes the default and moves active streams where supported.
- Live application playback streams can be muted and adjusted independently.
- Wi-Fi scanning, open/saved connection, password connection, disconnection, and forgetting do not
  expose secrets to logs, process arguments, or Foyer Shell storage.
- Bluetooth power, discovery, connect/disconnect, removal, and Just Works pairing report success or
  a visible error without blocking other panels.
- Battery data and optional power modes degrade cleanly when their providers are absent.
- Do Not Disturb survives restart, keeps history, suppresses non-critical cards, and permits
  critical cards.
- An idle 30-second process trace launches no periodic `nmcli`, `bluetoothctl`, or snapshot `pactl`
  helpers; one persistent audio event monitor is expected.
- NetworkManager, BlueZ, and PulseAudio state changes trigger one coalesced authoritative refresh,
  and restarting a provider reconnects its event stream with bounded backoff.
- Saved Wi-Fi profile and remembered Bluetooth device counts do not multiply helper-process launches.
- No query, subprocess, database operation, or radio scan runs from a GPUI render callback.

## Supersession

This ADR supersedes ADR 0003's single-worker implementation while retaining its typed snapshot,
semantic command, direct-argument execution, availability, and replaceable-backend decisions. It
extends ADR 0004 through ADR 0009 without changing their remaining decisions.
