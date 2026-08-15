# ADR 0013: Present StatusNotifier items in a bounded toolbar popover

- **Status:** Accepted
- **Date:** 2026-08-14
- **Owners:** Foyer Shell project

## Context

ADR 0012 introduced Foyer Shell's StatusNotifier host and placed background-application controls in the
unified full-height panel. In use, that surface is disproportionate: tray applications are
normally a small set of compact identities whose primary interaction is an application-provided
menu. Forwarding `ContextMenu` with fixed coordinates also delegates presentation to inconsistent
external surfaces and prevents Foyer Shell from preserving its visual language.

The toolbar already has a stable ellipsis target and captures its actual painted bounds. A tray can
therefore use a much smaller surface without introducing configurable placement or guessing an
anchor coordinate.

## Decision

The ellipsis target opens one compact layer-shell popover immediately to the left of the toolbar. Its
collapsed content is a three-column by variable-row grid of non-passive StatusNotifier items. The
popover's bottom edge is derived from the ellipsis target's painted lower bound, so additional rows
grow upward. It does not reserve compositor space, shift the toolbar, or move tiled windows.

The tray popover and unified panel are mutually exclusive. Opening either closes the other;
selecting the active ellipsis target, pressing Escape, or removing its output closes the tray.
Only one tray popover may exist across outputs.

Selecting an item replaces the grid within the same popover with that item's option menu. Foyer Shell
reads the StatusNotifier `Menu` object path and the standard `com.canonical.dbusmenu` layout,
supports nested menus, separators, disabled items, and toggle state, and sends standard `Event`
`clicked` messages for chosen leaves. Back returns through a submenu and then to the icon grid.
Items without a usable DBusMenu receive a bounded fallback with Activate and SecondaryActivate;
they do not create another panel.

The service worker, never a GPUI render callback, performs watcher registration, property reads,
menu loading, and method calls. It registers as a host whether Foyer Shell owns or joins the watcher and
retries initial session-bus acquisition. Menu input is bounded to eight levels, 192 items, and
bounded labels before it reaches presentation state. Theme icon names are resolved against the
item's declared theme path and common XDG icon locations; unresolved names use a generic Foyer Shell
glyph.

## Alternatives and deliberate exclusions

- Keeping Application Status in the panel would preserve one surface type but spend an
  entire screen-height control plane on a small icon collection.
- Calling the item's `ContextMenu` method remains a compatibility operation, but it cannot provide
  consistent placement or styling and is not the normal menu path.
- Legacy XEmbed, arbitrary embedded widgets, draggable or configurable tray ordering, and a dock
  remain excluded.
- Raw StatusNotifier pixmap conversion and exhaustive freedesktop icon-theme inheritance are
  deferred. Missing assets degrade to a visible generic icon rather than blocking the tray.

## Consequences and risks

Foyer Shell gains one additional interactive layer-surface class, but its ownership and geometry are
strictly bounded. Application menus now match the Foyer Shell frame and can remain anchored while nested
content changes. Malformed or unusually large external menus are truncated at the trust boundary.

DBusMenu implementations vary and may update layouts after `AboutToShow`; the first implementation
loads a fresh layout when an item is selected and closes after a leaf action. Applications that
expose only remote popup behavior receive the semantic fallback rather than an embedded foreign
surface.

## Validation criteria

- The ellipsis target opens a popover directly to its left with exactly three icon columns and as
  many upward-growing rows as required.
- Passive items are absent; active and attention items remain selectable and attention remains
  visible on the toolbar.
- Selecting an item shows its DBusMenu in the same surface; nested navigation, separators, disabled
  state, toggles, and leaf activation preserve the originating service, menu path, and item id.
- Opening a panel section closes the tray, opening the tray closes the panel, and output
  removal leaves no interactive surface behind.
- Watcher ownership and joining both register Foyer Shell as a host, unavailable session-bus acquisition
  retries, and no D-Bus operation runs during GPUI rendering.

## Supersession

This ADR extends ADR 0006 with one bounded popover exception. It supersedes ADR 0012 only where
that record places StatusNotifier in the Application Status panel section, forwards the
application menu instead of rendering DBusMenu, and retains icon names without resolving them.
ADR 0012's host ownership and typed service-worker boundary remain accepted.

ADR 0018 supersedes this record's former rail terminology with Toolbar.
