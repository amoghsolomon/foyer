#!/usr/bin/env bash
# Local restore drill: seed, encrypted backup, isolated restore, compare.
set -euo pipefail

SCRIPT_DIR=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
# shellcheck source=../scripts/lib.sh
. "${SCRIPT_DIR}/../scripts/lib.sh"
HOST="${SCRIPT_DIR}/../scripts/host.sh"

COMPOSE_ENV="${FOYER_COMPOSE_ENV_FILE:-${FOYER_DEPLOY_ROOT}/.env}"
if [ ! -f "${COMPOSE_ENV}" ]; then
  COMPOSE_ENV="${FOYER_DEPLOY_ROOT}/.env.example"
fi
COMPOSE=(docker compose --env-file "${COMPOSE_ENV}" -f "${FOYER_DEPLOY_ROOT}/compose.yaml")

export FOYER_BACKUP_SECRETS_DIR="${FOYER_BACKUP_SECRETS_DIR:-${FOYER_BACKUP_ROOT}/.local/secrets}"
export FOYER_DRILL_REPORT="${FOYER_BACKUP_ROOT}/.local/drill-report.txt"
export FOYER_DRILL_COMPARE="${FOYER_BACKUP_ROOT}/.local/drill-compare.json"
export FOYER_DRILL_API="${FOYER_DRILL_API:-http://127.0.0.1:3583}"
export FOYER_DRILL_RESTORE_API="${FOYER_DRILL_RESTORE_API:-http://127.0.0.1:13583}"
if [ -z "${FOYER_DEV_TOKEN:-}" ]; then
  FOYER_DEV_TOKEN=$(compose_env_value FOYER_DEV_TOKEN)
  export FOYER_DEV_TOKEN
fi

wait_http() {
  local url=$1
  local i
  for i in $(seq 1 40); do
    if curl -sf --max-time 2 "${url}" >/dev/null; then
      return 0
    fi
    sleep 3
  done
  return 1
}

log "starting isolated restore drill"

if ! curl -sf --max-time 2 "${FOYER_DRILL_API}/health/ready" >/dev/null; then
  log "source stack is not healthy; starting stack-dev services"
  "${COMPOSE[@]}" up -d --build postgres radicale server powersync
  wait_http "${FOYER_DRILL_API}/health/ready" || die "source Foyer Server did not become ready"
fi

"${HOST}" test-up

mkdir -p "${FOYER_BACKUP_SECRETS_DIR}"
chmod 700 "${FOYER_BACKUP_SECRETS_DIR}"
"${SCRIPT_DIR}/seed.sh"

log "creating encrypted backup"
FOYER_BACKUP_SKIP_LOCK="${FOYER_BACKUP_SKIP_LOCK:-1}"
export FOYER_BACKUP_SKIP_LOCK
"${HOST}" create

RESTORE_SECRETS=$(mktemp -d /var/tmp/foyer-restore-test.XXXXXX)
chmod 700 "${RESTORE_SECRETS}"
mkdir -p "${RESTORE_SECRETS}/secrets"
trap 'wipe_bounded_dir "${RESTORE_SECRETS}" remove-root || true' EXIT
export FOYER_RESTORE_ISOLATED=1
export FOYER_RESTORE_POSTGRES_HOST=restore-test-postgres
export FOYER_RESTORE_RADICALE_DIR=/restore/radicale
export FOYER_RESTORE_SECRETS_DIR=/restore/secrets
export FOYER_RESTORE_SECRETS_HOST_DIR="${RESTORE_SECRETS}/secrets"
export FOYER_RESTORE_RADICALE_VOLUME="${COMPOSE_PROJECT_NAME:-foyer}_restore_test_radicale_data"
# Restore the dump as-is so API-visible projections match immediately.
# The restore-test server still runs migrations. A projector pass may rebuild
# DAV projections from restored Radicale; that is reported, not required,
# because the projector lives in foyer-server and may still be changing.
export FOYER_RESTORE_REBUILD_PROJECTIONS="${FOYER_RESTORE_REBUILD_PROJECTIONS:-0}"

log "resetting isolated restore-test volumes"
"${COMPOSE[@]}" --profile backup-test stop restore-test-server restore-test-radicale restore-test-postgres || true
"${COMPOSE[@]}" --profile backup-test rm -f restore-test-server restore-test-radicale restore-test-postgres || true
docker volume rm -f \
  "${COMPOSE_PROJECT_NAME:-foyer}_restore_test_postgres_data" \
  "${COMPOSE_PROJECT_NAME:-foyer}_restore_test_radicale_data" \
  >/dev/null 2>&1 || true

OBJECT=$("${HOST}" list | awk -F'\t' 'NF {print $1}' | sort | tail -n 1)
[ -n "${OBJECT}" ] || die "no backup object was uploaded"
log "restoring ${OBJECT} into isolated targets"
"${HOST}" restore "${OBJECT}"

log "starting isolated restore-test server and Radicale"
"${COMPOSE[@]}" --profile backup-test up -d restore-test-radicale restore-test-server
wait_http "${FOYER_DRILL_RESTORE_API}/health/ready" || die "restore-test server did not become ready"

log "waiting for restore-test API and optional projector pass"
if ! wait_http "${FOYER_DRILL_RESTORE_API}/health/ready"; then
  die "restore-test server lost readiness"
fi
# Give the optional projector one pass if it is running.
sleep 8

list_radicale_names() {
  local service=$1
  shift
  "${COMPOSE[@]}" "$@" exec -T "${service}" \
    find /var/lib/radicale -type f \( -name '*.ics' -o -name '*.vcf' \) \
    -printf '%f\n' 2>/dev/null | sort || true
}

FOYER_DRILL_SOURCE_RADICALE=$(list_radicale_names radicale)
FOYER_DRILL_RESTORE_RADICALE=$(list_radicale_names restore-test-radicale --profile backup-test)
export FOYER_DRILL_SOURCE_RADICALE FOYER_DRILL_RESTORE_RADICALE
FOYER_DRILL_COMPARE_OUT="${FOYER_DRILL_COMPARE}"
FOYER_DRILL_REPORT="${FOYER_DRILL_COMPARE_OUT}"
export FOYER_DRILL_REPORT
python3 "${SCRIPT_DIR}/compare.py"
status=$?

log "drill artifacts: ${FOYER_BACKUP_ROOT}/.local/drill-report.txt ${FOYER_DRILL_COMPARE_OUT}"
if [ "${status}" -eq 0 ]; then
  log "restore drill passed"
else
  die "restore drill found mismatched canonical/API-visible state"
fi
