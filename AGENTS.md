# Repository guidance

## Required project context

Before planning or implementing product work, read the relevant documents in `docs/`, including:

- `docs/product-vision.md` for the intended product;
- `docs/architecture.md` for the existing Presentation system; and
- every accepted or proposed decision in `docs/adr/`.

Treat ADRs as authoritative. Do not silently work around an accepted ADR. If new evidence invalidates
one, update its status and write a superseding ADR that explains the change.

## Component boundaries

This is a polyglot monorepo with independently built and deployed components:

- `apps/shell/` is the Rust/GPUI Foyer Shell workspace;
- `apps/android/` is the Android launcher;
- `services/server/` is the hosted Rust service; and
- `contracts/` contains versioned wire contracts and compatibility fixtures.

Clients communicate with the server through versioned contracts. Do not make Android or Foyer Shell
depend on server database models or internal Rust types. Keep credentials, production configuration,
database migrations, and deployment authority out of client applications.

## Maintaining ADRs

Add an ADR when work introduces or materially changes a decision that is expensive to reverse,
affects multiple components or product surfaces, or establishes a long-lived constraint. Examples
include dependency/platform choices, crate and process boundaries, state ownership, sync and IPC
contracts, service backends, persistence, permissions, security policy, and compositor integration.

Do not create ADRs for routine implementation details, local refactors, easily reversible UI
choices, or decisions already covered by an existing ADR. Extend an existing ADR when clarifying
the same decision; create the next numbered ADR when making a distinct decision. Record context,
the decision, alternatives or deliberate exclusions, consequences, risks, and validation criteria.

Keep ADRs current as implementation reveals new facts. Use the statuses `Proposed`, `Accepted`,
`Superseded`, or `Rejected`, and link superseding and superseded records in both directions.
