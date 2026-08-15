# Foyer

Foyer is a personal agent system with two first-party clients:

- **Foyer Shell**, an agent-native Niri desktop environment; and
- **Foyer for Android**, a local-first launcher and mobile agent surface.

The monorepo also contains the new self-hosted Rust service that will own personal data, sync,
retrieval, and the persistent agent boundary. The former Cloudflare/Flue JavaScript service has
been removed; its old HTTP contract remains only as a migration reference.

## Repository layout

```text
apps/android/       Android launcher, Room state, and mobile UI
apps/shell/         Rust/GPUI Foyer Shell and its pinned Node Presentation sidecar
services/server/    Self-hosted Rust API and worker foundation
contracts/          Versioned OpenAPI contracts and migration fixtures
deploy/             Docker Compose and reverse-proxy configuration
docs/               Product vision, architecture, and ADRs
```

Each component has an independent build and lockfile. The monorepo is a source and contract boundary,
not one combined application binary.

## Development

Run targeted checks from the repository root:

```bash
make server-check
make android-test
make shell-check
make sidecar-test
make compose-config
```

`make check` runs all of the above. Android requires a full JDK 17 or newer, an Android SDK, and
`JAVA_HOME` pointing at that JDK. The Rust workspaces are pinned to Rust 1.97.1. Building Foyer
Shell also requires Clang for the native PowerSync SQLite bindings, plus its documented Niri/system
dependencies. The Presentation sidecar remains an isolated npm project with its own
`package-lock.json`.

The Rust server owns the personal-data stack: PostgreSQL-canonical notes and bookmarks, rebuildable
task/contact/calendar projections from official Radicale 3.7.3, manually enrolled device-key
authentication, and PowerSync Open Edition as the replaceable replica transport. See
[ADR 0020](docs/adr/0020-hosted-personal-data-and-replicated-clients.md),
[ADR 0021](docs/adr/0021-powersync-notes-vertical-slice.md), and
[ADR 0022](docs/adr/0022-radicale-dav-and-base-personal-apps.md). Foyer clients never receive DAV
credentials. Production images, Dokploy setup, and the manual promotion flow are documented in
[deploy/production.md](deploy/production.md).

## Local personal-data stack

Prerequisites: Docker, a JDK 17+ and Android SDK for the launcher, Rust 1.97.1 for the Rust
workspaces, and Clang when building Foyer Shell.

```bash
make stack-dev
```

That copies `deploy/.env.example` to `deploy/.env` if needed, starts PostgreSQL (private), official
Radicale 3.7.3 on `127.0.0.1:5232`, Foyer Server on `127.0.0.1:3583`, and PowerSync Open Edition on
`127.0.0.1:8080`, then waits for health. Caddy is optional (`COMPOSE_PROFILES=proxy`). PostgreSQL is
not published to the host. The Radicale htpasswd in `deploy/radicale/users.dev` is an insecure
localhost development default.

Development credentials are visible in `deploy/.env.example` and are rejected unless
`FOYER_SERVER_ENV=development`. Do not use them outside local development.

```bash
make stack-health
make stack-down
curl -H "Authorization: Bearer ${FOYER_DEV_TOKEN}" \
  http://127.0.0.1:3583/v1/session
```

Android debug builds default to `http://10.0.2.2:3583` and the same development token. Foyer Shell
uses `FOYER_API_BASE_URL` (default `http://127.0.0.1:3583`) and `FOYER_DEV_TOKEN`. The Shell notes
adapter uses the native PowerSync Rust SDK and stores its replica separately from
`foyer-shell-storage`. Both clients upload semantic mutations only through Foyer Server.

## Architecture

Read [the product vision](docs/product-vision.md), [the Presentation architecture](docs/architecture.md),
and the accepted decisions in [docs/adr](docs/adr) before changing component boundaries or durable
state ownership.

## License

Foyer-authored source is licensed under the
[GNU General Public License, version 3 or later](LICENSE). Third-party dependencies and retained
upstream artifacts remain under their own licenses. See
[ADR 0026](docs/adr/0026-gplv3-project-license.md) for the project and binary-distribution policy.
