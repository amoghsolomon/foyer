# ADR 0012: Own bounded interactive desktop D-Bus integrations

- **Status:** Accepted
- **Date:** 2026-08-14
- **Owners:** Foyer Shell project

## Context

The daily-driver panels established typed polling adapters for audio, Wi-Fi, Bluetooth, display,
battery, and session actions. Three desktop interactions remained materially incomplete:

- BlueZ pairing that requires PIN entry, passkey display, numeric confirmation, or service
  authorization could not be completed by the non-interactive Bluetooth helper;
- the notification daemon discarded application action keys and could not activate an associated
  desktop entry when an application supplied no default action; and
- MPRIS players and StatusNotifierItems had no Foyer Shell host, leaving playback and essential
  background-application controls outside the desktop frame.

These are long-lived D-Bus protocol and ownership decisions. They must remain independent of GPUI
rendering and must not become arbitrary application-command execution paths.

## Decision

### Bluetooth Agent1

`foyer-shell-services` exports one `org.bluez.Agent1` object on the system bus and registers it with
BlueZ as a `KeyboardDisplay` default agent. BlueZ remains the pairing authority. The agent converts
PIN, passkey, confirmation, and authorization callbacks into one typed pending request in the
Bluetooth snapshot. GPUI renders that request inside the existing Bluetooth section and returns a
typed answer through an in-memory, single-use response channel.

Pairing requests expire after 90 seconds. Cancel, timeout, malformed passkeys, and panel rejection
fail closed using BlueZ error replies. PINs and passkeys are never logged or persisted. Display-only
codes may be shown, but only callbacks that require an answer block awaiting the exact request id.

### Notifications

The notification daemon advertises `actions`, `body`, and `persistence`. It preserves a bounded set
of exact action keys with sanitized labels and emits `ActionInvoked` when the user chooses one.
Non-resident notifications close after invocation; resident notifications remain until explicitly
closed. Replacement and close calls retain the existing notification-id ownership rules.

The `desktop-entry` hint is retained only as a bounded identifier. If a notification has no default
action, Foyer Shell may resolve that identifier against its already-indexed XDG desktop entries and
launch the matched fixed argument vector. Untrusted hints never become shell commands. Persisted
history keeps notification content, not stale action authority: actions are available only while
the originating notification session is active.

Foyer Shell does not fabricate an xdg-activation token. Applications receive `ActionInvoked` and remain
responsible for their normal Wayland activation behavior; the indexed desktop-entry fallback is
used only when no default action exists.

### MPRIS

An independent session-bus worker discovers `org.mpris.MediaPlayer2.*` names, reads player identity,
playback status, bounded metadata, artwork URI, and control capabilities, and invokes only the
standard Raise, Previous, PlayPause, and Next methods. Active players appear in the Audio section,
ordered ahead of paused and stopped players. Multiple players remain individually selectable.

Artwork accepts only `file`, `http`, and `https` sources. A failed player or artwork load degrades
that player without blocking audio device controls.

### StatusNotifier

> **Partially superseded by ADR 0013.** ADR 0013 replaces the full reveal-panel presentation and
> the decision to forward rather than render DBusMenu trees. The host and typed-worker boundary
> established here remain accepted.

An independent worker acts as a StatusNotifier host. When no watcher exists it also owns
`org.kde.StatusNotifierWatcher`; otherwise it registers as a host with the existing watcher. One
curated Toolbar icon opens the Application Status section. The section omits passive items and exposes
the standard Activate, SecondaryActivate, and ContextMenu operations with fixed coordinates.

The initial host retains item title, category, status, icon name, and menu behavior metadata. It
does not implement legacy XEmbed, arbitrary embedded widgets, or a configurable icon row. A menu
request is sent back to the item; Foyer Shell does not yet re-render arbitrary DBusMenu trees.

All four integrations publish immutable typed snapshots or events. No D-Bus query, pairing wait,
image load, or application method call runs from a GPUI render callback.

## Alternatives and deliberate exclusions

- Continuing with `bluetoothctl --agent NoInputNoOutput` cannot satisfy interactive pairing and
  reports legitimate devices as unsupported.
- Persisting notification actions would retain authority after the client and notification id are
  gone, so history deliberately remains informational.
- `playerctl` could provide basic MPRIS commands but would add output parsing and obscure individual
  bus owners; direct typed D-Bus calls are smaller here.
- A separate tray daemon or Waybar would duplicate the persistent Foyer Shell frame. Legacy XEmbed is an
  X11 embedding protocol and remains excluded from the Niri-only shell.
- Full DBusMenu rendering, tray icon pixmap decoding, media seeking, playlists, media-key player
  arbitration, and notification inline reply fields are deferred until a concrete application
  requires them.

## Consequences and risks

Foyer Shell becomes the session's default Bluetooth authentication surface and StatusNotifier host.
Crashes or bus disconnects therefore temporarily remove those interactions, while BlueZ, media
players, and applications keep their underlying state. Each worker retries or reports availability
independently so core audio, network, and display controls remain usable.

Some applications expose incomplete MPRIS or StatusNotifier properties. Every optional property is
treated as fallible, item failures are isolated, and UI controls honor advertised capability flags.
StatusNotifier has implementation variation between KDE and Freedesktop naming; the host follows
the deployed `org.kde` watcher contract used by current Linux applications.

## Validation criteria

- BlueZ reports Foyer Shell's `KeyboardDisplay` agent and PIN, passkey, confirmation, rejection, timeout,
  and cancel paths resolve the exact pending request without persistence.
- Notification capabilities include actions and persistence; action selection emits the original
  key, non-resident notifications close, resident ones remain, and desktop-entry fallback launches
  only an indexed entry.
- Every live MPRIS player reports identity and metadata independently; Previous, PlayPause, Next,
  and Raise work only when supported.
- StatusNotifier applications can register whether Foyer Shell owns or joins the watcher, appear behind
  one Toolbar icon, and receive Activate, SecondaryActivate, and ContextMenu calls.
- Slow or unavailable D-Bus services do not delay audio, Wi-Fi, brightness, notifications, or the
  GPUI render thread.

## Supersession

This ADR extends ADR 0004's notification protocol boundary, ADR 0006's unified Toolbar, and ADR 0010's
independent service-worker model. It supersedes ADR 0010 only where that record deliberately deferred
interactive BlueZ Agent1 pairing.

ADR 0013 supersedes this record only for StatusNotifier presentation, icon resolution, watcher
recovery, and DBusMenu handling.
