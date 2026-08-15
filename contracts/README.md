# Contracts

`openapi/foyer-v1.yaml` is the authoritative contract for the self-hosted Rust server. It covers
notes, bookmarks, tasks, contacts, calendar, and authentication as normalized values only. Clients
never receive DAV credentials, raw iCalendar/vCard payloads, or operation/checkpoint tables.

`auth/v1/` is the device-key authentication contract: JSON field names, thumbprint construction,
challenge payload domain separation, signature encoding, JWT claims, and cross-language fixtures.

`legacy-cloudflare-http-api.md` documents the deleted Cloudflare/Flue service for migration analysis.
It is not implemented by the Rust server and is not an authoritative target for new code.

Contracts are language-neutral. Android, Foyer Shell, and the service remain independent clients of
the wire protocol and must not share database models or server-internal domain types.
