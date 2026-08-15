# ADR 0001: Build the Niri shell on upstream GPUI layer-shell support

- **Status:** Accepted
- **Date:** 2026-08-13
- **Owners:** Foyer Shell project

## Context

Foyer Shell is evolving from the current presentation prototype into a GPUI desktop shell running on
Niri. The intended product surface is broad, but the implementation should stay deliberately
small: one fixed visual design, one supported compositor, and no user-extensible theming or widget
system in the initial versions.

Noctalia is the primary feature and interaction reference. It provides the clearest catalogue of
the shell behavior we want to reproduce selectively: a persistent top bar, launcher, control and
settings panels, notifications, on-screen displays, workspace and window state, and system-service
integration. We are using it as a product reference rather than copying its implementation or its
configuration model.

`andre-brandao/gpui-shell` is a useful implementation reference for the boundary between GPUI and
Wayland. It demonstrates that current Zed GPUI can create native layer-shell surfaces without a
project-maintained GPUI fork. The repository is early-stage and explicitly warns that substantial
parts were AI-generated, so it is not an architectural foundation or a source to copy wholesale.

The current workspace pins the crates.io releases `gpui = 0.2.2` and
`gpui-component = 0.5.1`. That released GPUI does not expose the layer-shell API used by
`gpui-shell`. Current Zed GPUI does.

## Decision

We will build the desktop shell specifically for Niri using the layer-shell support in upstream
Zed GPUI.

We will not begin by forking GPUI. Instead, the first implementation spike will pin a known Zed
commit and validate its layer-shell behavior. GPUI, `gpui_platform`, and `gpui-component` must be
moved to mutually compatible Git revisions together. Mixing Git GPUI with the released
`gpui-component 0.5.1` would introduce two incompatible GPUI type universes.

The initial shell surfaces map to Wayland as follows:

| Foyer Shell surface | Layer | Keyboard interaction | Exclusive zone |
| --- | --- | --- | --- |
| Top bar | Top | None | Bar height |
| Launcher | Overlay | Exclusive while open | None |
| Settings/control panel | Overlay | On demand | None |
| Notifications | Overlay | None | None |
| OSD | Overlay | None | None |

Each output gets its own relevant layer surfaces. Niri IPC is authoritative for output identity,
logical geometry, scale, workspaces, windows, and focus. GPUI display identifiers are rendering
handles, not the source of desktop topology.

Foyer Shell will own an explicit mapping between Niri output names, GPUI display identifiers, and open
window handles. It will not depend on display enumeration order, reported display origins, or
`Window::display()` to identify the output containing a layer-shell surface.

Transparent or screen-sized surfaces will set explicit input regions so they do not intercept
clicks outside interactive content. Surface sizes will be calculated in logical pixels instead of
requesting a zero size on a stretched dimension.

## Initial implementation boundaries

The first runnable foundation includes only enough infrastructure to make shell UI development
safe and repeatable:

1. Pin a compatible GPUI dependency set and migrate application startup to the current platform
   API.
2. Open a fixed-height top bar on Niri using the Top layer and a correct exclusive zone.
3. Toggle one overlay launcher with correct focus acquisition and dismissal.
4. Display one non-focus-stealing notification surface.
5. Verify output selection, fractional scaling, input regions, and cleanup when an output is
   removed.
6. Keep a reliable emergency terminal shortcut and preserve a non-Foyer Shell login session during
   development.

This spike is a platform proof, not the complete first product iteration. Its UI can use temporary
content, but it must establish APIs that the real bar, launcher, panels, notifications, and OSD can
share.

## Project-shape direction

The current presentation pipeline remains useful and should not be rewritten as part of the shell
bootstrap. New desktop-shell code should be separated by responsibility rather than added to the
large application entry point. The exact crate split will be decided during first-iteration
scoping, but the boundaries should cover:

- application lifecycle and surface coordination;
- a small wrapper around GPUI layer-shell window creation;
- Niri IPC state and commands;
- typed system services and their lifecycle;
- fixed-design reusable UI components; and
- product surfaces such as the bar, launcher, panels, notifications, and OSD.

We may borrow the following patterns from `gpui-shell`: reactive typed service state, generation
tokens that prevent stopped listeners from publishing stale state, shared D-Bus connections, one
open panel at a time, per-output surface ownership, and a component gallery for developing widgets
in isolation.

We will not copy its generic compositor abstraction, global mutex-based window ownership, copied UI
library, mixed audio backends, or listener behavior that exits permanently after a Niri IPC
disconnect. Niri listeners in Foyer Shell must reconnect and rebuild authoritative state.

## Deliberate simplifications

The early shell will support:

- Niri only;
- one built-in visual language;
- one opinionated layout and interaction model; and
- a curated set of first-party modules.

It will not initially provide theme engines, user-authored widgets, arbitrary module placement,
other compositor backends, compatibility shims for multiple generations of GPUI, or a generalized
desktop-extension API.

These constraints remove configuration and compatibility work without narrowing the eventual
first-party feature set.

## Known risks to validate

- Upstream GPUI is consumed from Git, so all revisions must be pinned and upgraded deliberately.
- The compatible GPUI stack may require a newer Rust toolchain than the workspace currently uses.
- Fractional scaling and output identity have rough edges in GPUI's Wayland display abstraction.
- Output hot-plug and global removal may be incomplete upstream and need an application-level
  recovery path or a narrowly scoped upstream contribution.
- Layer-shell focus, popup parenting, input regions, and exclusive zones must be tested under Niri,
  not assumed from compilation alone.

A maintained GPUI fork becomes an option only if the validation spike finds a blocking defect that
cannot reasonably be fixed upstream or isolated behind Foyer Shell's surface wrapper.

## Consequences

This decision replaces a likely custom Wayland/GPUI fork with a contained dependency migration and
compositor validation effort. It should make the bare minimum shell substantially more
straightforward while preserving room for a wide first-party feature set.

The cost is that Foyer Shell temporarily follows pinned Git dependencies and must own careful regression
testing around multi-output Wayland behavior.

ADR 0006 supersedes the initial top-bar surface mapping after the layer-shell proof succeeded. The
GPUI/Niri platform, output identity, input-region, and recovery decisions in this ADR remain in
force.

## References

- [Zed GPUI layer-shell API](https://github.com/zed-industries/zed/blob/dd04a229dd22700ff43815b8514453e197c333b7/crates/gpui/src/platform/layer_shell.rs)
- [`gpui-shell` repository](https://github.com/andre-brandao/gpui-shell)
- [`gpui-shell` bar surface](https://github.com/andre-brandao/gpui-shell/blob/110f0f7126df8df9c738924b94130a0e0c057a06/crates/app/src/bar/view.rs)
- [`gpui-shell` launcher surface](https://github.com/andre-brandao/gpui-shell/blob/110f0f7126df8df9c738924b94130a0e0c057a06/crates/app/src/launcher/mod.rs)
- [`gpui-shell` panel coordination](https://github.com/andre-brandao/gpui-shell/blob/110f0f7126df8df9c738924b94130a0e0c057a06/crates/app/src/panel.rs)
- [`gpui-shell` display-state workarounds](https://github.com/andre-brandao/gpui-shell/blob/110f0f7126df8df9c738924b94130a0e0c057a06/crates/app/src/state.rs)
- [`gpui-shell` Niri service](https://github.com/andre-brandao/gpui-shell/blob/110f0f7126df8df9c738924b94130a0e0c057a06/crates/services/src/compositor/niri.rs)
