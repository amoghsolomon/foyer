# ADR 0026: License Foyer under GPLv3 or later

- **Status:** Accepted
- **Date:** 2026-08-15
- **Owners:** Foyer project

## Context

Foyer is intended to become a public source repository and to ship binaries for its Android,
Foyer Shell, and server components. The monorepo did not contain a root license, while its Rust
packages previously declared `MIT OR Apache-2.0`. A repository-wide license must cover the
Foyer-authored source and establish clear redistribution terms for those binaries without claiming
to relicense dependencies or retained upstream artifacts.

The resolved Rust dependency graph was audited before this decision. Foyer Server's dependencies
offer permissive license choices compatible with GPLv3. Foyer Shell additionally links
GPL-3.0-or-later code through `niri-ipc` and the pinned Zed GPUI tracing crates. Two small Zed
utility crates omit package-level license metadata and therefore fall under Zed's stated default
GPL-3.0-or-later policy; the other package without an SPDX metadata value,
`tree-sitter-graphql`, includes an MIT license file.

PowerSync Open Edition remains an independently distributed external service container as defined
by ADR 0021. Its source-available terms and every other third-party license remain independent of
the license granted for Foyer-authored source.

## Decision

Foyer-authored source in this monorepo is licensed under the GNU General Public License, version 3
or, at the recipient's option, any later version. The repository root contains the complete GPLv3
license text, the README states the scope, and Rust and Node package metadata use the SPDX identifier
`GPL-3.0-or-later`. Published Foyer OCI images carry the same license identifier.

This license grant covers the Foyer-authored Android, Shell, server, contract, deployment, and
documentation source. It does not relicense third-party dependencies, generated dependency locks,
Gradle wrapper material, protocol fixtures derived from published standards, external container
images, or any file that carries a different license or attribution.

Foyer's shipped binaries and their corresponding source will be distributed in accordance with
GPLv3. Release artifacts must retain required notices and provide recipients equivalent access to
the complete corresponding source, including the scripts needed to build and install the covered
work. Public source at the exact released revision is the preferred delivery mechanism.

## Alternatives and deliberate exclusions

- Apache-2.0 would be a permissive license for original source, but the linked Foyer Shell binary
  already contains GPL-3.0-or-later dependencies and would still carry GPLv3 distribution
  obligations.
- `MIT OR Apache-2.0` would preserve the earlier Rust metadata but would not express the intended
  copyleft policy for future Android, server, and Shell binaries.
- Per-component project licenses would permit different policies, but would make shared contracts,
  documentation, and cross-component changes harder to classify.
- Dependency licenses are not copied into the root license or silently treated as GPLv3.

## Consequences and risks

Foyer has one strong-copyleft project license that aligns its source and planned binary releases
with the Shell's strongest linked dependencies. Recipients may run, inspect, modify, and
redistribute Foyer while downstream distributions remain equally open.

Binary releases require corresponding-source and notice compliance. Dependency upgrades may add
new reciprocal, source-available, attribution, or incompatible terms, so release audits must use
the complete resolved dependency graph. Network use by itself does not trigger source-distribution
requirements under GPLv3; selecting AGPL would be a separate decision.

## Validation criteria

- The repository root contains the unmodified GPLv3 license text and the README links to it.
- Every Foyer-authored Rust package declares `GPL-3.0-or-later` directly or through workspace
  metadata.
- The Foyer Presentation sidecar and published OCI image metadata declare `GPL-3.0-or-later`.
- The resolved Shell and Server dependency graphs contain no license reported as missing without a
  documented upstream license determination.
- Every binary release identifies its exact source revision and provides equivalent access to the
  corresponding source and build/install scripts.
- Adding or upgrading a dependency requires reviewing its license and the licenses of newly resolved
  transitive dependencies before release.

## Supersession

This record establishes the repository licensing policy. It does not supersede any architecture or
product decision.
