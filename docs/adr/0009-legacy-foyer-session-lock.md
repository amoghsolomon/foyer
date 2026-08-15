# ADR 0009: Present the foyer through a real Hyprlock session lock

- **Status:** Superseded
- **Superseded by:** ADR 0011
- **Date:** 2026-08-14
- **Owners:** Foyer Shell project

## Context

The initial Workspace 1 foyer establishes Foyer Shell's quiet visual center. Locking should preserve that
visual language while preventing every ordinary desktop action and accepting a password at the
bottom center beside the thoughtful line.

A normal GPUI window, layer-shell overlay, input grab, or compositor rule cannot provide this
security boundary. A lock must use Wayland's session-lock protocol so Niri withholds desktop
content and input until a PAM-authenticated locker releases the session. The existing swaylock
backend provides that boundary, but its fixed circular prompt cannot reproduce the foyer layout.

## Decision

Hyprlock becomes Foyer Shell's preferred lock backend. It owns the Wayland session lock, password input,
and PAM authentication. Foyer Shell owns only the repository-managed visual configuration: black
background, white date and time, the foyer paragraph, a thoughtful line, and a bottom-centered
password field. No cards, panels, agent controls, or actionable desktop surfaces appear while
locked.

All lock entry points use the existing typed services boundary. `foyer-shell session lock` sends
a fixed control message to the running desktop process; the service starts Hyprlock directly with
no shell. Because Hyprlock remains resident for the duration of the lock, it is spawned without
blocking the service worker. If Hyprlock is not installed, Foyer Shell retains `swaylock --daemonize` as
a secure recovery backend. The Niri keybinding calls the typed Foyer Shell command rather than selecting
a locker itself.

The initial foyer and thoughtful copy remain hardcoded. Later they may become dynamic without
changing the authentication boundary: before locking, a Foyer Shell-owned presentation service may
write a bounded, sanitized, plain-text snapshot to the user's runtime directory, and a fixed
trusted reader may expose that snapshot to Hyprlock. The last valid snapshot must be usable
offline. The locker must never invoke the agent, tools, network requests, arbitrary commands,
untrusted markup, or mutable approval actions.

## Alternatives and deliberate exclusions

- Styling a fullscreen GPUI window like a lock screen is excluded because it is only a visual
  overlay and can be bypassed.
- Extending swaylock's renderer would fork a security-sensitive program for presentation needs.
- Implementing the session-lock protocol and PAM authentication inside GPUI would create a new
  security boundary unrelated to Foyer Shell's core product work.
- Live agent prose, tool progress, notifications, agenda data, and scene playback are excluded from
  this slice.

## Consequences and risks

The lock screen can match the foyer without weakening Niri's real session-lock boundary. Hyprlock
and its PAM service become host dependencies; installations without Hyprlock keep the less tailored
swaylock presentation. Configuration compatibility must be validated against the installed
Hyprlock version before the tailored screen is considered live.

Dynamic copy will require explicit length, character, escaping, freshness, and fallback rules. It
is presentation-only even if its source is an agent, and must not reveal sensitive workspace data
on a screen visible to bystanders.

## Validation criteria

- Niri reports the session locked and ordinary compositor/application actions cannot bypass it.
- A correct PAM credential unlocks; an incorrect or empty credential does not.
- Date, time, foyer copy, thoughtful copy, and password field remain legible on every active output.
- The password field is bottom-centered on the same visual row as the thoughtful line.
- The power panel and Niri binding reach the same typed lock action.
- With Hyprlock absent, the action still invokes swaylock rather than presenting an insecure Foyer Shell
  overlay.

## Supersession

This ADR supersedes only ADR 0005's choice of swaylock as the primary lock presentation backend.
ADR 0005's typed action, direct execution, local-interaction, and power-confirmation decisions
remain in force. This ADR extends ADR 0008's foyer visual language without changing Workspace 1.

ADR 0011 supersedes this record after installation research showed that the viable Fedora 43
Hyprlock package also installs the Hyprland compositor. The secure session-lock and visual
requirements remain; GTKLock replaces the backend choice.
