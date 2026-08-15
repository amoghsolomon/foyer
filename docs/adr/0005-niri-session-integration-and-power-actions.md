# ADR 0005: Explicit Niri session integration and confirmed power actions

- **Status:** Accepted
- **Date:** 2026-08-13
- **Owners:** Foyer Shell project

## Context

The first desktop slices can be launched manually and controlled through a small Unix socket, but
they are not yet a dependable daily Niri session. Media keys bypass Foyer Shell, service snapshots may
lag behind those changes, bars are opened only once, and there is no Foyer Shell-owned path for locking,
suspending, ending the compositor session, restarting, or powering off.

Startup and power controls cross durable ownership and safety boundaries. Niri configuration is
user-authored state that may already contain conflicting bindings. Restart, power-off, and logout
discard an active desktop session. These decisions should not be hidden in view callbacks or an
installer that edits configuration without review.

## Decision

### Session startup and commands

The repository will ship reviewed Niri binding and systemd user-service artifacts. Installation is
an explicit documented step: Foyer Shell will not rewrite `~/.config/niri/config.kdl`, enable a user
service, or overwrite an existing unit automatically.

The existing per-user Unix control socket remains the command boundary for Niri keybindings. It
will accept semantic launcher, settings, power-menu, volume, mute, and brightness requests. The
client invocation sends one fixed protocol line; it cannot forward arbitrary commands or shell
text. Media requests enter the typed service controller and refresh authoritative state
immediately after each mutation, allowing the existing OSD comparison to react without a separate
OSD protocol.

The systemd unit restarts Foyer Shell only after failure and is tied to `graphical-session.target`. An
emergency terminal binding remains compositor-owned and outside Foyer Shell. The Niri artifact is a
snippet to merge deliberately because bindings such as `Mod+Comma`, media keys, and
`Ctrl+Alt+Delete` commonly already exist.

### Output lifecycle

Niri output names remain authoritative. Foyer Shell periodically and event-driven reconciles those names
against GPUI display identifiers. Each bar record includes its Niri output, GPUI display, geometry,
and handle. Missing or changed outputs remove or recreate their bars, close interactive transients
on the removed display, and discard non-focus notification/OSD surfaces that can no longer be
routed. A transient Niri IPC disconnect does not remove bars.

### Power and session actions

The top bar exposes one power/session popover as an ordinary mutually exclusive interactive
transient. Lock runs immediately through a typed service action. Suspend, logout, restart, and
power-off require a second in-popover confirmation that names the exact action and consequence.
The confirmation is local presentation state and cannot change the selected action after display.

Backends for this iteration are deliberately narrow:

| Action | Backend |
| --- | --- |
| Lock | `swaylock --daemonize` |
| Suspend | `systemctl suspend` |
| Log out | typed Niri `Quit { skip_confirmation: true }` after Foyer Shell confirmation |
| Restart | `systemctl reboot` |
| Power off | `systemctl poweroff` |

Commands are executed directly with argument arrays on worker threads. No shell is involved.
Actions disable when their required executable or Niri connection is unavailable. These controls
are for direct local user interaction only; they are not exposed as agent capabilities. A future
capability broker must establish separate immutable approval and audit contracts before an agent
can request them.

## Alternatives and deliberate exclusions

- Automatically patching the active Niri configuration would be convenient but risks destroying
  comments, conflicting with existing bindings, or leaving the session unstartable.
- Direct media commands in Niri are reliable emergency fallbacks, but bypass Foyer Shell's semantic
  service boundary and delay or omit OSD feedback.
- `loginctl lock-session` without a known lock handler can report success while presenting no lock
  screen, so this iteration requires `swaylock` for the lock button.
- A privileged helper or policy daemon is unnecessary for direct local logind operations and would
  create a much larger security boundary.
- Agent-triggered power actions, configurable command templates, automatic Niri config migration,
  and a custom Foyer Shell lock screen are excluded.

## Consequences and risks

Foyer Shell becomes installable as a restartable Niri session component and owns immediate media-key
feedback. Output hotplug no longer leaves duplicate or orphaned layer surfaces. Consequential power
operations have one consistent confirmation flow.

System power requests can still be rejected by logind or polkit at runtime. Presence of the backend
is not a guarantee of authorization, so failures remain logged and must later become visible
service results when the desktop gains a general activity/error surface. The shipped snippets also
require one deliberate user installation step.

## Validation criteria

- Repeated reconciliation never opens two bars for the same Niri output.
- Removing an output closes its bar and all surfaces routed to that GPUI display; reconnecting it
  recreates exactly one bar.
- Socket media commands mutate the real service and publish the refreshed snapshot quickly enough
  to drive the OSD.
- The lock action starts the installed locker; suspend, logout, restart, and power-off cannot run
  from their first button press.
- Unknown socket commands cannot execute a process or reach a system-service adapter.
- The provided systemd unit and Niri snippet validate without modifying the active configuration.

## Supersession

This ADR extends ADR 0001 through ADR 0004. It does not supersede them.

ADR 0006 moves the power/session experience from a top-bar popover into the unified reveal panel.
The confirmation policy, typed actions, backends, startup, control-socket, and output-lifecycle
decisions in this ADR remain in force.

ADR 0011 supersedes swaylock as the preferred lock presentation backend with a foyer-styled
GTKLock configuration; it also supersedes the uninstalled Hyprlock choice in ADR 0009. The typed
action and swaylock recovery fallback remain in force.
