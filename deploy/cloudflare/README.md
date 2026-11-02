# Foyer's exact public API route

Live URL: https://foyer.amoghs.net — backend JSON/API, not a chat page.

`foyer-route.yaml` is one published-application rule fragment for existing Tunnel
`amoghs-net-hetzner`, not a complete Tunnel configuration. Keep the existing apex
rule and terminal HTTP404 rule. Only the exact Foyer hostname and anchored paths
reach `http://server.foyer.svc.cluster.local:3583`. Unknown paths/hostnames retain404.
Database, DAV, PowerSync, readiness, enrollment/admin and personal-data CRUD paths
are not exposed. The first public rule also created an explicit proxied CNAME for
Foyer pointing to the same Tunnel as the existing wildcard. No new token is used.

`foyer-https-redirect.json` records the one exact-host HTTP308 redirect configured
in the amoghs.net zone. It uses Cloudflare's `ssl` Boolean, preserves path/query and
request method, and changes no global SSL setting or other hostname. Dashboard
validation warns when only wildcard DNS is present; verify that wildcard first.

Changes to these fragments do not automatically call Cloudflare. Apply only the
reviewed Foyer rule, preserve concurrent shared Tunnel/website configuration, and
then run HTTPS/auth/excluded-path checks. Rollback removes only the Foyer rule and
its exact-host redirect/CNAME as appropriate; never delete the shared Tunnel or
its credentials. See ADR0029 and the dated exposure audit for findings and limits.

The API requires manually enrolled device signatures and short-lived bearer JWTs.
There is no public enrollment/first-visitor ownership flow. Provider configuration,
owner-device enrollment, agent tools/memory and client migration remain deferred.
