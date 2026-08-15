# ADR 0006: Right-edge toolbar and unified panel

- **Status:** Accepted
- **Date:** 2026-08-13
- **Owners:** Foyer Shell project

## Context

The first desktop slices proved GPUI layer-shell behavior with a horizontal top bar, centered
launcher, right drawer, and small audio, network, and power popovers. That arrangement works, but
each new capability would add another transient shape and anchor. It also leaves little stable
space for future agenda, Activity, approval, and agent-progress entry points.

The older Deepin control-center interaction demonstrates a stronger spatial model: one persistent
edge toolbar remains the user's anchor while a full-height plane is revealed between that toolbar and
the screen edge. Foyer Shell can adopt that model without adopting Deepin's visual styling or a
customizable module system.

## Decision

### Persistent frame

Foyer Shell will render one fixed-width vertical toolbar at the right edge of every output. The toolbar never
moves: its controls remain at stable screen coordinates while content opens and closes. The
collapsed toolbar reserves only its own width from Niri's usable area. The toolbar has two curated groups:

- workspace context, search, agenda, reminders, and Activity utilities grow from the top; and
- notifications, audio, network, Bluetooth, display, and session controls grow from the bottom.

Controls use `gpui-component`'s Lucide icon system with tooltips and accessibility identifiers.
Foyer Shell may bundle missing Lucide glyphs through its asset source rather than substituting Unicode
symbols or permanent text labels. Workspace numbers, time, state values, headings, and explanatory
copy remain text because they communicate data rather than label compact icon controls.

The otherwise monochrome toolbar may carry one cyan, blue, violet, and magenta ambient waveform behind
its controls. Its color fields are spatially offset, and its motion uses periodic harmonics that
join without a visible loop seam. The band is centered in the open space between the two control
groups, rolls off slowly to full transparency before reaching either group, and does not encode
agent or service state. Its rolling wave, breathing luminance, and traveling crest may be clearly
visible within that bounded space. Reduced-motion mode keeps the same band without continuous
movement.

The toolbar currently omits a persistent main-agent form. The earlier monochrome living-thread preview
was removed pending a separate decision about the agent's visual language and rendering approach.
The Activities utility remains the stable entry point in the top group. Foyer Shell will not replace the
removed form with a placeholder, local state preview, or animation that fabricates unobserved agent
progress. Reintroducing a persistent agent representation requires an explicit update to this
decision after its authoritative state source and presentation are defined.

### Unified panel

Launcher, utility, system-control, and power content are sections of one panel
model. Opening a section creates one full-height overlay immediately to the left of the stationary
toolbar on the invoking output. The content slides out from underneath the toolbar toward the left. The
panel and toolbar read as one edge-to-edge composition, but every toolbar target stays under the pointer.
Only the toolbar width remains exclusive; opening the panel does not reflow Niri's tiled windows.

Selecting another icon switches section in the existing expanded surface. Selecting the icon for
the currently open section closes it. All curated system sections remain visible in the collapsed
and expanded toolbar. The bottom clock is a non-interactive status readout without button chrome and
becomes a close icon in the exact same screen position while expanded.
Escape, the close icon, or the active section icon removes the overlay immediately. Opening and
section switching are also immediate: the panel does not animate its surface, header, or content,
and closing does not hold an invisible layer surface open for an exit animation.
Focus moving between the
panel, stationary toolbar, and another application does not implicitly dismiss it; focus is not a
reliable proxy for intent across adjacent layer surfaces. At most one interactive panel is
open across outputs. That limit is a lifecycle invariant rather than a rendering convention: a
panel remains registered while its window is closing, a requested replacement waits for the close
notification, and the window factory refuses to create a second panel while any panel handle is
registered. Dismissal requested from a panel interaction is queued on the application rather than
the originating window, so focus transfer or application launch cannot discard the close request.

StatusNotifier applications are the narrow exception defined by ADR 0013: the ellipsis target
opens a compact three-column tray popover rather than consuming a full-height system section.
Opening that popover closes the panel, and opening any panel section closes it, so
there is still only one interactive Foyer Shell transient at a time.

Search keeps desktop-entry indexing, fuzzy matching, keyboard selection, and launch behavior, but
renders in the panel instead of a centered Raycast-style window. Audio, network, Bluetooth,
display, and confirmed power/session actions move into panel sections. Agenda, reminders,
notifications, and Activities receive stable section identities before their backends arrive so
future work does not add new surface types.

### Ambient surfaces

Notifications and OSDs remain independent, non-focus layer surfaces. Notification cards stack
immediately left of the collapsed toolbar from the bottom. OSDs and future live agent actions appear
immediately left of the toolbar from the top. When the panel is open, ambient surfaces remain
separate and must not take keyboard focus.

## Alternatives and deliberate exclusions

- Keeping the top bar would avoid migration but would preserve an increasingly fragmented set of
  popovers and drawers.
- A generic settings section is excluded until it has concrete controls that do not belong in an
  existing system section; static implementation and service-status facts do not justify a toolbar
  destination.
- Reserving the expanded panel width would cause every open and close to reflow tiled application
  windows.
- Arbitrary toolbar ordering, third-party modules, visual themes, and user-selected panel geometry
  remain excluded. The toolbar is a curated first-party product surface.
- Permanent text labels are not used on compact toolbar controls; tooltips and accessible names carry
  their descriptions.

## Consequences and risks

Foyer Shell gains one recognizable frame for current system UI and later agent-native capabilities. The
number of layer-surface classes remains deliberately small, section switching becomes predictable,
and new first-party features normally add content rather than another window type. ADR 0013 adds
one bounded exception for StatusNotifier items because a full-height settings surface is
disproportionate to a small application-icon grid and its menus.

The panel and toolbar are separate adjacent layer surfaces. Their seam, animation, focus-loss behavior,
and per-output replacement must be validated under Niri.
Icon-only navigation also makes tooltips, selected state, and accessibility metadata mandatory.
The curated toolbar must stay bounded so it does not become a dock or a configurable widget strip.

## Validation criteria

- Every output has exactly one collapsed right toolbar and reserves only the toolbar width.
- Opening a section reveals content to the toolbar's left without moving the toolbar or reflowing Niri
  windows.
- Selecting another section reuses the open surface and preserves its output.
- Launching an application closes Search even if focus transfers before the current effect cycle
  ends; opening another section during closure waits and cannot overlap an orphaned panel.
- Instrumented rapid open, close, launch, section-switch, and output-switch sequences never create
  more than one panel window or leave an unregistered panel surface.
- Search focuses immediately and retains keyboard navigation and application launching.
- System controls and power confirmations retain their typed service boundaries.
- Toolbar controls render Lucide icons, expose descriptive tooltips, and have no placeholder text
  labels.
- The toolbar contains no placeholder agent form, local phase preview, or fabricated progress state.
- Notifications appear bottom-adjacent to the toolbar and OSDs top-adjacent without taking focus.
- Output removal closes its toolbar, panel, tray popover, notification surface, and OSD exactly
  once.

## Supersession

This ADR supersedes ADR 0002's surface model, ADR 0004's ambient-surface placement, ADR 0005's
power-popover placement, and ADR 0001's initial top-bar mapping. It preserves their platform,
visual-language, ownership, protocol, safety, and recovery decisions.

ADR 0014 supersedes this record only where it reserves and labels the third utility slot as
Reminders. The slot keeps its placement and stable panel behavior but ships as Tasks once
the EDS task-list provider is available.

ADR 0018 supersedes this record's former rail/reveal-panel names with Toolbar and Panel.
