# Production deployment

Foyer's production path is GitHub Actions → public GHCR images → an explicit Dokploy Update/Deploy.
The VPS does not compile the repository, and GitHub cannot reach the Tailscale-only Dokploy panel.
See [ADR 0025](../docs/adr/0025-ci-published-production-images.md).

## 1. Publish the images

Push `main`, or run **Actions → Publish production images → Run workflow**. The workflow publishes
all six images into one package:

```text
ghcr.io/amoghsolomon/foyer:server-main
ghcr.io/amoghsolomon/foyer:bootstrap-main
ghcr.io/amoghsolomon/foyer:postgres-main
ghcr.io/amoghsolomon/foyer:powersync-main
ghcr.io/amoghsolomon/foyer:radicale-main
ghcr.io/amoghsolomon/foyer:backup-main
```

Every image also receives a `SERVICE-sha-FULL_COMMIT` tag. After the first successful run, open the
repository's **Packages → foyer → Package settings → Change visibility** and make the package
Public. This is a one-time step. No Actions secret is required; the workflow publishes with its
scoped `GITHUB_TOKEN`.

For a reviewed or rollback deployment, set `FOYER_IMAGE_VERSION=sha-FULL_COMMIT` in Dokploy. Leave
it as `main` while iterating.

## 2. Point DNS at the VPS

Create these records at the DNS provider for `i0t.in`:

| Record | Target |
| --- | --- |
| `api.foyer.i0t.in` A (and AAAA if used) | the Hetzner VPS public address |
| `sync.foyer.i0t.in` A (and AAAA if used) | the Hetzner VPS public address |

DNS-only records are simplest for the first certificate issuance. Cloudflare proxying can be
enabled later if desired. Keep Dokploy port 3000 Tailscale-only. Only ports 80 and 443 need to be
public for Foyer; the Compose file publishes no host ports.

Radicale deliberately has no public hostname in this milestone. Foyer Server is its only consumer,
as required by ADR 0023; external DAV-client authentication is a separate decision.

## 3. Create the Dokploy Compose service

In the Dokploy UI:

1. Create project **Foyer**, environment **production**, then a **Docker Compose** service named
   **foyer**.
2. Choose the public GitHub repository, branch `main`, and Compose path
   `./deploy/compose.production.yaml`.
3. Leave **Auto Deploy** off. Dokploy must pull images, not build the repository.
4. Generate the environment block locally and paste its complete output into the Compose
   **Environment** tab:

   ```bash
   ./deploy/scripts/generate-production-env.sh
   ```

   The generated passwords are URL-safe hex values. Store the block only in Dokploy or another
   secret manager. Running the command again creates different credentials; do not replace values
   after the database has initialized unless intentionally rotating them.
5. Save, but add the domains before the first deployment.

The `bootstrap` container creates a P-256 signing key in the persistent `foyer-secrets` volume on
the first deployment and keeps it on later deployments. It also writes Radicale's bcrypt file from
`FOYER_DAV_PASSWORD`. PostgreSQL and Radicale use explicit persistent volume names so a Compose
refresh does not silently create new canonical storage.

## 4. Add the public routes

In the Compose service's **Domains** tab, add:

| Host | Compose service | Container port | HTTPS |
| --- | --- | ---: | --- |
| `api.foyer.i0t.in` | `server` | 3583 | Let's Encrypt |
| `sync.foyer.i0t.in` | `powersync` | 8080 | Let's Encrypt |

Use `/` as the path. Do not add domains for `postgres`, `radicale`, `bootstrap`, or `backup`.
Dokploy attaches Traefik to the external `dokploy-network`; the private default network remains the
only path to PostgreSQL and Radicale.

Select **Deploy**. On later revisions, wait for the GitHub Actions workflow to finish, then select
**Update/Deploy** in Dokploy. Because each image has `pull_policy: always`, the moving `main` tags
are refreshed during that operator-triggered deployment.

## 5. Verify before enrolling clients

From a machine on the public internet:

```bash
curl --fail https://api.foyer.i0t.in/health/live
curl --fail https://api.foyer.i0t.in/health/ready
curl --fail https://api.foyer.i0t.in/v1/auth/jwks
curl --fail https://sync.foyer.i0t.in/probes/liveness
```

Also confirm in Dokploy that `postgres`, `radicale`, `server`, and `powersync` are healthy and that
`bootstrap` exited successfully. A completed bootstrap container is expected, not a crash loop.

Build Android release artifacts with `FOYER_API_BASE_URL=https://api.foyer.i0t.in` and
`FOYER_POWERSYNC_URL=https://sync.foyer.i0t.in`. Run Foyer Shell with
`FOYER_API_BASE_URL=https://api.foyer.i0t.in`. Each client exports its public JWK; enroll it with
`foyer-admin` from the `server` container's Dokploy terminal. There is no remote registration API.

## 6. Add R2 later

R2 is independent of the initial application deployment. After creating the bucket and offline
`age` identity, add only the public `age1...` recipient and the generic `FOYER_BACKUP_S3_*` values
shown in [production.env.example](production.env.example). Never put the `AGE-SECRET-KEY-...`
identity on the routine VPS backup job.

Production scheduling still uses Foyer's portable host wrapper and restore format, not Dokploy
volume backups. Follow [backup/README.md](backup/README.md) when the R2 credential is ready; the
Compose project name and production file can be supplied with `FOYER_COMPOSE_PROJECT_NAME` and
`FOYER_COMPOSE_FILE`.
