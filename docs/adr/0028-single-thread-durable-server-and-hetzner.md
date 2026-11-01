# ADR 0028: Single-thread Pi Durable service and private Hetzner deployment

- **Status:** Accepted
- **Date:** 2026-11-02
- **Owners:** Foyer project

## Context

The user authorized fixing and deploying the server from this public Foyer repository. Android and
Niri client migration, model credentials, memory, retrieval, tools, schedules and delegation are deferred.
The existing Presentation sidecar uses the older coding-agent SDK and intentionally ephemeral sessions.
Hetzner already hosts an independent website behind its own Tunnel; Foyer must not borrow its secrets,
change its resources or expose new public ports.

## Decision

Add an independently supervised Node service using exact npm versions of upstream
`@earendil-works/pi-durable`, `pi-ai`, and `chord` 1.4.0. Its only conversation is `main`.
Use the actual published package API: `Harness.open`, root conversation, `submit`, `entries`,
`watch`, `abort`, and reopening/resuming SQLite. Do not use incompatible unreleased `main` APIs.
No tools, execution environment, application memory, retrieval, schedules or subagents are installed.
No model is selected and no provider is contacted until private provider/model/key configuration is supplied.
Submissions return an explicit `provider_not_configured` error meanwhile; persisted history remains readable.

The agent runs on loopback in the Rust server pod/network namespace. Only Rust exposes the device-authenticated
`/v1/agent` contract, restricted to one configured Foyer user. Device credentials are not forwarded.
The service has no HTTP enrollment route. Snapshot streams close within four minutes to require renewed
authentication; reconnect retrieves current committed state and history is paginated. Thinking blocks,
system prompts and raw provider diagnostics are never public API fields.

Pi SQLite is canonical for this one thread's transcript, submission receipts and execution state.
PostgreSQL remains canonical for the existing personal-data domains and device enrollment. This narrowly
supersedes ADR 0020's future assignment of agent Activities to PostgreSQL for this initial single-thread
runtime only. Do not duplicate the transcript in PostgreSQL or PowerSync. The built-in SQLite adapter
has one writer, protected by a local lease and a single-replica Recreate deployment. Retain its separate
PVC and include it in an operator-managed quiesced backup; existing PostgreSQL/Radicale backups alone
are not a complete backup of the new thread. No new off-host backup account is provisioned here.

A persisted content hash binds each request ID before admission. Pi's own request-ID deduplication
recovers an admission whose HTTP receipt was lost. No external effects are available to this agent.
The personal-data DAV APIs separately bind request intent before external writes and reconcile
conditional writes carrying the same operation marker after projection rollback.

Deploy an isolated `foyer` namespace on Hetzner with ClusterIP-only services and independent PostgreSQL,
PowerSync, Radicale and agent storage. Source reconciliation uses an anonymous HTTPS Flux GitRepository
for this public repository. OCI images are published by GitHub Actions with its built-in token and
promoted by immutable commit tags. Bootstrap secrets are generated privately and applied directly;
plaintext secrets, signing keys and provider credentials never enter Git. No image-pull token is needed
for public GHCR images. The existing website, its Tunnel, OVH and firewall rules remain untouched.

## Consequences and validation

The core durable primitives are upstream-owned. Foyer owns authentication, the wire adapter, host
lifecycle, storage custody and the explicit unconfigured state. Pi Durable is experimental: upgrades
must pin and test one coherent release, including a killed worker's recovery, request replay and cancel.
SQLite's default WAL/NORMAL mode protects process crashes, not every host/power failure. Retain storage
and test restores before treating off-host backup coverage as complete.

Required regressions use real PostgreSQL and Radicale and fail if their prerequisites are absent.
Cover repeated projection, external edits/deletions, duplicate UIDs across users, resource read errors,
unchanged checkpoints on failed reads, stale ETags and retry after accepted DAV/failed SQL projection.
Provider-free agent tests cover persistence, restart, duplicate argument binding, cancellation, stream
reconnection, private-reasoning redaction and unconfigured operation. Live inference and public routing
are separate future operations, not inferred from readiness.
