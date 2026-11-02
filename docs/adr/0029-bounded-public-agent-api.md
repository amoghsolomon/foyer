# ADR 0029: Bound the public personal-agent API

- **Status:** Accepted
- **Date:** 2026-11-02

## Context and decision

The owner requested a safety audit and the hostname `foyer.amoghs.net`. The bare
single-thread foundation remains provider-free and has no enrolled production
devices. ADR 0023's manual device enrollment stays authoritative: there is no
HTTP registration, recovery, pairing or first-visitor ownership mechanism.

Publish one exact-host, anchored-path rule through the existing Hetzner Tunnel.
`deploy/cloudflare/foyer-route.yaml` is the complete rule fragment. It exposes
only `/`, the three version-one authentication endpoints, `/v1/session`, and the
six existing agent endpoint forms. Internal health/readiness, personal-data
CRUD, PowerSync, PostgreSQL and DAV are excluded. The existing apex application
and terminal HTTP404 rule are preserved. No inbound port, new Tunnel token,
Cloudflare Access account, CNI or external database route is introduced.

Before public routing, apply aggregate API admission of 300 requests/minute,
64 active request/response bodies, 1MiB bodies, 10-second uploads, and 60-second
handler execution. Agent requests additionally retain their 32KiB input limit,
30-second non-stream upstream deadline, eight-stream sidecar cap, 100-entry
history pages and a stream deadline at the earlier of JWT expiry or 240 seconds.
The aggregate budget ignores forwarded IP headers; it is a personal-service
resource ceiling, not a fair per-user distributed abuse-control system. Health
probes bypass admission and stay outside the public Tunnel path allowlist.

Sanitize internal 5xx details, retain explicit provider-not-configured responses,
set no-store/nosniff headers, and reject unissued/malformed JWT claims. Unknown
session IDs cannot create persistent security-audit rows. Enrolled-device
challenge rejection remains audited. CORS is not enabled; authentication uses
explicit bearer headers, not ambient cookies or forwarded identity headers.

## Deliberate exclusions and consequences

No browser chat UI, automatic enrollment, model credentials or inference,
memory, agent tools, schedules, or client migration is included. An operator
must enroll the owner's public device key before authenticated use. A public
URL therefore does not grant a visitor ownership or session credentials.

JWT revocation retains ADR 0023's five-minute issued-token window (plus five
seconds of validation clock leeway). Streams now end at signed expiry. The
process-local aggregate budget can be exhausted by a noisy visitor and resets
on restart. Cloudflare mitigations and app quotas do not guarantee availability
or absence of vulnerabilities. Flannel currently has no policy enforcer; the
Foyer-only connector allowance describes intended policy, not current network
isolation. Node-local data backups and future provider enablement need their
previously documented follow-up checks.

The scope excludes an unrequested cryptographic backend rewrite. The audited
P-256 wire protocol and ring JWT signer remain unchanged. Dependency advisories
and their reachability are documented in the exposure audit; a clean scanner
result is not claimed when a version warning remains mitigated or test-only.

## Verification

Real PostgreSQL/DAV full suite, signed non-owner denial for every agent action,
unknown-session no-write regression, no-registration route checks, strict token
claims, expiry-bound stream calculation, retained-response concurrency and
stalled/oversized-upload tests. Verify pinned images and private rollout before
activating the exact-host rule. After activation, verify root and JWKS over
HTTPS, protected-route denial, excluded-path404, valid certificate, unchanged
apex200/wildcard404, no public internal-service route and unconfigured provider.
This is a scoped assessment, not a security guarantee.
