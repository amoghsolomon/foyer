# ADR 0002: Desktop shell surfaces, UI language, and state ownership

- **Status:** Superseded
- **Superseded by:** ADR 0006
- **Date:** 2026-08-13
- **Owners:** Foyer Shell project

## Context

ADR 0001 chose Niri and upstream GPUI layer-shell support as the platform foundation. The first
desktop iteration now needs stable product-surface behavior and code boundaries. Without these,
the bar, launcher, panels, and services could independently invent visual styles, window ownership,
and transient-state rules.

The repository already has a quiet monochrome GPUI design language and uses `gpui-component` for
native inputs, editors, charts, trees, and root-window behavior. The desktop shell should feel like
an extension of that application, not a separately themed Wayland utility.

## Decision

### Visual system

The existing Foyer Shell presentation UI is the visual authority. Desktop surfaces will use the same
palette, typography, spacing, radii, borders, focus treatment, and restrained motion.

We will use `gpui-component` wherever an appropriate component exists. Foyer Shell will compose or
lightly wrap those components and add only the shell-specific primitives that are missing. We will
not create a parallel general-purpose widget toolkit.

Shared theme initialization and design tokens belong in `crates/ui`. The presentation UI and the
desktop UI both consume that crate so their visual language cannot drift through copied constants.

### Surface behavior

The persistent top bar is created once per output and reserves exactly its height. Its main status
and detail controls live at the top-right.

Small bar details open as overlay popovers immediately below the control that opened them. They do
not reserve workspace space. Only one bar popover may be open at a time; choosing another control
replaces it.

The control panel and settings share a right-side overlay drawer. It begins below the top bar,
occupies a fixed bounded width, and slides in from the right. Closing it reverses the animation and
removes or suspends the layer surface after it is offscreen. It never changes Niri's usable
workspace width.

The launcher is a centered, keyboard-first overlay inspired by Raycast. Its first implementation
indexes desktop entries, performs fuzzy search, supports keyboard selection, and launches the
selected application. Commands, files, calculations, extensions, and plugins are excluded until
the application-launching path is complete and reliable.

Bars appear on every output. A surface opened from a clicked bar appears on that bar's output.
Keyboard-invoked transient surfaces appear on the focused output. Notifications may be routed per
output by notification policy later.

Escape, clicking the active trigger, or clicking outside closes a transient surface. At most one
interactive transient surface—the launcher, drawer, or a bar popover—owns keyboard focus at once.
Notification and OSD surfaces never take keyboard focus.

### State and window ownership

One application-owned surface coordinator tracks every GPUI window handle and the active transient
surface. Views request semantic operations such as `toggle_launcher` or `open_audio_popover`; they
do not keep process-wide static window handles.

Niri IPC owns compositor facts. Typed system-service adapters own their service facts. Views render
snapshots and send commands through those owners; they do not open independent IPC connections or
run service commands during rendering.

Niri and service listeners must reconnect after failure. A restarted listener publishes a fresh
authoritative snapshot before incremental state is trusted.

## Initial crate boundaries

- `crates/desktop`: desktop executable, lifecycle, surface coordinator, and composition of product
  surfaces.
- `crates/ui`: shared Foyer Shell theme, tokens, and reusable fixed-design components.
- `crates/niri`: typed Niri state, commands, event-stream tracking, and reconnection.
- `crates/services`: typed non-compositor services and lifecycle management, introduced as the
  first real service integrations are implemented.
- `crates/presentation-ui`: presentation-specific editors, charts, prompts, and trees; it consumes
  the shared UI crate rather than owning the global theme.

Product surfaces may begin as modules in `crates/desktop` or `crates/ui`. They become separate
crates only when compilation boundaries or independent testing provide a concrete benefit.

## First vertical slice

The first executable slice will:

1. align GPUI and `gpui-component` on compatible locked Git revisions;
2. start through `gpui_platform` on Wayland;
3. create a styled per-output layer-shell bar;
4. show current Niri workspace and focused-window state;
5. open a styled launcher overlay from the bar;
6. index desktop entries, fuzzy-filter them, and launch a selection; and
7. close the launcher cleanly without terminating the persistent bar process.

The right drawer, anchored detail popovers, notifications, OSD, and system services build on the
same coordinator after this slice proves surface creation and focus behavior.

## Consequences

The desktop gains a consistent interaction model and one owner for transient focus. Sharing the
theme through a small UI crate makes visual consistency the default and keeps
`gpui-component` integration reusable.

The coordinator is an intentional central authority, so care is required to keep it focused on
surface lifecycle rather than turning it into a container for service logic. Anchored popovers and
drawer motion also require real compositor testing because layer-shell positioning and input
regions cannot be validated by unit tests alone.

## Supersession

ADR 0006 supersedes this ADR's top-bar, popover, drawer, and centered-launcher surface model. Its
visual-system, state-ownership, output-routing, and crate-boundary decisions remain in force.
