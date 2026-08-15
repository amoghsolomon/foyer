# ADR 0019: Adopt the Foyer product monorepo

- **Status:** Accepted
- **Date:** 2026-08-14
- **Owners:** Foyer project

## Context

Foyer Shell and the Foyer Android launcher developed in separate repositories even though they share
the same persistent-agent, Activity, personal-data, permission, and retrieval concepts. The Android
repository also contained an abandoned Cloudflare/Flue JavaScript service whose D1, Vectorize, R2,
Durable Object, and Firebase assumptions are not the intended self-hosted architecture.

The next server and synchronization work changes both clients and their wire contracts together.
Keeping those changes in separate repositories would require coordinating incompatible intermediate
versions while the contracts are still being designed. Conversely, combining their runtime builds
or internal domain types would erase useful trust and deployment boundaries.

ADR 0018 retained the historical `shell` Git repository when it renamed the desktop product to
Foyer Shell. The product is now broader than the desktop component, and the existing `foyer`
repository already provides the Android application and a product-level layout.

## Decision

The `foyer` Git repository becomes the canonical product monorepo. It contains:

- `apps/shell`, the imported Foyer Shell repository and full history;
- `apps/android`, the Android launcher;
- `services/server`, the independently deployable Rust server;
- `contracts`, language-neutral versioned wire contracts and fixtures;
- `deploy`, self-hosted deployment configuration; and
- root product documentation and ADRs.

Foyer Shell keeps its own Cargo workspace and lockfile. Android keeps its Gradle build. The server
keeps a separate Cargo package and lockfile. The pinned Node Presentation sidecar remains isolated
below `apps/shell/sidecar` with its npm lockfile. A root orchestration file may invoke component
builds, but no package manager owns the complete monorepo dependency graph.

The abandoned `services/agent` JavaScript service, root pnpm workspace, Cloudflare configuration,
D1 migrations, and Sandbox code are removed. Its documented HTTP surface is retained only as a
clearly marked migration reference; it is not a compatibility promise for the new server.

Product-wide decisions live in root `docs/adr`. Clients communicate with hosted services through
versioned wire contracts. They do not import server persistence models or gain direct database
access merely because the code shares a repository.

The historical `shell` repository remains available as an archived source and redirect after the
monorepo migration is published. It is not deleted as part of the filesystem migration.

## Alternatives and deliberate exclusions

- Keeping separate repositories would preserve their current paths but make early contract and
  migration changes non-atomic.
- Making the Shell Cargo workspace the monorepo root would couple unrelated Android and server
  build orchestration to a desktop-specific dependency graph.
- Porting the abandoned JavaScript service into the monorepo would retain a backend that is being
  replaced and obscure which persistence model is authoritative.
- Sharing server database structs directly with clients is excluded. OpenAPI and versioned events
  are the cross-component boundary.
- A single version number, release train, container, or runtime process for every component is not
  implied by the monorepo.

## Consequences and risks

Contract, migration, Android, and Shell changes can land atomically, and shared product decisions
have one authoritative documentation tree. CI must use path filters and separate caches so routine
changes do not build every toolchain.

The import changes Shell paths and GitHub repository identity. Local scripts, documentation links,
CI, issues, releases, secrets, and deployment references require deliberate migration; Git history
alone does not move GitHub metadata. The archived repository provides recovery and historical links.

The monorepo contains Rust, Kotlin/Gradle, Python deployment helpers, and one pinned Node sidecar.
Root tooling must orchestrate those builds without silently changing their independent dependency
or runtime contracts.

## Validation criteria

- `apps/shell` contains the complete imported Shell tree and history at the selected source commit.
- Shell, Android, the Rust server, and the Presentation sidecar build or test from their documented
  independent roots.
- The deleted Cloudflare service and root pnpm workspace are absent.
- Root documentation links resolve, and all accepted or proposed ADRs remain discoverable.
- Server Docker builds do not send Android, Shell, Gradle, Node, or Cargo build outputs as context.
- The old `shell` remote can be archived without removing the imported history from `foyer`.

## Supersession

This ADR supersedes only ADR 0018's decision that the Git repository and origin remain `shell`.
ADR 0018's product terminology, runtime namespace, durable migration, and component naming decisions
remain accepted.
