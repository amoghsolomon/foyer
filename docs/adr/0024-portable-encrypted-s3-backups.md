# ADR 0024: Produce portable encrypted canonical backups for S3

- **Status:** Accepted
- **Date:** 2026-08-15
- **Owners:** Foyer project

## Context

ADR 0020 requires encrypted off-host backups and demonstrated restores. Foyer's canonical state is
split deliberately: PostgreSQL owns notes, bookmarks, operation records, and device enrollment;
Radicale owns calendars, tasks, and contacts. PowerSync replicas and PostgreSQL DAV projections are
rebuildable. A raw volume snapshot or an orchestrator-specific backup cannot express these authority
and rebuild boundaries or prove a coherent application restore.

The production target is currently Cloudflare R2, but deployment must remain portable to any
S3-compatible object store and must not depend on Dokploy or another hosting control plane. Local
validation must not require public infrastructure or real cloud credentials.

## Decision

Foyer owns one standalone backup/restore image and host-invoked workflow. It produces timestamped,
self-contained canonical bundles rather than backing up PowerSync or treating Docker volumes as the
restore contract.

A bundle contains:

- a PostgreSQL custom-format logical dump of the Foyer canonical database;
- an archive of the quiesced Radicale canonical storage;
- the Foyer token-signing key and required production secret/configuration files supplied through a
  narrowly mounted secrets directory;
- a versioned manifest containing creation time, service/schema versions, included authorities,
  file sizes, and cryptographic checksums.

The unencrypted staging directory is private, bounded, and removed on success or failure. The bundle
is compressed and encrypted locally to an `age` recipient before upload. Only the public `age`
recipient is present on the VPS backup job; the recovery identity remains offline. The S3 credential
is restricted to the configured bucket and prefix. Secrets are never supplied in command arguments or
logged.

The workflow quiesces Foyer writes and Radicale only while staging canonical state, then restarts them
before the potentially slower encryption and upload. Offline first-party writes remain queued. The
workflow fails closed, uses a host lock to prevent concurrent runs, and reports a nonzero result if
quiescing, dumping, checksumming, encryption, upload, or verification fails. It never deletes the
source volumes.

Configuration uses generic S3 endpoint, region, bucket, prefix, access-key, secret-key, and optional
session-token values. No Cloudflare, Hetzner, AWS, Dokploy, or MinIO API appears in the backup format.
Object-store retention, lifecycle, and bucket-lock policy remain deployment configuration.

Local Compose provides an opt-in `backup-test` profile with MinIO and an isolated test bucket. Tests
seed all canonical domains, create an encrypted bundle, restore it into new temporary PostgreSQL and
Radicale volumes, run migrations/projector rebuilds, and compare normalized API-visible state. Restore
tests never overwrite the active development volumes.

Production scheduling is an external concern. A checked-in systemd service/timer example may invoke
the same workflow, while other schedulers may do so without changing the image or bundle format.

## Alternatives and deliberate exclusions

- Dokploy database, control-plane, and named-volume backups may be used independently but are not a
  Foyer dependency or the authoritative restore format.
- PowerSync storage and client replicas are excluded because they rebuild from canonical state.
- A live byte copy of PostgreSQL storage is excluded; PostgreSQL is captured with `pg_dump`.
- A live Radicale volume archive is excluded because filesystem consistency is not assumed during
  writes.
- Restic remains viable for larger deployments, but timestamped full bundles are simpler and bounded
  for the initial personal dataset and work naturally with object retention.
- Backup encryption does not replace TLS, server-disk protection, least-privilege credentials, or
  object-store retention.

## Consequences and risks

Restores are independent of the deployment platform and one snapshot contains every non-rebuildable
authority needed to recover Foyer. A compromised object store cannot read locally encrypted content.
Losing the offline `age` identity makes every backup irrecoverable.

Staging requires bounded local disk space and a short write interruption. A compromised VPS can still
upload garbage or delete objects allowed by its S3 credential; bucket retention and an independent
restore verifier mitigate but do not remove that risk. Server signing keys in a recovered snapshot may
need rotation after a suspected compromise.

## Validation criteria

- The same image creates and restores a bundle using local MinIO and an S3-compatible endpoint without
  provider-specific branches.
- No plaintext bundle, dump, credential, or encryption identity remains after either success or
  injected failure.
- A restore into empty isolated volumes recovers notes, bookmarks, device enrollment, DAV authority,
  and server signing configuration while rebuilding PowerSync and DAV projections.
- Corrupt, incomplete, wrongly encrypted, or manifest/checksum-mismatched bundles are rejected before
  mutating a restore target.
- Backup and restore are mutually exclusive, idempotent where appropriate, and never operate on an
  unresolved or broad filesystem target.
- Documentation covers manual backup, listing, download, restore drill, retention, key custody, and
  recovery when the original VPS is unavailable.

## Supersession

This ADR selects the backup implementation left open by ADR 0020 without changing its authority map.
