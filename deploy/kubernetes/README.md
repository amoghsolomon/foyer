# Private Hetzner deployment

Source stays in the public Foyer repository. `flux/foyer.yaml` uses anonymous HTTPS,
a separate Flux source and namespace. The `hetzner` overlay is single node, single replica
and uses the existing `local-path-retain` StorageClass. Images are promoted by immutable
source commit tags after the required server and provider-free Pi Durable checks pass.
The release commit can refer to the preceding image-build commit; never promote `*-main`.
The overlay pins all five workload images to verified registry digests from source
commit `ca23de8462a636c3a84bfdf4683e109aff85fdc5`. Update those digests only after
the corresponding source revision passes checks and its images are published.

Always explicitly select kubeconfig and context. This deployment targets the verified
`admin@personal-website-hetzner` context and node `nobuntu`. Never use the default context.
It does not modify website resources, Cloudflare Tunnel, public DNS or firewall rules.
All service endpoints are ClusterIP. The namespace NetworkPolicy requests ingress only
from this namespace. The current Hetzner cluster uses Flannel without a policy controller,
so that policy is not an enforced isolation boundary. API authentication and dedicated
PostgreSQL/DAV credentials remain required. Adding policy enforcement is a separate
cluster change; this deployment does not modify the CNI or expose public ports.
For operator checks, use explicit-context `kubectl -n foyer port-forward service/server 3583:3583`.

## Bootstrap

`bootstrap-secrets.py --help` describes dedicated secret generation. Run it only with
explicit approval, the correct context, and a private directory on `/mnt/storage`.
It preserves existing secrets and refuses incomplete or mixed initialization. The generated
0600 private recovery bundle must remain outside Git and needs a secure off-node backup.
Do not reuse website credentials. No imagePullSecret or repository token is required
when the published GHCR packages are publicly readable.

Apply `flux/foyer.yaml` only once the pinned images are available. Check both Flux
resources, PVC binding, all four workloads, `/health/ready` and PowerSync readiness.
The Rust server requires a production P-256 signing key and owner device enrollment via
`foyer-admin`; enrollment is a separate operator action. The configured agent owner is
`owner`. Other principals cannot access its thread. Model/provider setup is deferred:
the agent is healthy with configured=false and submissions return 503.

## Persistence, restart and recovery

PostgreSQL owns normalized data, auth and mutation intents. Radicale owns DAV resources.
Pi Durable owns its independent SQLite transcript and admission bindings on the `agent`
PVC. The Rust/agent pod uses Recreate and the agent takes a renewable exclusive writer
lease. Abrupt termination can take up to 10 seconds to expire that lease before restart.
Never run multiple writers against the same directory, or use multiple replicas.

All three PVCs are node-local and Retain, not replicated backups. Existing Compose backup
scripts cover PostgreSQL and DAV only; they do not cover this Kubernetes agent store.
Before relying on this service for valuable data, configure an approved off-node backup
location. A consistent manual backup requires quiescing writes, dumping PostgreSQL,
archiving the entire Radicale directory and stopping the agent pod before archiving its
whole directory (SQLite database plus any WAL/SHM files). Keep the private signing/auth
recovery bundle separately encrypted. Restore all three stores into new volumes, restore
runtime secrets privately, then start a single writer. Test restoration before discarding
old volumes. No off-node backup account or scheduled backup is configured by this change.

Roll back code by changing the pinned image revision in Git. Do not reverse migrations
or delete retained volumes as a code rollback. The new intent table is additive. Restart
checks must confirm the same root conversation persists and provider-free submission
continues to return 503; real inference needs a later explicit provider configuration.
