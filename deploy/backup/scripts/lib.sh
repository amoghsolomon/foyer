# Shared helpers for Foyer backup/restore. Sourced by Bash scripts.
# shellcheck shell=bash

set -euo pipefail

FOYER_BACKUP_FORMAT='foyer-canonical-backup'
FOYER_BACKUP_FORMAT_VERSION='1'
FOYER_BACKUP_BUNDLE_PREFIX='foyer-canonical'

_foyer_lib_dir=$(CDPATH='' cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
FOYER_BACKUP_ROOT=$(CDPATH='' cd -- "${_foyer_lib_dir}/.." && pwd)
FOYER_DEPLOY_ROOT=$(CDPATH='' cd -- "${FOYER_BACKUP_ROOT}/.." && pwd)
if [ -z "${FOYER_REPO_ROOT:-}" ]; then
  FOYER_REPO_ROOT=$(CDPATH='' cd -- "${FOYER_DEPLOY_ROOT}/.." && pwd)
fi

FOYER_BACKUP_LOG_REDACT='PASSWORD|SECRET|TOKEN|IDENTITY|AGE-SECRET|AWS_|AUTHORIZATION|PGPASSWORD|FOYER_BACKUP_S3_SECRET|FOYER_BACKUP_S3_ACCESS|FOYER_BACKUP_AGE_IDENTITY|FOYER_DAV_PASSWORD|FOYER_DEV_TOKEN'

log() {
  printf '%s %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$*"
}

log_err() {
  printf '%s %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$*" >&2
}

die() {
  log_err "error: $*"
  exit 1
}

require_cmd() {
  local cmd
  for cmd in "$@"; do
    command -v "${cmd}" >/dev/null 2>&1 || die "required command not found: ${cmd}"
  done
}

utc_stamp() {
  date -u +%Y%m%dT%H%M%SZ
}

is_redacted_name() {
  printf '%s' "$1" | grep -Eq "${FOYER_BACKUP_LOG_REDACT}"
}

load_env_file() {
  local file="$1"
  local line key value
  [ -f "${file}" ] || die "env file not found: ${file}"
  [ -r "${file}" ] || die "env file is not readable"
  while IFS= read -r line || [ -n "${line}" ]; do
    case "${line}" in
      ''|\#*) continue ;;
    esac
    key=${line%%=*}
    value=${line#*=}
    case "${key}" in
      FOYER_*|POSTGRES_*|POWERSYNC_*|RADICALE_*|COMPOSE_*|RUST_LOG) ;;
      *) die "refusing to load unexpected key from env file" ;;
    esac
    case "${value}" in
      \"*\") value=${value#\"}; value=${value%\"} ;;
      \'*\') value=${value#\'}; value=${value%\'} ;;
    esac
    printf -v "${key}" '%s' "${value}"
    export "${key}"
  done < "${file}"
}

read_secret_file() {
  local file="$1"
  [ -f "${file}" ] || die "secret file not found"
  [ -r "${file}" ] || die "secret file is not readable"
  tr -d '\r\n' < "${file}"
}

postgres_password() {
  if [ -n "${FOYER_BACKUP_POSTGRES_PASSWORD_FILE:-}" ]; then
    read_secret_file "${FOYER_BACKUP_POSTGRES_PASSWORD_FILE}"
    return 0
  fi
  if [ -n "${FOYER_BACKUP_POSTGRES_PASSWORD:-}" ]; then
    printf '%s' "${FOYER_BACKUP_POSTGRES_PASSWORD}"
    return 0
  fi
  if [ -n "${POSTGRES_PASSWORD:-}" ]; then
    printf '%s' "${POSTGRES_PASSWORD}"
    return 0
  fi
  die "PostgreSQL password is not configured"
}

export_postgres_env() {
  PGHOST="${FOYER_BACKUP_POSTGRES_HOST:-postgres}"
  PGPORT="${FOYER_BACKUP_POSTGRES_PORT:-5432}"
  PGDATABASE="${FOYER_BACKUP_POSTGRES_DB:-${POSTGRES_DATABASE:-foyer}}"
  PGUSER="${FOYER_BACKUP_POSTGRES_USER:-${POSTGRES_USER:-foyer}}"
  PGPASSWORD="$(postgres_password)"
  export PGHOST PGPORT PGDATABASE PGUSER PGPASSWORD
}

clear_postgres_env() {
  unset PGPASSWORD || true
}

path_has_glob() {
  case "$1" in
    *'*'*|*'?'*|*'['*) return 0 ;;
    *) return 1 ;;
  esac
}

is_broad_path() {
  case "$1" in
    /|/tmp|/var|/var/tmp|/var/lib|/var/lib/docker|/home|/root|/etc|/usr|/opt|/opt/foyer|/data)
      return 0
      ;;
    /tmp/|/var/|/home/|/root/|/etc/|/usr/|/opt/)
      return 0
      ;;
  esac
  return 1
}

is_allowed_container_path() {
  case "$1" in
    /staging|/staging/*)
      return 0
      ;;
    /secrets|/secrets/*)
      return 0
      ;;
    /var/lib/radicale|/var/lib/radicale/*)
      return 0
      ;;
    /restore/radicale|/restore/radicale/*)
      return 0
      ;;
    /restore/secrets|/restore/secrets/*)
      return 0
      ;;
    /restore/verify|/restore/verify/*)
      return 0
      ;;
    /restore/bundle|/restore/bundle/*)
      return 0
      ;;
    /restore/age-identity.txt)
      return 0
      ;;
  esac
  return 1
}

is_allowed_host_path() {
  local path="$1"
  local root
  case "${path}" in
    "${FOYER_BACKUP_ROOT}/.local"|"${FOYER_BACKUP_ROOT}/.local"/*)
      return 0
      ;;
    /var/tmp/foyer-backup.*|/var/tmp/foyer-restore-test.*|/var/tmp/foyer-backup-test.*)
      return 0
      ;;
    /var/lib/foyer/*)
      return 0
      ;;
  esac
  if [ -n "${FOYER_BACKUP_ALLOWED_ROOTS:-}" ]; then
    old_ifs=${IFS}
    IFS=':'
    for root in ${FOYER_BACKUP_ALLOWED_ROOTS}; do
      IFS=${old_ifs}
      [ -n "${root}" ] || continue
      case "${path}" in
        "${root}"|"${root}"/*) return 0 ;;
      esac
    done
    IFS=${old_ifs}
  fi
  return 1
}

clean_abs_path() {
  local path="$1"
  [ -n "${path}" ] || die "path is empty"
  path_has_glob "${path}" && die "path must not contain glob characters"
  case "${path}" in
    /*) ;;
    *) die "path must be absolute and explicit" ;;
  esac
  case "${path}" in
    *'..'*) die "path must not contain .." ;;
  esac
  if [ "${path}" = "/" ]; then
    printf '/'
    return 0
  fi
  printf '%s' "${path}" | sed 's#/*$##'
}

resolve_explicit_path() {
  local path resolved
  path=$(clean_abs_path "$1")
  if command -v realpath >/dev/null 2>&1; then
    if [ -e "${path}" ] || [ -L "${path}" ]; then
      resolved=$(realpath -e "${path}") || die "unable to resolve path"
    else
      resolved=$(realpath -m "${path}")
    fi
  else
    resolved=${path}
  fi
  resolved=$(clean_abs_path "${resolved}")
  if [ "${resolved}" != "${path}" ]; then
    die "path is unresolved or changes after canonicalization"
  fi
  printf '%s' "${resolved}"
}

assert_safe_target() {
  local path role isolated
  path=$(resolve_explicit_path "$1")
  role=${2:-target}
  isolated=${3:-0}
  is_broad_path "${path}" && die "${role} is too broad: refusing ${path}"
  if is_allowed_container_path "${path}"; then
    printf '%s' "${path}"
    return 0
  fi
  if is_allowed_host_path "${path}"; then
    printf '%s' "${path}"
    return 0
  fi
  if [ "${isolated}" = "1" ]; then
    die "${role} is not an isolated restore-test path: ${path}"
  fi
  die "${role} is not an allowed explicit path: ${path}"
}

assert_not_source_volume() {
  local path="$1"
  case "${path}" in
    /var/lib/postgresql|/var/lib/postgresql/*)
      die "refusing to mutate the live PostgreSQL data directory"
      ;;
    /var/lib/radicale|/var/lib/radicale/*)
      die "refusing to mutate the live Radicale data directory"
      ;;
  esac
}

dir_is_empty() {
  local path="$1"
  [ -d "${path}" ] || return 0
  [ -z "$(find "${path}" -mindepth 1 -maxdepth 1 -print -quit)" ]
}

wipe_bounded_dir() {
  local path
  path=$(resolve_explicit_path "$1")
  case "${path}" in
    /staging|/staging/*|/restore/verify|/restore/verify/*|/restore/bundle|/restore/bundle/*)
      ;;
    /var/tmp/foyer-backup.*|/var/tmp/foyer-restore-test.*|/var/tmp/foyer-backup-test.*)
      ;;
    "${FOYER_BACKUP_ROOT}/.local/staging"|"${FOYER_BACKUP_ROOT}/.local/staging"/*)
      ;;
    *)
      die "refusing to wipe path outside the bounded staging policy"
      ;;
  esac
  [ -e "${path}" ] || return 0
  [ -d "${path}" ] || die "wipe target is not a directory"
  find "${path}" -mindepth 1 -maxdepth 1 -exec rm -rf -- {} +
  if [ "${2:-keep-root}" = "remove-root" ]; then
    rmdir -- "${path}"
  fi
}

sha256_file() {
  sha256sum -- "$1" | awk '{print $1}'
}

file_size() {
  wc -c < "$1" | tr -d ' '
}

is_identity_filename() {
  local name
  name=$(basename -- "$1")
  case "${name}" in
    *age-identity*|age-identity.txt|identity.txt|AGE-SECRET-KEY*|*.agekey)
      return 0
      ;;
  esac
  return 1
}

age_recipient() {
  if [ -n "${FOYER_BACKUP_AGE_RECIPIENT:-}" ]; then
    printf '%s' "${FOYER_BACKUP_AGE_RECIPIENT}"
    return 0
  fi
  if [ -n "${FOYER_BACKUP_AGE_RECIPIENT_FILE:-}" ]; then
    tr -d '\r\n' < "${FOYER_BACKUP_AGE_RECIPIENT_FILE}"
    return 0
  fi
  die "FOYER_BACKUP_AGE_RECIPIENT is not set"
}

require_age_identity_file() {
  local file="${FOYER_BACKUP_AGE_IDENTITY_FILE:-}"
  [ -n "${file}" ] || die "FOYER_BACKUP_AGE_IDENTITY_FILE is required for restore/verify"
  file=$(assert_safe_target "${file}" "age identity" "${FOYER_RESTORE_ISOLATED:-0}")
  [ -f "${file}" ] || die "age identity file not found"
  if ! grep -Eq '^(AGE-SECRET-KEY-1|-----BEGIN AGE ENCRYPTED PRIVATE KEY-----)' "${file}"; then
    die "age identity file does not look like an age secret key"
  fi
  printf '%s' "${file}"
}

s3_prefix() {
  local prefix="${FOYER_BACKUP_S3_PREFIX:-}"
  prefix=${prefix#/}
  if [ -n "${prefix}" ]; then
    case "${prefix}" in
      */) ;;
      *) prefix="${prefix}/" ;;
    esac
  fi
  printf '%s' "${prefix}"
}

object_key_for_bundle() {
  printf '%s%s.tar.zst.age' "$(s3_prefix)" "$1"
}

validate_object_key() {
  local key="$1"
  [ -n "${key}" ] || die "object key is empty"
  case "${key}" in
    /*|*..*|*'*'*|*'?'*|*'['*)
      die "object key is invalid"
      ;;
  esac
  printf '%s' "${key}"
}

compose_env_file() {
  local env_file="${FOYER_COMPOSE_ENV_FILE:-${FOYER_DEPLOY_ROOT}/.env}"
  if [ ! -f "${env_file}" ]; then
    env_file="${FOYER_DEPLOY_ROOT}/.env.example"
  fi
  printf '%s' "${env_file}"
}

compose_env_value() {
  local key="$1"
  local file
  file=$(compose_env_file)
  awk -F= -v k="${key}" '$1==k {print substr($0, index($0,"=")+1)}' "${file}" | tail -n 1
}

compose_file() {
  printf '%s' "${FOYER_COMPOSE_FILE:-${FOYER_DEPLOY_ROOT}/compose.yaml}"
}

compose_cmd() {
  local command
  command="docker compose --env-file $(compose_env_file) -f $(compose_file)"
  if [ -n "${FOYER_COMPOSE_PROJECT_NAME:-}" ]; then
    command="${command} --project-name ${FOYER_COMPOSE_PROJECT_NAME}"
  fi
  printf '%s' "${command}"
}

# Host loopback MinIO is published on 19000. The backup container must use the
# Compose service name; remap only that documented local-test endpoint.
normalize_s3_endpoint_for_container() {
  case "${FOYER_BACKUP_S3_ENDPOINT:-}" in
    http://127.0.0.1:19000|http://localhost:19000|http://[::1]:19000)
      FOYER_BACKUP_S3_ENDPOINT=http://minio:9000
      export FOYER_BACKUP_S3_ENDPOINT
      ;;
  esac
}
