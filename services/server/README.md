# Foyer server

This directory contains the self-hosted Rust service. Notes and bookmarks are PostgreSQL-canonical.
Tasks, contacts, and calendar are rebuildable projections of official Radicale 3.7.3. Foyer Server
is the only mutation boundary; clients never receive DAV or PostgreSQL credentials. PowerSync Open
Edition replicates user-scoped projection/canonical rows.

## Local process

The server requires PostgreSQL and, in development, a visible static token:

```bash
export FOYER_DATABASE_URL=postgres://foyer:foyer-dev-postgres-password@127.0.0.1:5432/foyer
export FOYER_SERVER_ENV=development
export FOYER_DEV_USER_ID=dev-user
export FOYER_DEV_TOKEN=foyer-dev-token-do-not-use-outside-development
export FOYER_POWERSYNC_URL=http://127.0.0.1:8080
export FOYER_AUTH_API_AUDIENCE=foyer-api
export FOYER_POWERSYNC_AUDIENCE=foyer-powersync
export FOYER_DAV_URL=http://127.0.0.1:5232
export FOYER_DAV_USERNAME=foyer
export FOYER_DAV_PASSWORD=foyer-dev-dav-password-do-not-use-outside-development
cargo run --manifest-path services/server/Cargo.toml
curl http://127.0.0.1:3583/health/ready
```

Prefer `make stack-dev` from the repository root. That starts PostgreSQL, official Radicale,
this server, and PowerSync with localhost-only published ports. Caddy is optional.

The service binds to loopback by default. Containers set `FOYER_SERVER_BIND=0.0.0.0:3583`.
Development authentication fails closed outside `FOYER_SERVER_ENV=development`. Production
requires `FOYER_AUTH_SIGNING_KEY_PATH` (P-256 PEM), `FOYER_AUTH_KEY_ID`, `FOYER_AUTH_ISSUER`,
and `FOYER_AUTH_API_AUDIENCE`. Development generates an ephemeral ES256 key when the PEM
path is unset. PowerSync verifies `GET /v1/auth/jwks`; there is no symmetric JWT secret.

Device enrollment is a local `foyer-admin` command with direct database authority:

```bash
export FOYER_DATABASE_URL=postgres://foyer:foyer-dev-postgres-password@127.0.0.1:5432/foyer
foyer-admin devices add --user-id owner --label phone --jwk ./device.jwk.json
foyer-admin devices list
foyer-admin devices revoke --device-key-id <thumbprint>
```

Challenges last at most 60 seconds and are single-use. Each process allows 8 outstanding
unconsumed challenges per device, 20 challenge requests per device per minute, and 60
challenge or session attempts process-wide per minute. Authorization headers, signing
payloads, signatures, bearer tokens, and private PEM are never logged.

The deleted Cloudflare service is not a compatibility target. Consult
`contracts/legacy-cloudflare-http-api.md` only when identifying remaining Android migration work.
