#!/usr/bin/env bash
# Isolated MinIO roundtrip that does not use live postgres_data/radicale_data
# as restore targets. Used when the development stack is unhealthy, and as a
# format-level check of dump -> encrypt -> upload -> verify -> pg_restore.
set -euo pipefail

SCRIPT_DIR=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
# shellcheck source=../scripts/lib.sh
. "${SCRIPT_DIR}/../scripts/lib.sh"
HOST="${SCRIPT_DIR}/../scripts/host.sh"

COMPOSE_ENV=$(compose_env_file)
COMPOSE=(docker compose --env-file "${COMPOSE_ENV}" -f "${FOYER_DEPLOY_ROOT}/compose.yaml")
WORKDIR=$(mktemp -d /var/tmp/foyer-backup-test.XXXXXX)
chmod 700 "${WORKDIR}"
trap 'wipe_bounded_dir "${WORKDIR}" remove-root || true' EXIT

mkdir -p "${WORKDIR}/radicale/collections/collection-root/roundtrip" \
  "${WORKDIR}/secrets" "${WORKDIR}/restore-secrets"
printf 'BEGIN:VCALENDAR\nVERSION:2.0\nBEGIN:VEVENT\nUID:roundtrip@example.test\nSUMMARY:Roundtrip\nEND:VEVENT\nEND:VCALENDAR\n' \
  > "${WORKDIR}/radicale/collections/collection-root/roundtrip/event.ics"
printf 'roundtrip-signing-key\n' > "${WORKDIR}/secrets/token-signing.key"
chmod 600 "${WORKDIR}/secrets/token-signing.key"

"${HOST}" test-up
if ! "${COMPOSE[@]}" --profile backup-test exec -T restore-test-postgres \
  pg_isready -U "${POSTGRES_USER:-foyer}" -d "${POSTGRES_DATABASE:-foyer}" >/dev/null 2>&1; then
  "${COMPOSE[@]}" --profile backup-test up -d --force-recreate restore-test-postgres || \
    "${COMPOSE[@]}" --profile backup-test up -d restore-test-postgres
fi
ready=0
for _ in $(seq 1 30); do
  if "${COMPOSE[@]}" --profile backup-test exec -T restore-test-postgres \
    pg_isready -U "${POSTGRES_USER:-foyer}" -d "${POSTGRES_DATABASE:-foyer}" >/dev/null 2>&1; then
    ready=1
    break
  fi
  sleep 2
done
[ "${ready}" -eq 1 ] || die "restore-test PostgreSQL did not become ready"

"${COMPOSE[@]}" --profile backup-test stop restore-test-server || true
"${COMPOSE[@]}" --profile backup-test exec -T restore-test-postgres \
  psql -v ON_ERROR_STOP=1 -U "${POSTGRES_USER:-foyer}" -d "${POSTGRES_DATABASE:-foyer}" <<'SQL'
CREATE TABLE IF NOT EXISTS backup_roundtrip_probe (
  id TEXT PRIMARY KEY,
  body TEXT NOT NULL
);
INSERT INTO backup_roundtrip_probe (id, body)
VALUES ('roundtrip', 'canonical probe row')
ON CONFLICT (id) DO UPDATE SET body = EXCLUDED.body;
SQL

export FOYER_BACKUP_SECRETS_DIR="${WORKDIR}/secrets"
export FOYER_BACKUP_RADICALE_HOST_DIR="${WORKDIR}/radicale"
export FOYER_BACKUP_POSTGRES_HOST=restore-test-postgres
export FOYER_BACKUP_USE_TEST_PROFILE=1
export FOYER_BACKUP_SKIP_LOCK="${FOYER_BACKUP_SKIP_LOCK:-1}"
export FOYER_BACKUP_SKIP_QUIESCE=1
"${HOST}" create

OBJECT=$("${HOST}" list | awk -F'\t' 'NF {print $1}' | sort | tail -n 1)
[ -n "${OBJECT}" ] || die "no backup object was uploaded"
"${HOST}" verify "${OBJECT}"

export FOYER_RESTORE_ISOLATED=1
export FOYER_RESTORE_POSTGRES_HOST=restore-test-postgres
export FOYER_RESTORE_RADICALE_DIR=/restore/radicale
export FOYER_RESTORE_SECRETS_DIR=/restore/secrets
export FOYER_RESTORE_SECRETS_HOST_DIR="${WORKDIR}/restore-secrets"
export FOYER_RESTORE_RADICALE_VOLUME="${COMPOSE_PROJECT_NAME:-foyer}_restore_test_radicale_data"
"${COMPOSE[@]}" --profile backup-test stop restore-test-radicale restore-test-server || true
"${HOST}" restore "${OBJECT}"

body=$("${COMPOSE[@]}" --profile backup-test exec -T restore-test-postgres \
  psql -X -Atqc "SELECT body FROM backup_roundtrip_probe WHERE id='roundtrip'" \
  -U "${POSTGRES_USER:-foyer}" -d "${POSTGRES_DATABASE:-foyer}")
[ "${body}" = "canonical probe row" ] || die "restored probe row did not match"
[ -f "${WORKDIR}/restore-secrets/token-signing.key" ] || die "signing secret was not restored"
cmp -s "${WORKDIR}/secrets/token-signing.key" "${WORKDIR}/restore-secrets/token-signing.key" \
  || die "restored signing secret did not match"
log "isolated MinIO roundtrip passed object=${OBJECT}"
