#!/bin/sh
# Print a fresh Dokploy environment block. Redirect it only into a private file
# or paste it directly into Dokploy; the output contains production secrets.
set -eu

if ! command -v openssl >/dev/null 2>&1; then
  echo "openssl is required" >&2
  exit 1
fi

public_zone=${FOYER_PUBLIC_ZONE:-foyer.i0t.in}
image_registry=${FOYER_IMAGE_REGISTRY:-ghcr.io/amoghsolomon/foyer}

random_hex() {
  openssl rand -hex 32
}

cat <<EOF
# Generated Foyer production configuration. Keep this private.
FOYER_IMAGE_REGISTRY=${image_registry}
FOYER_IMAGE_VERSION=main
FOYER_EDGE_NETWORK=dokploy-network

POSTGRES_DATABASE=foyer
POSTGRES_USER=foyer
POSTGRES_PASSWORD=$(random_hex)
POWERSYNC_REPLICATION_PASSWORD=$(random_hex)
POWERSYNC_STORAGE_PASSWORD=$(random_hex)

FOYER_DAV_PASSWORD=$(random_hex)
FOYER_AUTH_KEY_ID=foyer-prod-$(openssl rand -hex 8)
FOYER_AUTH_ISSUER=https://api.${public_zone}
FOYER_AUTH_API_AUDIENCE=foyer-api
FOYER_POWERSYNC_URL=https://sync.${public_zone}
FOYER_POWERSYNC_AUDIENCE=foyer-powersync
RUST_LOG=info

# Portable backups use the Compose-managed secret volume and must never fall
# back to the local MinIO test credential in production.
FOYER_BACKUP_USE_COMPOSE_SECRETS=1
FOYER_BACKUP_REQUIRE_EXPLICIT_S3=1
EOF
