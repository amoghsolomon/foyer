# ADR 0004: In-process notification daemon and snapshot-driven OSD surfaces

- **Status:** Accepted
- **Date:** 2026-08-13
- **Owners:** Foyer Shell project

## Context

ADR 0002 reserves non-focus overlay surfaces for notifications and on-screen displays, but leaves
their service ownership and routing policy open. The first useful desktop iteration needs to show
application notifications and immediate feedback for volume and brightness changes without adding
a second process, a notification history database, or another general extension system.

Notifications are an external protocol boundary: Linux applications call
`org.freedesktop.Notifications` on the session bus and expect stable identifiers, replacement, close
requests, and close-reason signals. Volume and brightness OSDs are different. Their authoritative
facts already arrive through the typed snapshots established by ADR 0003, so another IPC service
would duplicate state ownership.

## Decision

The desktop process will host a small `org.freedesktop.Notifications` implementation through a
typed adapter in `crates/services`. It requests the well-known name with `DoNotQueue` and never
replaces an existing notification daemon. Failure to connect or acquire the name is published as
an unavailable status and retried, rather than preventing the rest of the shell from starting.

The first protocol surface implements `GetCapabilities`, `Notify`, `CloseNotification`,
`GetServerInformation`, and the `NotificationClosed` signal. It supports notification replacement
through `replaces_id`, server-default and explicit expiry, and the standard close reasons. Actions,
inline replies, images, sound, persistence, history, and application-specific positioning are not
advertised or interpreted in this iteration.

Notification payloads are untrusted. Summary, body, and application name are length-bounded before
they cross into the UI. Markup is reduced to plain text, and icon paths, URLs, and action commands
are never executed. A single top-right overlay on the currently focused output shows at most three
cards below the bar. New cards replace matching identifiers or evict the oldest card. The surface
uses an explicit card-sized input region, is removed when empty, and never requests keyboard focus.

Volume and brightness OSDs are derived in `crates/desktop` by comparing consecutive available
service snapshots after the first snapshot has initialized. One bottom-centered overlay is reused
for coalesced changes and removed after a short timeout. It has an empty input region and never
takes focus. OSD state is presentation state only; the service snapshot remains authoritative.

Notification and OSD handles are owned beside the interactive transient coordinator in the
application's `ShellState`. They do not participate in the one-interactive-transient rule because
they never own keyboard focus.

## Alternatives and deliberate exclusions

- A standalone daemon would isolate failures but adds process supervision and cross-process state
  before either surface has stabilized.
- Reusing an existing notification daemon would prevent Foyer Shell from owning the notification visual
  language and surface policy.
- Treating OSDs as notifications would conflate private shell feedback with an external protocol,
  add avoidable D-Bus traffic, and make coalescing less deterministic.
- Notification history and action support remain future slices because they require durable state,
  richer trust decisions, and additional interaction design.

## Consequences and risks

The shell can receive standard desktop notifications with a deliberately small protocol and UI
surface while keeping OSDs coupled to already-authoritative service state. The desktop process now
owns a session-bus name and must continue to degrade cleanly when another daemon already owns it.

The Freedesktop notification protocol has more optional behavior than this slice advertises.
Applications must receive honest capabilities, and unsupported hints must remain harmless. Keeping
the adapter typed and separate from GPUI leaves room for a later daemon process without changing
the view-facing event model.

## Validation criteria

- `notify-send` can create, replace, explicitly close, and naturally expire a notification.
- Notification cards remain clickable but never activate the window or steal keyboard focus.
- A fourth notification evicts the oldest card and emits a close reason.
- Initial service discovery shows no OSD; subsequent volume, mute, and brightness changes coalesce
  into the bottom-centered OSD and disappear automatically.
- An existing owner of `org.freedesktop.Notifications` is not replaced and does not stop the bar,
  launcher, drawer, or system controls from working.

## Supersession

This ADR extends ADR 0002 and ADR 0003. It does not supersede either decision.

ADR 0006 supersedes only the placement of notifications and OSD surfaces. The protocol, ownership,
focus, sanitization, expiry, and snapshot decisions in this ADR remain in force.

ADR 0007 supersedes this ADR's deliberate exclusion of notification persistence and history. The
remaining notification protocol and ambient presentation decisions remain in force.
