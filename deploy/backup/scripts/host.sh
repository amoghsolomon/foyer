#!/usr/bin/env bash
# Host-side Foyer backup wrapper. Quiesces writers via Compose, never mounts
# the Docker socket in the backup image, and restarts services before encrypt
# and upload.
set -euo pipefail

SCRIPT_DIR=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
# shellcheck source=lib.sh
. "${SCRIPT_DIR}/lib.sh"

umask 077

LOCK_FILE="${FOYER_BACKUP_LOCK_FILE:-${XDG_RUNTIME_DIR:-/tmp}/foyer-backup.lock}"
COMPOSE_ENV="${FOYER_COMPOSE_ENV_FILE:-${FOYER_DEPLOY_ROOT}/.env}"
if [ ! -f "${COMPOSE_ENV}" ]; then
  COMPOSE_ENV="${FOYER_DEPLOY_ROOT}/.env.example"
fi
COMPOSE_FILE="${FOYER_COMPOSE_FILE:-${FOYER_DEPLOY_ROOT}/compose.yaml}"
COMPOSE=(docker compose --env-file "${COMPOSE_ENV}" -f "${COMPOSE_FILE}")
if [ -n "${FOYER_COMPOSE_PROJECT_NAME:-}" ]; then
  COMPOSE+=(--project-name "${FOYER_COMPOSE_PROJECT_NAME}")
fi
WRITERS_STOPPED=0
RESTART_SERVER=0
RESTART_POWERSYNC=0
RESTART_RADICALE=0
STAGE_DIR=""
HOLDING_LOCK=0

usage() {
  cat <<'EOF'
usage: host.sh <command>

commands:
  create              quiesce, stage, restart writers, encrypt, upload
  list                list objects in the configured bucket/prefix
  download KEY OUT    download one object to an explicit path
  verify KEY|FILE     download if needed, decrypt, and validate
  restore KEY|FILE    restore into explicit empty or isolated targets
  restore-drill       seed, backup, isolated restore, compare
  age-identity        generate a local test age identity
  test-up             start the opt-in MinIO backup-test profile
  test-down           stop backup-test services without removing live volumes
  clean-test          remove only explicit temporary test resources
  ensure-bucket       create the configured test/production bucket
EOF
}

acquire_lock() {
  if [ "${FOYER_BACKUP_SKIP_LOCK:-0}" = "1" ]; then
    return 0
  fi
  mkdir -p "$(dirname -- "${LOCK_FILE}")"
  exec 9>"${LOCK_FILE}"
  if ! flock -n 9; then
    die "another backup or restore is already running"
  fi
  HOLDING_LOCK=1
}

service_running() {
  local id
  id=$("${COMPOSE[@]}" ps --status running -q "$1" 2>/dev/null || true)
  [ -n "${id}" ]
}

restart_writers() {
  if [ "${WRITERS_STOPPED}" -ne 1 ]; then
    return 0
  fi
  log "restarting quiesced services"
  if [ "${RESTART_RADICALE}" -eq 1 ]; then
    "${COMPOSE[@]}" start radicale || log_err "failed to restart radicale"
  fi
  if [ "${RESTART_SERVER}" -eq 1 ]; then
    "${COMPOSE[@]}" start server || log_err "failed to restart server"
  fi
  if [ "${RESTART_POWERSYNC}" -eq 1 ]; then
    "${COMPOSE[@]}" start powersync || log_err "failed to restart powersync"
  fi
  WRITERS_STOPPED=0
}

wipe_host_stage() {
  if [ -n "${STAGE_DIR}" ] && [ -d "${STAGE_DIR}" ]; then
    wipe_bounded_dir "${STAGE_DIR}" keep-root || true
    rmdir -- "${STAGE_DIR}" 2>/dev/null || wipe_bounded_dir "${STAGE_DIR}" remove-root || true
    STAGE_DIR=""
  fi
}

host_cleanup() {
  restart_writers
  wipe_host_stage
}

load_local_backup_env() {
  local file
  if [ -f "${COMPOSE_ENV}" ]; then
    # Dokploy writes the Compose Environment tab to this file. Load it before
    # the optional backup-specific file so the latter can override values.
    load_env_file "${COMPOSE_ENV}"
  fi
  if [ -n "${FOYER_BACKUP_ENV_FILE:-}" ]; then
    load_env_file "${FOYER_BACKUP_ENV_FILE}"
  fi
  file="${FOYER_BACKUP_ROOT}/.local/backup.env"
  if [ -f "${file}" ]; then
    load_env_file "${file}"
  fi
  if [ -f "${FOYER_BACKUP_ROOT}/.local/age-recipient.txt" ] && [ -z "${FOYER_BACKUP_AGE_RECIPIENT:-}" ]; then
    FOYER_BACKUP_AGE_RECIPIENT=$(tr -d '\r\n' < "${FOYER_BACKUP_ROOT}/.local/age-recipient.txt")
    export FOYER_BACKUP_AGE_RECIPIENT
  fi
  if [ -f "${FOYER_BACKUP_ROOT}/.local/age-identity.txt" ] && [ -z "${FOYER_BACKUP_AGE_IDENTITY_FILE:-}" ]; then
    FOYER_BACKUP_AGE_IDENTITY_FILE="${FOYER_BACKUP_ROOT}/.local/age-identity.txt"
    export FOYER_BACKUP_AGE_IDENTITY_FILE
  fi
  if [ "${FOYER_BACKUP_USE_COMPOSE_SECRETS:-0}" != "1" ] \
    && [ -z "${FOYER_BACKUP_SECRETS_DIR:-}" ]; then
    FOYER_BACKUP_SECRETS_DIR="${FOYER_BACKUP_ROOT}/.local/secrets"
    export FOYER_BACKUP_SECRETS_DIR
  fi
  apply_test_s3_defaults
  normalize_s3_endpoint_for_container
  if [ -n "${FOYER_BACKUP_SECRETS_DIR:-}" ]; then
    mkdir -p "${FOYER_BACKUP_SECRETS_DIR}"
    chmod 700 "${FOYER_BACKUP_SECRETS_DIR}" || true
  fi
}

apply_test_s3_defaults() {
  if [ "${FOYER_BACKUP_REQUIRE_EXPLICIT_S3:-0}" = "1" ]; then
    [ -n "${FOYER_BACKUP_S3_ENDPOINT:-}" ] || die "FOYER_BACKUP_S3_ENDPOINT is required"
    [ -n "${FOYER_BACKUP_S3_BUCKET:-}" ] || die "FOYER_BACKUP_S3_BUCKET is required"
    [ -n "${FOYER_BACKUP_S3_ACCESS_KEY:-}" ] || die "FOYER_BACKUP_S3_ACCESS_KEY is required"
    [ -n "${FOYER_BACKUP_S3_SECRET_KEY:-}" ] || die "FOYER_BACKUP_S3_SECRET_KEY is required"
    return 0
  fi
  if [ -z "${FOYER_BACKUP_S3_ENDPOINT:-}" ]; then
    FOYER_BACKUP_S3_ENDPOINT="${FOYER_BACKUP_TEST_S3_ENDPOINT:-http://minio:9000}"
  fi
  if [ -z "${FOYER_BACKUP_S3_REGION:-}" ]; then
    FOYER_BACKUP_S3_REGION=us-east-1
  fi
  if [ -z "${FOYER_BACKUP_S3_BUCKET:-}" ]; then
    FOYER_BACKUP_S3_BUCKET=foyer-backups
  fi
  if [ -z "${FOYER_BACKUP_S3_PREFIX:-}" ]; then
    FOYER_BACKUP_S3_PREFIX=foyer/
  fi
  if [ -z "${FOYER_BACKUP_S3_ACCESS_KEY:-}" ]; then
    FOYER_BACKUP_S3_ACCESS_KEY="${FOYER_BACKUP_TEST_MINIO_USER:-foyer-backup-test}"
  fi
  if [ -z "${FOYER_BACKUP_S3_SECRET_KEY:-}" ]; then
    FOYER_BACKUP_S3_SECRET_KEY="${FOYER_BACKUP_TEST_MINIO_PASSWORD:-foyer-backup-test-minio-password}"
  fi
  export FOYER_BACKUP_S3_ENDPOINT FOYER_BACKUP_S3_REGION FOYER_BACKUP_S3_BUCKET
  export FOYER_BACKUP_S3_PREFIX FOYER_BACKUP_S3_ACCESS_KEY FOYER_BACKUP_S3_SECRET_KEY
}

backup_run() {
  local extra=()
  if [ -n "${STAGE_DIR}" ]; then
    extra+=(-v "${STAGE_DIR}:/staging")
  fi
  if [ -n "${FOYER_BACKUP_SECRETS_DIR:-}" ]; then
    extra+=(-v "${FOYER_BACKUP_SECRETS_DIR}:/secrets:ro")
  fi
  if [ -n "${FOYER_BACKUP_RADICALE_HOST_DIR:-}" ]; then
    extra+=(-v "${FOYER_BACKUP_RADICALE_HOST_DIR}:/var/lib/radicale:ro")
  fi
  if [ -n "${FOYER_RESTORE_RADICALE_VOLUME:-}" ]; then
    extra+=(-v "${FOYER_RESTORE_RADICALE_VOLUME}:/restore/radicale")
  elif [ -n "${FOYER_RESTORE_RADICALE_HOST_DIR:-}" ]; then
    extra+=(-v "${FOYER_RESTORE_RADICALE_HOST_DIR}:/restore/radicale")
  fi
  if [ -n "${FOYER_RESTORE_SECRETS_HOST_DIR:-}" ]; then
    extra+=(-v "${FOYER_RESTORE_SECRETS_HOST_DIR}:/restore/secrets")
  fi
  if [ -n "${FOYER_BACKUP_AGE_IDENTITY_FILE:-}" ]; then
    extra+=(-v "${FOYER_BACKUP_AGE_IDENTITY_FILE}:/restore/age-identity.txt:ro")
  fi
  if [ -n "${FOYER_BACKUP_BUNDLE_HOST_FILE:-}" ]; then
    extra+=(-v "${FOYER_BACKUP_BUNDLE_HOST_FILE}:/restore/bundle/input.tar.zst.age:ro")
  fi
  if [ -n "${FOYER_BACKUP_DOWNLOAD_HOST_FILE:-}" ]; then
    extra+=(-v "$(dirname -- "${FOYER_BACKUP_DOWNLOAD_HOST_FILE}"):/restore/bundle")
  fi
  local profiles=(--profile backup)
  if [ "${FOYER_BACKUP_USE_TEST_PROFILE:-0}" = "1" ] || [ "${FOYER_RESTORE_ISOLATED:-0}" = "1" ]; then
    profiles+=(--profile backup-test)
  fi
  "${COMPOSE[@]}" "${profiles[@]}" run --rm --no-deps \
    "${extra[@]}" \
    -e FOYER_BACKUP_AGE_RECIPIENT \
    -e FOYER_BACKUP_S3_ENDPOINT \
    -e FOYER_BACKUP_S3_REGION \
    -e FOYER_BACKUP_S3_BUCKET \
    -e FOYER_BACKUP_S3_PREFIX \
    -e FOYER_BACKUP_S3_ACCESS_KEY \
    -e FOYER_BACKUP_S3_SECRET_KEY \
    -e FOYER_BACKUP_S3_SESSION_TOKEN \
    -e FOYER_BACKUP_S3_ADDRESSING \
    -e FOYER_BACKUP_POSTGRES_HOST \
    -e FOYER_BACKUP_POSTGRES_PORT \
    -e FOYER_BACKUP_POSTGRES_DB \
    -e FOYER_BACKUP_POSTGRES_USER \
    -e FOYER_BACKUP_POSTGRES_PASSWORD \
    -e POSTGRES_PASSWORD \
    -e FOYER_RESTORE_ISOLATED \
    -e FOYER_RESTORE_POSTGRES_HOST \
    -e FOYER_RESTORE_RADICALE_DIR \
    -e FOYER_RESTORE_SECRETS_DIR \
    -e FOYER_RESTORE_REBUILD_PROJECTIONS \
    -e FOYER_SERVER_VERSION \
    -e FOYER_RADICALE_IMAGE \
    -e "FOYER_BACKUP_AGE_IDENTITY_FILE=${FOYER_BACKUP_AGE_IDENTITY_FILE:+/restore/age-identity.txt}" \
    backup \
    "$@"
}

capture_server_version() {
  local body
  body=$(curl -sf --max-time 2 http://127.0.0.1:3583/health/live 2>/dev/null || true)
  if [ -n "${body}" ]; then
    FOYER_SERVER_VERSION=$(printf '%s' "${body}" | python3 -c 'import json,sys; print(json.load(sys.stdin).get("version","unknown"))' 2>/dev/null || printf 'unknown')
  else
    FOYER_SERVER_VERSION=unknown
  fi
  FOYER_RADICALE_IMAGE=kozea/radicale:3.7.3
  export FOYER_SERVER_VERSION FOYER_RADICALE_IMAGE
}

quiesce_writers() {
  if [ "${FOYER_BACKUP_SKIP_QUIESCE:-0}" = "1" ]; then
    log "skipping live-writer quiesce (isolated format test)"
    return 0
  fi
  service_running postgres || die "PostgreSQL must stay running and readable during staging"
  if service_running server; then
    RESTART_SERVER=1
  fi
  if service_running powersync; then
    RESTART_POWERSYNC=1
  fi
  if service_running radicale; then
    RESTART_RADICALE=1
  fi
  log "quiescing Foyer writers and Radicale"
  if [ "${RESTART_SERVER}" -eq 1 ]; then
    "${COMPOSE[@]}" stop server
  fi
  if [ "${RESTART_POWERSYNC}" -eq 1 ]; then
    "${COMPOSE[@]}" stop powersync
  fi
  if [ "${RESTART_RADICALE}" -eq 1 ]; then
    "${COMPOSE[@]}" stop radicale
  fi
  WRITERS_STOPPED=1
}

make_stage_dir() {
  STAGE_DIR=$(mktemp -d /var/tmp/foyer-backup.XXXXXX)
  chmod 700 "${STAGE_DIR}"
}

cmd_create() {
  acquire_lock
  trap host_cleanup EXIT INT HUP TERM
  load_local_backup_env
  [ -n "${FOYER_BACKUP_AGE_RECIPIENT:-}" ] || die "age recipient is not configured"
  mkdir -p "${FOYER_BACKUP_SECRETS_DIR}"
  chmod 700 "${FOYER_BACKUP_SECRETS_DIR}"
  capture_server_version
  make_stage_dir
  local stage_status=0
  quiesce_writers
  backup_run stage || stage_status=$?
  restart_writers
  if [ "${stage_status}" -ne 0 ]; then
    die "staging failed"
  fi
  backup_run seal
  wipe_host_stage
  log "backup create finished"
}

cmd_list() {
  load_local_backup_env
  FOYER_BACKUP_USE_TEST_PROFILE=1
  export FOYER_BACKUP_USE_TEST_PROFILE
  backup_run list
}

cmd_ensure_bucket() {
  load_local_backup_env
  FOYER_BACKUP_USE_TEST_PROFILE=1
  export FOYER_BACKUP_USE_TEST_PROFILE
  backup_run ensure-bucket
}

cmd_download() {
  local key out
  [ "${#}" -eq 2 ] || die "usage: host.sh download KEY OUT"
  acquire_lock
  trap host_cleanup EXIT INT HUP TERM
  load_local_backup_env
  key=$1
  out=$(assert_safe_target "$2" "download output" "${FOYER_RESTORE_ISOLATED:-0}")
  if [ -e "${out}" ]; then
    die "download output already exists"
  fi
  mkdir -p "$(dirname -- "${out}")"
  chmod 700 "$(dirname -- "${out}")"
  FOYER_BACKUP_DOWNLOAD_HOST_FILE="${out}"
  export FOYER_BACKUP_DOWNLOAD_HOST_FILE
  backup_run download "${key}" "/restore/bundle/$(basename -- "${out}")"
}

cmd_verify() {
  local source
  [ "${#}" -eq 1 ] || die "usage: host.sh verify KEY|FILE"
  acquire_lock
  trap host_cleanup EXIT INT HUP TERM
  load_local_backup_env
  source=$1
  make_stage_dir
  if [ -f "${source}" ]; then
    source=$(assert_safe_target "${source}" "bundle file" "${FOYER_RESTORE_ISOLATED:-0}")
    FOYER_BACKUP_BUNDLE_HOST_FILE="${source}"
    export FOYER_BACKUP_BUNDLE_HOST_FILE
    backup_run verify /restore/bundle/input.tar.zst.age
  else
    backup_run verify "${source}"
  fi
}

cmd_restore() {
  local source
  [ "${#}" -eq 1 ] || die "usage: host.sh restore KEY|FILE"
  acquire_lock
  trap host_cleanup EXIT INT HUP TERM
  load_local_backup_env
  source=$1
  [ -n "${FOYER_RESTORE_POSTGRES_HOST:-}" ] || die "FOYER_RESTORE_POSTGRES_HOST must be explicit"
  [ -n "${FOYER_RESTORE_RADICALE_HOST_DIR:-}${FOYER_RESTORE_RADICALE_DIR:-}" ] || \
    die "FOYER_RESTORE_RADICALE_DIR must be explicit"
  [ -n "${FOYER_RESTORE_SECRETS_HOST_DIR:-}${FOYER_RESTORE_SECRETS_DIR:-}" ] || \
    die "FOYER_RESTORE_SECRETS_DIR must be explicit"
  if [ -n "${FOYER_RESTORE_RADICALE_HOST_DIR:-}" ]; then
    FOYER_RESTORE_RADICALE_DIR=/restore/radicale
    export FOYER_RESTORE_RADICALE_DIR
  fi
  if [ -n "${FOYER_RESTORE_SECRETS_HOST_DIR:-}" ]; then
    FOYER_RESTORE_SECRETS_DIR=/restore/secrets
    export FOYER_RESTORE_SECRETS_DIR
  fi
  make_stage_dir
  if [ "${FOYER_RESTORE_ISOLATED:-0}" = "1" ]; then
    "${COMPOSE[@]}" --profile backup-test up -d restore-test-postgres
    local ready=0
    local i
    for i in $(seq 1 30); do
      if "${COMPOSE[@]}" --profile backup-test exec -T restore-test-postgres \
        pg_isready -U "${POSTGRES_USER:-foyer}" -d "${POSTGRES_DATABASE:-foyer}" >/dev/null 2>&1; then
        ready=1
        break
      fi
      sleep 2
    done
    [ "${ready}" -eq 1 ] || die "restore-test PostgreSQL did not become ready"
    "${COMPOSE[@]}" --profile backup-test stop restore-test-server restore-test-radicale || true
    if [ -z "${FOYER_RESTORE_RADICALE_VOLUME:-}" ]; then
      FOYER_RESTORE_RADICALE_VOLUME="${COMPOSE_PROJECT_NAME:-foyer}_restore_test_radicale_data"
      export FOYER_RESTORE_RADICALE_VOLUME
    fi
    "${COMPOSE[@]}" --profile backup-test up --no-start restore-test-radicale >/dev/null
    FOYER_RESTORE_RADICALE_DIR=/restore/radicale
    export FOYER_RESTORE_RADICALE_DIR
  fi
  if [ -f "${source}" ]; then
    source=$(assert_safe_target "${source}" "bundle file" "${FOYER_RESTORE_ISOLATED:-0}")
    FOYER_BACKUP_BUNDLE_HOST_FILE="${source}"
    export FOYER_BACKUP_BUNDLE_HOST_FILE
    backup_run restore /restore/bundle/input.tar.zst.age
  else
    backup_run restore "${source}"
  fi
}

cmd_age_identity() {
  local dest="${FOYER_BACKUP_ROOT}/.local"
  mkdir -p "${dest}"
  chmod 700 "${dest}"
  if [ -f "${dest}/age-identity.txt" ]; then
    log "local test age identity already exists"
    return 0
  fi
  if command -v age-keygen >/dev/null 2>&1; then
    age-keygen -o "${dest}/age-identity.txt"
    age-keygen -y "${dest}/age-identity.txt" > "${dest}/age-recipient.txt"
  else
    "${COMPOSE[@]}" --profile backup build backup
    "${COMPOSE[@]}" --profile backup run --rm --no-deps \
      --user "$(id -u):$(id -g)" \
      -v "${dest}:/var/tmp/foyer-backup-test.keys" \
      backup age-keygen /var/tmp/foyer-backup-test.keys
  fi
  chmod 600 "${dest}/age-identity.txt" "${dest}/age-recipient.txt" || true
  log "wrote ${dest}/age-identity.txt and age-recipient.txt"
}

cmd_test_up() {
  load_local_backup_env
  "${COMPOSE[@]}" --profile backup --profile backup-test build backup
  "${COMPOSE[@]}" --profile backup-test up -d minio minio-init
  local i ready=0
  for i in $(seq 1 30); do
    if curl -sf http://127.0.0.1:19000/minio/health/live >/dev/null; then
      ready=1
      break
    fi
    sleep 1
  done
  [ "${ready}" -eq 1 ] || die "MinIO did not become ready on 127.0.0.1:19000"
  FOYER_BACKUP_S3_ENDPOINT=http://minio:9000
  FOYER_BACKUP_USE_TEST_PROFILE=1
  export FOYER_BACKUP_S3_ENDPOINT FOYER_BACKUP_USE_TEST_PROFILE
  backup_run ensure-bucket
  log "MinIO backup-test profile is ready on 127.0.0.1:19000"
}

cmd_test_down() {
  "${COMPOSE[@]}" --profile backup-test stop \
    minio minio-init restore-test-postgres restore-test-radicale restore-test-server \
    >/dev/null 2>&1 || true
  "${COMPOSE[@]}" --profile backup-test rm -f \
    minio minio-init restore-test-postgres restore-test-radicale restore-test-server \
    >/dev/null 2>&1 || true
}

cmd_clean_test() {
  acquire_lock
  trap host_cleanup EXIT INT HUP TERM
  cmd_test_down
  local project="${COMPOSE_PROJECT_NAME:-foyer}"
  docker volume rm -f \
    "${project}_backup_test_minio_data" \
    "${project}_restore_test_postgres_data" \
    "${project}_restore_test_radicale_data" \
    >/dev/null 2>&1 || true
  if [ -d /var/tmp ]; then
    find /var/tmp -maxdepth 1 \( \
      -name 'foyer-backup.*' -o \
      -name 'foyer-restore-test.*' -o \
      -name 'foyer-backup-test.*' \
    \) -type d -exec rm -rf -- {} +
  fi
  if [ -d "${FOYER_BACKUP_ROOT}/.local/staging" ]; then
    wipe_bounded_dir "${FOYER_BACKUP_ROOT}/.local/staging" remove-root || true
  fi
  log "removed explicit backup-test volumes and temporary directories"
}

latest_object() {
  backup_run list | awk -F'\t' 'NF {print $1}' | sort | tail -n 1
}

cmd_restore_drill() {
  acquire_lock
  trap host_cleanup EXIT INT HUP TERM
  load_local_backup_env
  cmd_age_identity
  load_local_backup_env
  FOYER_BACKUP_SKIP_LOCK=1
  export FOYER_BACKUP_SKIP_LOCK
  "${SCRIPT_DIR}/../tests/drill.sh"
}

main() {
  if [ "${#}" -lt 1 ]; then
    usage
    exit 2
  fi
  local command=$1
  shift
  case "${command}" in
    create) cmd_create "$@" ;;
    list) cmd_list "$@" ;;
    download) cmd_download "$@" ;;
    verify) cmd_verify "$@" ;;
    restore) cmd_restore "$@" ;;
    restore-drill) cmd_restore_drill "$@" ;;
    age-identity) cmd_age_identity "$@" ;;
    test-up) cmd_test_up "$@" ;;
    test-down) cmd_test_down "$@" ;;
    clean-test) cmd_clean_test "$@" ;;
    ensure-bucket) cmd_ensure_bucket "$@" ;;
    -h|--help|help) usage ;;
    *)
      usage
      die "unknown command"
      ;;
  esac
}

main "$@"
