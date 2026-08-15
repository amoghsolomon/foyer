# ADR 0008: Reserve Workspace 1 with a normal maximized workspace host

- **Status:** Accepted
- **Date:** 2026-08-14
- **Owners:** Foyer Shell project

## Context

The product vision reserves Workspace 1 for Foyer Shell beside the persistent right-edge Toolbar.
The workspace host must be visible only on that workspace, fill the remaining usable area, survive
ordinary close attempts, and show either Overview or Presentation without replacing the Toolbar.

Wayland layer-shell surfaces are associated with outputs rather than Niri workspaces. They are the
right mechanism for the persistent Toolbar and transient overlays, but a layer-shell workspace host would
appear on every workspace. Niri fullscreen also covers Top-layer surfaces, while its
maximized-to-edges state preserves exclusive layer-shell zones such as the Toolbar.

## Decision

Foyer Shell will open one normal GPUI toplevel with the application id `foyer-shell-workspace`. The repository-owned
Niri configuration declares a named `foyer-shell-workspace-1`, opens the workspace host there, and requests
maximized-to-edges rather than fullscreen. The workspace name is its stable identity; Foyer Shell keeps
it at index 1 on its configured output.

The native workspace policy observes Niri's authoritative workspace and toplevel snapshots. It
moves the workspace host back if it leaves `foyer-shell-workspace-1`, moves ordinary toplevels found on `foyer-shell-workspace-1` to
the next workspace on the same output, and restores the named workspace to index 1. Commands run
away from GPUI rendering and use window and workspace ids rather than mutable indices.

The initial overview contains only live date and time, one hardcoded paragraph describing upcoming
work, and one hardcoded thoughtful line. It uses white text on the shared black Foyer Shell background
without cards, boxes, agent execution, or fabricated product data.

Presentations render inside the same workspace host window and replace the preserved Overview for
their duration. They do not create another Niri toplevel or cover the Toolbar. ADR 0016 defines
durable Presentation playback and the Overview-to-Presentation transition.

The active Niri configuration is maintained in the repository and installed through an explicit
symlink after validation. Replacing the user's config remains a deliberate operation with a backup;
Foyer Shell never rewrites it at runtime.

## Alternatives and deliberate exclusions

- A layer-shell workspace host would be output-scoped and visible on ordinary workspaces.
- True fullscreen would hide the Top-layer Toolbar.
- A separate workspace-host process would add IPC and lifecycle boundaries before the host and Toolbar need
  independent failure domains.
- Agent input, Activities, approvals, agenda data, artifacts, and transient work animation are not
  part of this slice.

## Consequences and risks

The host participates in Niri's workspace model while the Toolbar remains a compositor-level anchor.
The configuration currently pins `foyer-shell-workspace-1` to `eDP-1`, so changing the primary output requires
an intentional repository config edit. Workspace-policy corrections may briefly observe an invalid
placement before Niri applies the typed move command.

## Validation criteria

- `Mod+1` focuses `foyer-shell-workspace-1` even when workspace indices change.
- The workspace host fills all usable space left of the Toolbar and does not appear on other workspaces.
- Closing or moving the workspace host does not permanently remove it from Workspace 1.
- An ordinary window placed on `foyer-shell-workspace-1` moves to the next workspace on the same output.
- Restarting Foyer Shell while another workspace is focused does not switch focus to the overview.
- The repository configuration passes `niri validate` before becoming the active symlink target.

## Supersession

This ADR extends ADR 0001, ADR 0002's retained state-ownership rules, ADR 0005, and ADR 0006. It
does not supersede them.

ADR 0018 supersedes this record's former foyer/canvas terminology and names the shared views
Overview and Presentation.
