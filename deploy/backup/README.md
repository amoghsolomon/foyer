# Portable encrypted canonical backups

This directory implements [ADR 0024](../../docs/adr/0024-portable-encrypted-s3-backups.md).
Foyer owns one standalone backup/restore image and a host-invoked Compose
workflow. The restore contract is a timestamped, self-contained canonical
bundle. Docker volumes, PowerSync storage, and any hosting control plane are
not the restore format.

A bundle contains:

- a PostgreSQL custom-format logical dump of the Foyer database (`pg_dump -Fc`)
- an archive of quiesced Radicale storage
- the narrowly mounted production signing/required secret files
- a versioned manifest with timestamp, schema/service versions, authorities,
  sizes, and SHA-256 checksums

PostgreSQL remains the authority for notes, bookmarks, operation records, and
device enrollment. Radicale remains the authority for calendars, tasks, and
contacts. PowerSync replicas and PostgreSQL DAV projections are rebuildable.

## What this does not do

- No Restic, live PostgreSQL byte copy, or named-volume restore contract
- No PowerSync backup
- No Docker socket in the backup image
- No provider-specific object format. R2, AWS S3, Hetzner, and MinIO all use
  the same endpoint, region, bucket, prefix, access key, secret key, and
  optional session token
- Backup jobs receive only the public `age` recipient. The recovery identity
  stays offline

## Local test identity and MinIO

```bash
make backup-age-identity
make backup-test-up
```

That writes `deploy/backup/.local/age-identity.txt` and
`deploy/backup/.local/age-recipient.txt`, starts the opt-in `backup-test`
profile (MinIO on the Compose network as `http://minio:9000`, published on
the host as `127.0.0.1:19000`), runs `minio-init` to create the isolated
test bucket, and leaves restore-test PostgreSQL/Radicale volumes unused until
a drill. Those volumes are separate from `postgres_data` and `radicale_data`.

## Manual backup

Configure `deploy/backup/.local/backup.env` from `backup.env.example`, or
export the same variables. Then:

```bash
make backup-create
```

The host wrapper:

1. takes a host lock
2. stops Foyer Server and PowerSync writers and stops Radicale
3. leaves PostgreSQL running and readable
4. stages the dump, Radicale archive, secrets, and manifest
5. restarts the services it stopped, even if staging fails
6. compresses, encrypts to the public recipient, and uploads
7. wipes the private staging directory on success or failure

Never pass passwords, age identities, or S3 secrets as command arguments.

## List, download, and verify

```bash
make backup-list
./deploy/backup/scripts/host.sh download foyer/foyer-canonical-20260815T031700Z.tar.zst.age \
  /var/tmp/foyer-restore-test.manual/bundle.tar.zst.age
./deploy/backup/scripts/host.sh verify foyer/foyer-canonical-20260815T031700Z.tar.zst.age
```

Verify decrypts and checks the manifest and checksums before any target is
mutated. A corrupt, incomplete, or wrongly encrypted bundle is rejected.

## Isolated restore drill

```bash
make backup-restore-drill
```

When the development stack is not healthy, `make backup-roundtrip` still
exercises dump, encryption, MinIO upload, verify, and `pg_restore` against the
isolated restore-test PostgreSQL volume only.

The drill seeds representative notes, bookmarks, calendars/events, tasks,
contacts, Radicale files, signing-config files, and device/auth rows when those
tables already exist. It then creates an encrypted backup, restores into fresh
temporary isolated volumes, starts the restore-test server so migrations run,
and compares normalized API-visible state. DAV projections are restored from
the PostgreSQL dump; set `FOYER_RESTORE_REBUILD_PROJECTIONS=1` to truncate them
and let the server projector rebuild from Radicale. The script reports which
domains were exercised instead of guessing a concurrently changing schema.

```bash
make backup-test-clean
```

removes only the named restore-test/MinIO volumes and `/var/tmp/foyer-*` test
directories. It never deletes `postgres_data` or `radicale_data`.

## Production scheduling

Scheduling is an external concern. The checked-in systemd unit is an example:

- `systemd/foyer-backup.service`
- `systemd/foyer-backup.timer`

Copy the unit, point `WorkingDirectory` at the checkout, and load
`/etc/foyer/backup.env`. Any other timer or cron can invoke
`deploy/backup/scripts/host.sh create` the same way. Do not put the age
identity on the VPS backup job.

For `compose.production.yaml`, set `FOYER_COMPOSE_FILE`, the Dokploy Compose
project name in `FOYER_COMPOSE_PROJECT_NAME`, and
`FOYER_BACKUP_USE_COMPOSE_SECRETS=1`. The production backup service already
mounts the persistent `foyer-secrets` volume read-only; do not shadow it with a
host directory. The wrapper's project name must match the running project so
it quiesces the existing server, PowerSync, and Radicale containers rather
than creating a parallel Compose project.

## Retention

Object retention and lifecycle stay in the object store. Configure a
prefix-scoped lifecycle rule on `s3://$bucket/$prefix` (for example, keep 30
daily objects). The bundle format does not encode a vendor lifecycle API.
Bucket object-lock or versioning is recommended so a compromised VPS cannot
silently destroy every copy.

## Least-privilege object credentials

Grant the backup job only:

- `s3:PutObject` and `s3:GetObject` on `arn:...:bucket/prefix/*`
- `s3:ListBucket` on the bucket, limited to that prefix

Omit `s3:DeleteObject` on the backup credential. Use a separate, offline
cleanup identity if you expire objects manually. The same policy shape applies
to R2, AWS, Hetzner, and MinIO.

## Age key custody

- Generate the identity offline: `age-keygen -o age-identity.txt`
- Store the secret key off the VPS (paper, hardware token, or a second
  machine). Losing it makes every backup unreadable.
- Put only the `age1...` recipient in `FOYER_BACKUP_AGE_RECIPIENT`.
- After a suspected VPS compromise, restore onto new storage and rotate the
  server token-signing key that was present in the recovered snapshot.

## Disaster recovery when the original VPS is gone

1. Install Docker Compose and this repository on the new host.
2. Recreate an empty stack. Do not attach old live volumes.
3. Place the offline age identity on a secure path and set
   `FOYER_BACKUP_AGE_IDENTITY_FILE`.
4. Point the generic S3 variables at the existing bucket/prefix.
5. `host.sh list`, then restore the chosen object into empty explicit targets
   (`FOYER_RESTORE_POSTGRES_HOST`, `FOYER_RESTORE_RADICALE_DIR`,
   `FOYER_RESTORE_SECRETS_DIR`). Isolated mode is only for the named
   restore-test volumes.
6. Start Foyer Server so migrations run and the projector rebuilds DAV
   projections from restored Radicale. Recreate PowerSync from canonical
   PostgreSQL; do not restore PowerSync storage.
7. Confirm notes, bookmarks, device enrollment, and DAV authority, then rotate
   signing keys if the previous host may have been compromised.

## Make targets

| Target | Purpose |
| --- | --- |
| `make backup-age-identity` | local test age identity |
| `make backup-test-up` | MinIO profile and test bucket |
| `make backup-create` | quiesced backup |
| `make backup-list` | list objects |
| `make backup-restore-drill` | isolated restore drill against the live stack |
| `make backup-roundtrip` | MinIO format roundtrip on isolated restore-test volumes |
| `make backup-test-clean` | remove explicit test resources only |
| `make backup-check` | static, unit, compose, and Python syntax validation |
| `make backup-image` | build the standalone backup image |

## Configuration

See `backup.env.example`. The main Compose `.env.example` is intentionally
unchanged; backup settings stay next to this workflow.
