# ADR 0003: Command-backed system-service adapters for the first desktop iteration

- **Status:** Superseded
- **Superseded by:** ADR 0010
- **Date:** 2026-08-13
- **Owners:** Foyer Shell project

## Context

ADR 0002 assigns non-compositor facts and mutations to typed service adapters in
`crates/services`. The first control-center implementation needs real audio, network, and display
brightness state without binding GPUI views to service-specific IPC or substantially expanding the
dependency and unsafe-code surface before the product interactions stabilize.

The supported Fedora/Niri environment already provides WirePlumber's `wpctl`, NetworkManager's
`nmcli`, `brightnessctl`, and the Linux backlight sysfs interface. These commands provide narrower
and more stable initial boundaries than parsing raw PipeWire graphs or implementing the full
NetworkManager D-Bus API inside the shell process.

## Decision

The first service crate will expose typed snapshots and semantic commands while using these local
backends internally:

| Service | Authoritative reads | Mutations |
| --- | --- | --- |
| Audio | `wpctl get-volume` and `wpctl inspect` | `wpctl set-volume` / `set-mute` |
| Network | `nmcli` general, radio, device, and cached Wi-Fi queries | `nmcli radio wifi` |
| Brightness | `/sys/class/backlight` | `brightnessctl set` |

One service worker owns polling and command execution away from GPUI's render thread. Every poll
rebuilds a complete snapshot. A failed query publishes an unavailable state, and later polls retry
without requiring the desktop process to restart. Views only read the latest snapshot and send
semantic commands through a controller.

Commands are invoked directly with argument arrays and a fixed `C` locale. They never pass through
a shell, interpolate user-authored command text, or expose the general command runner outside the
service crate. Volume and brightness are clamped, with brightness retaining a one-percent floor.

The public typed API is the long-lived boundary; the command-backed internals are replaceable. We
will move an adapter to native event-driven PipeWire or D-Bus integration when polling latency,
process overhead, richer device/network operations, or command-output compatibility becomes a
measured problem.

## Alternatives and deliberate exclusions

- Direct PipeWire and WirePlumber bindings would provide events and richer graph state, but add a
  larger native integration before the required UI behavior is known.
- Direct NetworkManager D-Bus integration is a likely later replacement when connection selection,
  secrets, and access-point lifecycle enter scope.
- GPUI views will not invoke `wpctl`, `nmcli`, `brightnessctl`, or read sysfs directly.
- The first network mutation is limited to the explicit Wi-Fi radio switch. Joining networks,
  handling credentials, VPN controls, and airplane mode remain out of this slice.

## Consequences and risks

The desktop gets real controls with a small implementation and keeps service failures isolated from
rendering. The cost is periodic subprocess work and reliance on machine-readable output from local
system tools. Parsers therefore have fixtures, errors become visible availability state, and the
worker continuously retries.

Commands can still stall in pathological system-service failures. If observed, the adapter must
add bounded child-process timeouts or move to native asynchronous APIs before it grows additional
responsibilities.

## Validation criteria

- Parser tests cover normal, muted, escaped, empty, and malformed service output.
- Removing or denying a backend marks only that service unavailable and later recovery republishes
  a complete snapshot.
- Audio volume/mute, Wi-Fi radio state, and brightness changes round-trip through a refreshed
  authoritative snapshot.
- No service query or mutation runs from a GPUI render callback or blocks the render thread.

## Supersession

This ADR extends ADR 0002. It does not supersede it.

ADR 0010 supersedes the single-worker implementation and expands the daily-driver service set. It
retains this ADR's typed state, semantic commands, direct argument execution, failure isolation,
and replaceable backend boundary.
