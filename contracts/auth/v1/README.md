# Auth contract v1

Language-neutral wire contract for manually enrolled device-key authentication
(ADR 0023). Android and Foyer Shell depend on these documents and fixtures, not
on server database models or internal Rust types.

There is no public registration, pairing, invitation, OTP, password, recovery,
refresh token, or remote device-management endpoint.

## Endpoints

| Method | Path | Auth | Purpose |
| --- | --- | --- | --- |
| `POST` | `/v1/auth/challenges` | none | Start a one-shot signing challenge |
| `POST` | `/v1/auth/sessions` | none | Consume a challenge and receive an access JWT |
| `GET` | `/v1/auth/jwks` | none | Public ES256 verification JWKS |

Authenticated API calls, including `GET /v1/sync/credentials`, send
`Authorization: Bearer <accessToken>`.

`GET /v1/dev/jwks` is a development-only alias of the public JWKS. It is absent
outside `FOYER_SERVER_ENV=development` and never publishes a symmetric secret.

## Device identifier

The canonical device identifier (`deviceKeyId`) is the RFC 7638 JWK thumbprint
of the P-256 public JWK:

1. Normalize `x` and `y` to unpadded base64url encodings of the 32-byte
   coordinates.
2. Hash the exact UTF-8 bytes of
   `{"crv":"P-256","kty":"EC","x":"<x>","y":"<y>"}` with SHA-256.
   Member order, spelling, and the absence of whitespace are mandatory.
3. Encode the digest with base64url and no padding (43 characters).

Clients compute this identifier before enrollment. The operator copies the
public JWK into `foyer-admin`; the server does not mint a second identifier.

`fixtures/rfc7517-public.jwk.json` and `fixtures/thumbprint.txt` are the
RFC 7517 Appendix A.1 / RFC 7638 Appendix A vector.

## Challenge

Request (`examples/challenge-request.json`):

```json
{ "deviceKeyId": "<thumbprint>" }
```

Response (`examples/challenge-response.json`):

```json
{
  "challengeId": "<uuid>",
  "signingPayload": "<base64url-no-pad>",
  "expiresAt": "<RFC3339>"
}
```

`signingPayload` is opaque. Clients decode it and sign the raw bytes. They must
not parse the payload.

The server constructs those bytes as:

```text
"FOYER-AUTH-CHALLENGE-V1" || 0x00 || deviceKeyId || 0x00 || apiAudience
  || 0x00 || expiresAt (RFC3339 UTC seconds, e.g. 2026-01-01T00:01:00Z)
  || 0x00 || nonce
```

`nonce` is at least 32 cryptographically random bytes. Lifetime is at most 60
seconds. The payload is bound to the device thumbprint, the Foyer API audience,
and the expiry.

Unknown, revoked, or syntactically unexpected device identifiers receive a
response with the same JSON shape. Those dummy challenges are not persisted and
cannot be redeemed. This avoids leaking enrollment or revocation status.

## Signature

Algorithm: ECDSA P-256 with SHA-256.

Wire encoding: IEEE P1363, a fixed 64-byte concatenation of big-endian `r` and
`s`, then base64url without padding. Android must convert the DER value from
`SHA256withECDSA` before transmission. DER, JOSE compact signatures, and padded
base64 are rejected.

`fixtures/signing-payload.b64` and `fixtures/signature.b64` are a deterministic
RFC 6979 signature of a documented payload using the RFC 7517 example key.

## Session

Request (`examples/session-request.json`):

```json
{ "challengeId": "<uuid>", "signature": "<p1363-base64url>" }
```

Response (`examples/session-response.json`):

```json
{
  "accessToken": "<jwt>",
  "tokenType": "Bearer",
  "expiresAt": "<RFC3339>",
  "userId": "<user>",
  "deviceKeyId": "<thumbprint>"
}
```

One database transaction verifies the enrolled non-revoked key, the exact stored
payload, expiry, and signature, then consumes the challenge exactly once.
Unknown, malformed, expired, revoked, and replayed attempts fail with the same
generic `401` / `unauthenticated` body. There is no refresh token; clients
repeat challenge-response.

## Access JWT

ES256, five minutes, header `kid` matching `GET /v1/auth/jwks`.

Claims (`examples/access-claims.json`):

| Claim | Value |
| --- | --- |
| `iss` | Configured issuer (`FOYER_AUTH_ISSUER`) |
| `aud` | Foyer API audience (`FOYER_AUTH_API_AUDIENCE`, default `foyer-api`) |
| `sub` | Foyer user id |
| `deviceKeyId` | Device thumbprint |
| `iat` / `exp` | Unix seconds, `exp = iat + 300` |
| `jti` | Unique token id |

Production refuses to start without a valid PEM signing key, key id, issuer,
and API audience. The static development bearer token is accepted only when
`FOYER_SERVER_ENV=development`.

## PowerSync JWT

`GET /v1/sync/credentials` requires an authenticated principal and issues a
distinct-audience five-minute ES256 JWT. Claims match the access token except
`aud` is `FOYER_POWERSYNC_AUDIENCE` (default `foyer-powersync`). PowerSync
verifies only the public JWKS at `/v1/auth/jwks`. No symmetric verification
secret is published.

## Limits

Implemented per Foyer Server process (reset on restart). Sufficient for a
personal two-device deployment; not a public-signup budget.

| Limit | Value |
| --- | --- |
| Challenge lifetime | 60 seconds |
| Outstanding unconsumed, unexpired challenges per device | 8 |
| Challenge requests per device per 60 seconds | 20 |
| Challenge requests process-wide per 60 seconds | 60 |
| Session attempts per challenge id per 60 seconds | 30 |
| Session attempts process-wide per 60 seconds | 60 |

Exceeded limits return `429` / `rate_limited` without saying whether the device
exists.

## Logging

Servers must not log `Authorization` headers, challenge payloads, signatures,
bearer tokens, private keys, or private PEM. Audit rows store only event type,
user id, device thumbprint, challenge id, and time.

## Operator enrollment

`foyer-admin` talks to PostgreSQL on the VPS. It is not an HTTP API.

```text
foyer-admin devices add --user-id <id> --label <label> [--jwk <file>]
foyer-admin devices list [--user-id <id>]
foyer-admin devices revoke --device-key-id <thumbprint>
```

`add` reads a public JWK from `--jwk` or stdin, rejects anything other than a
P-256 public key, prints the thumbprint, and creates the user row when needed.
Re-adding the same thumbprint for the same user updates the label and clears
revocation. Revocation prevents new sessions immediately; already issued JWTs
remain valid until their five-minute expiry.
