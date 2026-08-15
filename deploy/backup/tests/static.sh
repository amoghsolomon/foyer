#!/usr/bin/env bash
# Static validation of the backup/restore slice.
set -euo pipefail

SCRIPT_DIR=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
# shellcheck source=../scripts/lib.sh
. "${SCRIPT_DIR}/../scripts/lib.sh"

ROOT="${FOYER_BACKUP_ROOT}"
status=0

fail() {
  log_err "FAIL: $*"
  status=1
}

pass() {
  log "ok: $*"
}

if command -v shellcheck >/dev/null 2>&1; then
  shellcheck -x \
    "${ROOT}/scripts/lib.sh" \
    "${ROOT}/scripts/foyer-backup" \
    "${ROOT}/scripts/host.sh" \
    "${ROOT}/tests/static.sh" \
    "${ROOT}/tests/unit.sh" \
    "${ROOT}/tests/seed.sh" \
    "${ROOT}/tests/drill.sh" \
    "${ROOT}/tests/roundtrip.sh" \
    || fail "shellcheck"
  pass "shellcheck"
else
  log "shellcheck not installed; running grep-based static checks only"
fi

SCRIPTS="${ROOT}/scripts"
if grep -R -n 'docker.sock' "${SCRIPTS}" "${ROOT}/Dockerfile"; then
  fail "backup scripts must not reference the Docker socket"
else
  pass "no Docker socket references"
fi

if grep -R -n -E 'restic|pg_basebackup|wal-g' "${SCRIPTS}"; then
  fail "initial format excludes restic and live PostgreSQL byte copies"
else
  pass "no restic or live PostgreSQL byte-copy tools"
fi

if grep -R -n -E 'dokploy|cloudflare|aws s3api|mc alias' "${SCRIPTS}"; then
  fail "backup scripts must stay on the generic S3 API"
else
  pass "no provider-specific object-store APIs"
fi

if grep -R -n -E 'rm -rf /\*|rm -rf /tmp/\*|docker volume prune|docker system prune' "${SCRIPTS}"; then
  fail "dangerous broad delete found"
else
  pass "no dangerous broad deletes"
fi

if grep -n 'FOYER_BACKUP_AGE_IDENTITY=' "${ROOT}/backup.env.example"; then
  fail "backup.env.example must not contain a recovery identity"
else
  pass "example config has only the public recipient slot"
fi

if grep -Eq '^[[:space:]]*FOYER_BACKUP_AGE_IDENTITY_FILE=' "${ROOT}/backup.env.example"; then
  fail "backup.env.example must keep the recovery identity file commented"
else
  pass "recovery identity file is commented in the example"
fi

if grep -q 'minio-init' "${FOYER_DEPLOY_ROOT}/compose.yaml" \
  && grep -q 'profiles: \["backup-test"\]' "${FOYER_DEPLOY_ROOT}/compose.yaml"; then
  pass "compose defines backup-test MinIO init"
else
  fail "compose is missing the backup-test MinIO init service"
fi

if grep -q '/var/run/docker.sock' "${FOYER_DEPLOY_ROOT}/compose.yaml"; then
  fail "compose must not mount the Docker socket"
else
  pass "compose does not mount the Docker socket"
fi

for section in "Manual backup" "List, download, and verify" "Isolated restore drill" \
  "Production scheduling" "Retention" "Least-privilege object credentials" \
  "Age key custody" "Disaster recovery"; do
  if grep -q "${section}" "${ROOT}/README.md"; then
    pass "README covers ${section}"
  else
    fail "README is missing ${section}"
  fi
done

if ! grep -q 'FOYER_BACKUP_AGE_RECIPIENT' "${ROOT}/backup.env.example"; then
  fail "example config is missing the age recipient"
else
  pass "example config documents the public recipient"
fi

if [ ! -f "${ROOT}/systemd/foyer-backup.service" ] || [ ! -f "${ROOT}/systemd/foyer-backup.timer" ]; then
  fail "systemd examples are missing"
else
  pass "systemd examples present"
fi

python3 -m py_compile "${ROOT}/scripts/s3.py" "${ROOT}/tests/compare.py" \
  || fail "python syntax"
pass "python syntax"

if command -v docker >/dev/null 2>&1; then
  docker compose --env-file "${FOYER_DEPLOY_ROOT}/.env.example" \
    -f "${FOYER_DEPLOY_ROOT}/compose.yaml" config --quiet \
    || fail "default compose config"
  docker compose --env-file "${FOYER_DEPLOY_ROOT}/.env.example" \
    -f "${FOYER_DEPLOY_ROOT}/compose.yaml" \
    --profile backup --profile backup-test config --quiet \
    || fail "backup-test compose config"
  pass "compose config for default and backup-test profiles"
else
  fail "docker is required to validate compose config"
fi

if [ "${status}" -ne 0 ]; then
  die "static validation failed"
fi
log "static validation passed"
