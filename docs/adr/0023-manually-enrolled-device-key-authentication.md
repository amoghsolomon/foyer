# ADR 0023: Authenticate manually enrolled devices with public keys

- **Status:** Accepted
- **Date:** 2026-08-15
- **Owners:** Foyer project

## Context

ADR 0020 requires production authentication, per-device revocation, and a server trust boundary for
personal data. ADR 0021 deliberately left production identity open and currently permits only a
development bearer token and symmetric PowerSync key. Foyer is a closed personal deployment with at
most a few known first-party devices. It does not need public registration, email identity,
passwords, account recovery, federation, or a general identity provider.

Native applications cannot keep a bundled client secret. Long-lived bearer credentials would also
turn a copied token into durable device access. The operator already has SSH authority over the VPS
and can explicitly admit the small number of devices.

## Decision

Foyer authenticates manually enrolled device signing keys. The initial algorithm is ECDSA P-256 with
SHA-256. Algorithm negotiation is excluded.

Each Android or Foyer Shell installation generates its private key locally and exports only a public
JWK plus a human-readable SHA-256 thumbprint. Android stores a non-exportable key in Android
Keystore. Foyer Shell stores its key through the desktop Secret Service boundary and exports its
public JWK to an operator-readable path. Private keys never cross the client boundary.

The operator manages devices only through a local `foyer-admin` command executed with VPS or Compose
authority. The command can add, list, and revoke devices and binds every device to the one explicit
Foyer user. There is no HTTP registration, pairing, invitation, OTP, recovery, or device-management
endpoint. Losing all clients requires VPS/SSH administration.

The canonical device identifier is the RFC 7638-style base64url SHA-256 thumbprint of the normalized
P-256 public JWK members. The client knows this identifier before enrollment; no server-generated
identifier must be copied back into the client.

Authentication is a bounded challenge exchange:

1. The client requests a challenge using its device thumbprint.
2. The server returns a challenge identifier, expiry, and an opaque base64url signing payload. The
   payload is server-generated, domain-separated, bound to the device, server audience, expiry, and
   at least 256 bits of randomness.
3. The client signs the decoded payload with ECDSA P-256/SHA-256 and returns the challenge identifier
   and signature. The wire signature is the fixed-width IEEE P1363 encoding: the 32-byte big-endian
   `r` value followed by the 32-byte big-endian `s` value, base64url encoded without padding. Android
   converts the DER value returned by `SHA256withECDSA` before transmission.
4. In one database transaction the server verifies the enrolled non-revoked public key, exact
   payload hash, expiry, and signature, then consumes the challenge exactly once.
5. The server issues an asymmetric, five-minute Foyer access JWT containing issuer, API audience,
   user, device thumbprint, issue/expiry times, and a unique token identifier.

Clients obtain a new access token by repeating challenge-response. Foyer issues no refresh token.
The existing development bearer token remains available only when the server explicitly runs in
development mode; production continues to fail closed when production signing configuration is
missing.

Foyer uses a distinct audience for PowerSync credentials. After authenticating a Foyer access token,
`/v1/sync/credentials` issues an asymmetric five-minute JWT scoped to the same user and device.
PowerSync receives only the public verification JWK. Development may use a visibly development-only
asymmetric key, but no symmetric verification secret is published to clients.

Authentication endpoints are HTTPS-only outside loopback development. Challenges expire after at
most 60 seconds, are single-use, and are subject to bounded outstanding-challenge and request-rate
limits. Authentication material and authorization headers are redacted from logs. Public keys,
device status, last-seen time, and security audit metadata are not replicated through PowerSync.

The version-one wire endpoints are `POST /v1/auth/challenges`, `POST /v1/auth/sessions`, and the
public verification document `GET /v1/auth/jwks`. JSON field names and cross-language examples live
in `contracts/auth/v1/`; clients depend on those contracts rather than server persistence types.

## Alternatives and deliberate exclusions

- Passkey/OIDC providers are unnecessary for a manually administered two-device deployment.
- Email/password, magic-link, OTP, SMS, public signup, and remotely initiated pairing are excluded.
- Static API keys and long-lived refresh tokens are excluded because possession alone grants access.
- Mutual TLS would move client-certificate handling into every HTTP and PowerSync integration and is
  not selected for the initial native-client path.
- Signing authenticates the device; it does not end-to-end encrypt DAV or PostgreSQL content. Foyer
  Server must continue parsing canonical personal data.
- External CalDAV/CardDAV client authentication remains a separate future decision. Radicale stays
  private for this milestone.

## Consequences and risks

The system has no public account lifecycle and a very small credential surface. Revoking one device
invalidates new sessions immediately and bounds an already issued token to five minutes. Client
private keys are not recoverable from server backups.

The challenge profile is security-sensitive application protocol. Exact payload bytes, signature
encoding, key parsing, thumbprints, expiry, atomic consumption, rate limits, and token claims require
cross-language fixtures and adversarial tests. TLS remains mandatory because signatures provide no
confidentiality. The operator must retain VPS access and securely retain the server token-signing
key.

## Validation criteria

- Android and Foyer Shell independently generate keys and produce the same JWK thumbprints and
  challenge signatures as server fixtures.
- An operator can add, list, and revoke devices without enabling any remote enrollment surface.
- Unknown, malformed, expired, revoked, and replayed challenges fail without issuing credentials or
  leaking whether unrelated user data exists.
- A valid device can continuously refresh five-minute Foyer and PowerSync tokens without user
  interaction, including after application and server restarts.
- PowerSync accepts only the asymmetric public verification key and scopes every stream to the JWT
  user.
- Production refuses development tokens and refuses to start without valid server signing keys.
- Android Keystore and the Shell Secret Service boundary never export or log private key material.

## Supersession

This ADR fills the production-authentication decision left open by ADR 0020 and ADR 0021. It retains
ADR 0021's development-only bearer flow solely for local fixtures.
