# Radicale 3.7.3

Pinned CalDAV/CardDAV authority for Foyer calendar, tasks, and contacts.

- Image: official `kozea/radicale:3.7.3`
- Storage: persistent volume at `/var/lib/radicale`
- Auth: bcrypt htpasswd; Foyer Server uses the `foyer` service user
- First-party Android and Foyer Shell clients never receive this credential
- Foyer Server must use HTTP CalDAV/CardDAV on the Compose network and must not mount Radicale storage

Development compose publishes `127.0.0.1:5232` and mounts `users.dev`, a clearly insecure localhost-only password file. Production must:

1. Set `RADICALE_HTPASSWD_FILE` to a host-only bcrypt htpasswd (never `users.dev`).
2. Drop the published `5232` port so only Caddy exposes DAV.
3. Keep PostgreSQL projection tables rebuildable; restoring Radicale restores authority.
