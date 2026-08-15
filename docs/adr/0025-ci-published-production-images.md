# ADR 0025: Publish production images in CI and deploy them explicitly

- **Status:** Accepted
- **Date:** 2026-08-15
- **Owners:** Foyer project

## Context

Foyer's hosted stack contains independently versioned server, bootstrap, PostgreSQL-init,
PowerSync, Radicale, and backup images. Building the Rust service and every adapter on the small
production VPS would consume avoidable CPU, memory, disk, and deployment time. Uploading local image
archives manually is not reproducible and makes it difficult to prove which source revision is
running.

The source repository and images are intended to be public. Dokploy is the current production
control plane, but its administration endpoint is intentionally reachable only through Tailscale.
There is no requirement for an unattended production rollout, and exposing Dokploy merely so a
hosted CI runner can invoke a deployment would weaken that boundary.

## Decision

GitHub Actions builds the production images on every push to `main` and on explicit workflow
dispatch. It publishes OCI images to the repository's public GitHub Container Registry package. The
package uses service-scoped tags:

- `<service>-main` is the moving candidate for the latest `main` revision;
- `<service>-sha-<full-commit>` identifies one source revision for rollback and audit.

All services for one rollout use the same version suffix. The checked-in production Compose file
defaults to `main`; changing `FOYER_IMAGE_VERSION` to `sha-<full-commit>` pins or rolls back the whole
stack coherently. Images carry source and revision labels and CI emits provenance and an SBOM.

Production deployment remains an explicit operator action in Dokploy. The operator reviews the
successful image workflow and then selects Update/Deploy. Dokploy pulls images and never builds the
repository. There is no deployment webhook, public Dokploy endpoint, self-hosted runner, or Dokploy
API credential in GitHub.

Runtime secrets are generated separately and stored in Dokploy's environment or persistent secret
volume. GitHub Actions and the public images never receive database passwords, the DAV password, the
server signing key, the `age` identity or recipient, or S3/R2 credentials. The bootstrap container
creates the server P-256 signing key once in the persistent secret volume and derives Radicale's
bcrypt file from the operator-supplied DAV password. It preserves an existing signing key on every
redeploy.

The production Compose adapter contains no Dokploy-generated Traefik labels. Public domains are
attached through Dokploy's domain UI to the API and PowerSync services. PostgreSQL, Radicale,
bootstrap, and backup receive no public route. The edge network name is configurable so the same
image-pulling stack can be hosted behind a different reverse proxy. ADR 0024's portable backup
remains independent of Dokploy and its volume-backup features.

## Alternatives and deliberate exclusions

- Building from Git in Dokploy is excluded because it moves expensive, cache-heavy compilation onto
  the production VPS.
- Manually exporting and uploading Docker image archives is excluded because it is slow,
  non-auditable, and easy to make inconsistent across services.
- Automatic deployment from GitHub Actions is excluded for now. A hosted runner cannot reach the
  Tailscale-only control plane, and unattended rollout is not required.
- Making the Dokploy panel or deployment API public solely for CI is excluded.
- Floating third-party images directly in production are excluded where Foyer needs checked-in
  configuration or initialization. Thin Foyer images pin those upstream bases and bake only public
  configuration.
- GHCR is the selected publication adapter, not a runtime API or persistence dependency. Another OCI
  registry can replace it by changing CI and `FOYER_IMAGE_REGISTRY`.

## Consequences and risks

The VPS performs only image pulls, migrations, and container replacement. A revision is traceable
and the operator can roll the entire stack back to one commit. Manual promotion also leaves room to
inspect CI before changing personal-data infrastructure.

The first successful publication requires a one-time package visibility change to Public (or a
registry credential if visibility is deliberately kept private). Moving `main` tags are convenient
but not immutable; critical production rollouts should set the commit-scoped version after review.
An upstream base tag can change between builds unless additionally digest-pinned, while the emitted
provenance and commit tag still identify the resulting artifact. Compromise of repository write or
Actions authority could publish a malicious candidate, so branch protection and workflow review are
part of operational hardening.

Manual deployment means security fixes are not live until the operator promotes them. This is an
intentional availability-versus-control tradeoff for the current personal deployment.

## Validation criteria

- A clean GitHub-hosted runner builds all production Dockerfiles without production credentials and
  publishes both moving and commit-scoped `linux/amd64` tags.
- `docker compose config` resolves the production stack from generated environment values without
  a build section, public host port, or missing secret.
- A fresh deployment creates the signing key and Radicale bcrypt file once; subsequent deployments
  preserve the signing key and update only the bcrypt file when its password changes.
- Only the API and PowerSync services join the configurable edge network. PostgreSQL and Radicale
  are reachable only on the private Compose network.
- Selecting one commit-scoped version changes every Foyer image to artifacts from that same source
  revision.
- Production startup still fails closed when required environment or persistent signing material is
  missing or malformed.

## Supersession

This ADR provides the production image-delivery adapter left open by ADR 0019 and ADR 0020. It does
not change ADR 0023's authentication boundary or ADR 0024's backup format and control-plane
independence.
