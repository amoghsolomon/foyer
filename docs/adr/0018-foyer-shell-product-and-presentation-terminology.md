# ADR 0018: Adopt Foyer Shell product and Presentation terminology

- **Status:** Accepted
- **Date:** 2026-08-14
- **Owners:** Foyer Shell project

## Context

The repository began as a narrated-presentation prototype named Shell and then grew into a Niri
desktop environment. Its documentation and implementation consequently use Shell, foyer, canvas,
scene, explanation, rail, launcher, and presentation for overlapping concepts. The ambiguity now
crosses user-facing language, crate boundaries, durable artifacts, service identities, D-Bus,
systemd, Niri rules, environment variables, and XDG storage.

The product is no longer only a presentation prototype. Its name and vocabulary need to describe
the complete desktop while distinguishing Workspace 1, its default view, and narrated content.
At the time of this decision the Git repository remained `shell`; ADR 0019 later moves the component
into the first-party Foyer monorepo without changing its runtime product identity.

## Decision

The product name is **Foyer Shell**. Machine-readable names use `foyer-shell`; Rust crate paths use
`foyer_shell`. “Shell” alone refers only to the generic desktop-shell or layer-shell concept.

The canonical interface vocabulary is:

- **Workspace 1** is the Niri workspace reserved for Foyer Shell. It is a container, not Overview.
- **Overview** is the default Foyer Shell view within Workspace 1.
- **Presentation** is the narrated view that replaces Overview within the same Workspace 1
  toplevel. It is also the durable artifact and compiled domain concept.
- **Toolbar** is the persistent right-edge control strip.
- **Panel** is the full-height content surface opened immediately to the Toolbar's left.
- **Tray popover** is the compact StatusNotifier exception.
- **Slide** is one authored unit within a Presentation.
- **Activity** and **run** keep their existing durable meanings.
- **Surface** is reserved for technical Wayland and layer-shell objects.
- **Niri overview** names Niri's compositor-wide zoomed-out workspace/window mode.
- **Search** is the application-search section and command; “launcher” is only a descriptive
  capability, not a product-surface identity.

The main desktop package and executable are `foyer-shell`. Supporting packages, executables,
systemd units, app ids, layer namespaces, sockets, and Node package names use the
`foyer-shell-*` prefix. D-Bus interfaces use `org.amazity.FoyerShell.*`, environment variables use
`FOYER_SHELL_*`, and XDG state lives below `foyer-shell`.

The Presentation implementation is divided into `foyer-shell-presentation` for compilation,
durable bundles, and replay state; `foyer-shell-presentation-player` for the renderer and playback
controller; and `foyer-shell-presentation-ui` for Presentation-specific components. Scene and
explanation are no longer code or artifact identities. They may appear only as ordinary prose or
in an explicitly historical compatibility migration.

Existing `$XDG_DATA_HOME/amazity-shell` and `$XDG_CONFIG_HOME/amazity-shell` directories are moved
once to their Foyer Shell names when the new paths do not already exist. The database catalog,
database filename, saved-Presentation directory, manifest identifier, and compiled-Presentation
filename receive explicit one-time migrations. Old service, D-Bus, command, and environment names
do not remain as permanent aliases.

The retained Bevy Stage and PixiJS renderer/web-host experiments are removed. The GPUI
Presentation path is the only supported renderer.

## Alternatives and deliberate exclusions

- Keeping **Shell** as the product name would preserve identifiers but no longer distinguish the
  product from its generic desktop-shell role.
- Naming Workspace 1 itself **Overview** is excluded because Presentation also occupies it.
- Keeping **rail**, **canvas**, **scene**, or **explanation** as internal synonyms would preserve
  the ambiguity this decision removes.
- Maintaining indefinite aliases for old binaries, services, D-Bus names, environment variables,
  or storage paths is excluded. The project uses a bounded migration instead.
- Renaming the Git remote or repository directory was excluded by this record. ADR 0019 supersedes
  that exclusion after the Android and desktop products adopt one monorepo.

## Consequences and risks

The product, documentation, UI, durable model, and runtime namespace now share one vocabulary.
Commands and deployment artifacts become easier to discover, and Overview and Presentation have
an explicit shared host rather than being described as different applications.

The rename is intentionally breaking for installed service and IPC names. Installation must
replace old user units and bindings. The one-time filesystem and database migrations must be
idempotent and must not overwrite an existing Foyer Shell destination. Historical ADR prose may
retain a superseded term when it is necessary to explain the original decision; this ADR is the
authoritative mapping for current work.

## Validation criteria

- The component remains identifiable as Foyer Shell regardless of its repository location.
- The workspace builds without the Bevy Stage, PixiJS renderer, or web host.
- The primary executable is `foyer-shell`, and supporting crates and services use the
  `foyer-shell-*` prefix.
- Current code and user-facing documentation use Overview, Presentation, Toolbar, Panel, Search,
  Activity, run, and slide according to this ADR.
- D-Bus, systemd, Niri, XDG, socket, app-id, and environment names use the Foyer Shell namespace.
- A legacy data root, Presentation bundle, and schema-v4 catalog migrate once without overwriting
  an existing destination.
- Overview and Presentation render in the same Workspace 1 toplevel while the Toolbar remains
  stationary.

## Supersession

This ADR supersedes only the terminology and namespace portions of ADR 0006, ADR 0008, ADR 0011,
ADR 0013, ADR 0015, ADR 0016, and ADR 0017. Their platform, ownership, safety, interaction,
persistence, and service-boundary decisions remain in force.

ADR 0019 supersedes only this record's Git-repository identity and remote-location decision. The
Foyer Shell product name, terminology, runtime namespaces, migration rules, and component boundaries
remain accepted.
