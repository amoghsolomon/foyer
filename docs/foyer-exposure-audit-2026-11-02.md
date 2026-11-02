# Foyer public exposure audit — 2026-11-02

Scope: the deployed Rust API and Pi Durable1.4.0 foundation on Hetzner, plus one
exact hostname through the existing Tunnel. Original Niri/Android changes,
website code, OVH, public firewall rules and model configuration are excluded.

## Findings and disposition

| Severity | Finding | Disposition |
|---|---|---|
| Medium | General API requests had no shared admission/concurrency/upload deadline; authentication-specific limits alone did not cover malformed bodies or arbitrary routes. | Added 300/min aggregate ceiling, 64 retained response permits, 1MiB/10s body bounds, 60s execution timeout and 4096-byte authorization-header limit. No forwarded-client-IP trust. |
| Medium | SSE's fixed240s allowance could extend beyond token expiry, including time spent awaiting upstream. | Absolute monotonic deadline is derived from the verified JWT expiry with millisecond precision, capped at240s. Non-owner cancel/history/stream/submission access returns404 before sidecar transport. |
| Medium | Unknown but syntactically valid session IDs allocated persistent rejection rows. | Unknown IDs return401 without creating audit rows. Actual enrolled-device challenge rejections remain audited. |
| Low | Database/DAV failure details could enter structured5xx response messages. | Rust and contact response boundaries replace internal5xx details with a generic message. Sidecar generic500 and explicit provider-not-configured503 remain. |
| Low | Public data responses lacked a shared no-store/nosniff policy. | Added response headers at the application boundary; no CORS or cookies are introduced. |
| Informational | No active owner devices or users exist in production. | Fail-closed manual CLI enrollment only; no open bootstrap, registration, invitation, recovery or first-visitor ownership route. Do not distribute the signing key or create a server-side client private key. |
| Informational | Flannel does not enforce NetworkPolicy and PVCs are node-local. | Explicitly documented; no claim of enforced namespace isolation or off-node backups. Connector policy selects only existing website Tunnel pods and Foyer API port3583. |

## Dependency review

Current OSV lockfile scan and npm production audit were run. npm reports zero
known advisories in the pinned production agent graph. Compatible fixes applied:
Tokio1.53.3, h2 0.4.16, rustls0.23.46 and rustls-webpki0.103.16. The TLS/HTTP2
packages occur in the development/test graph, not the Linux production graph.

Residual version warnings must not be described as patched:

- `jsonwebtoken9.3.1`, GHSA-h395-gr6q-cpjc: malformed optional claims can skip a
  library check in affected versions. Foyer always requires typed numeric expiry,
  validates issuer/audience/kid/ES256, and now rejects unknown claims including
  `nbf`. Regression cases reject malformed/missing expiry, malformed/unissued
  nbf, expired tokens and wrong issuer/audience. The application does not sign
  user-supplied arbitrary claims. This is an application mitigation, not an
  upgrade of the vulnerable package version.
- `crypto-bigint0.5.5`, RUSTSEC-2026-0387: timing variability in private arithmetic.
  Foyer's public request path uses p256 to verify public signatures; JWT signing
  uses ring's EcdsaKeyPair. P256 private-key/public-key derivation occurs at startup
  or in development/test key generation, not per Internet request. This review
  does not certify microarchitectural/local side-channel resistance. A compatible
  upstream p256 upgrade remains follow-up; do not switch live signing to an
  affected RustCrypto backend as a superficial version-number fix.
- `astral-tokio-tar0.5.6` archive advisories, `rsa0.9.10` Marvin warning and
  unmaintained `rustls-pemfile2.2.0` occur only in test/dev or unused lockfile paths.
  The production API has no archive-upload/extraction, RSA signing/decryption or
  TLS-session-debug endpoint. They remain dependency-maintenance debt.

Sources: [JWT advisory](https://github.com/advisories/GHSA-h395-gr6q-cpjc),
[crypto-bigint](https://rustsec.org/advisories/RUSTSEC-2026-0387.html),
[Tokio](https://rustsec.org/advisories/RUSTSEC-2026-0386.html),
[h2](https://rustsec.org/advisories/RUSTSEC-2026-0258.html).

## Route and trust review

All normal domain handlers require a Principal and scope reads/mutations to the
principal user; existing cross-user DAV identities and writes have integration
coverage. They are not included in the initial public path rule. Agent access is
further restricted to the configured owner. Bearer credentials are never forwarded
to the loopback sidecar. All model/tool capabilities remain disabled/unconfigured.
The public JWK document contains public coordinates and metadata only.

Challenge redemption locks and consumes one challenge once, checks enrolled
non-revoked device identity and fixed-width P1363 signature. Unknown thumbprints
receive dummy challenges without enrollment or persistent challenge allocation.
Authentication has its separate60/min aggregate budget and per-device/session
limits. A revoked device cannot obtain new sessions; previously issued JWTs have
the documented five-minute expiry. Browser-origin headers and proxy forwarding
headers do not select identity. There is no cookie authentication or CORS grant.

The single Tunnel rule permits only API identity/session and bare agent paths.
No PostgreSQL, DAV, PowerSync, admin, setup, personal-data CRUD or readiness route
is published. Existing apex and default unmatched-host404 behavior are preserved.

## Verification limits and remaining work

This is a focused source/dependency and integration review, not a penetration-test
certification or a guarantee against unknown flaws. Lockfile scans do not cover
all operating-system/base-image vulnerabilities or Cloudflare infrastructure.
Valid owner-device HTTPS sessions require later manual device enrollment; the
live checks do not create persistent test credentials. Provider enablement, tools,
new public paths, browser clients or multiple writers require a fresh scope check.
The public endpoint will return service JSON and authenticated API denials, not a
chat UI. Off-node data backups remain to be configured before valuable data.
