#!/usr/bin/env bash
# Seed representative canonical state. Robust to schema that another component
# may still be changing: API domains are attempted independently, and device
# or auth rows are inserted only when matching tables already exist.
set -euo pipefail

SCRIPT_DIR=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
# shellcheck source=../scripts/lib.sh
. "${SCRIPT_DIR}/../scripts/lib.sh"

API="${FOYER_DRILL_API:-http://127.0.0.1:3583}"
if [ -z "${FOYER_DEV_TOKEN:-}" ]; then
  FOYER_DEV_TOKEN=$(compose_env_value FOYER_DEV_TOKEN)
fi
TOKEN="${FOYER_DEV_TOKEN:-foyer-dev-token-do-not-use-outside-development}"
REPORT="${FOYER_DRILL_REPORT:-${FOYER_BACKUP_ROOT}/.local/drill-report.txt}"
DAV_URL="${FOYER_DRILL_DAV:-http://127.0.0.1:5232}"
if [ -z "${FOYER_DAV_USERNAME:-}" ]; then
  FOYER_DAV_USERNAME=$(compose_env_value FOYER_DAV_USERNAME)
fi
if [ -z "${FOYER_DAV_PASSWORD:-}" ]; then
  FOYER_DAV_PASSWORD=$(compose_env_value FOYER_DAV_PASSWORD)
fi
DAV_USER="${FOYER_DAV_USERNAME:-foyer}"
DAV_PASSWORD="${FOYER_DAV_PASSWORD:-foyer-dev-dav-password-do-not-use-outside-development}"
if [ -z "${FOYER_DEV_USER_ID:-}" ]; then
  FOYER_DEV_USER_ID=$(compose_env_value FOYER_DEV_USER_ID)
fi
DAV_USER_PATH="${FOYER_DEV_USER_ID:-dev-user}"
COMPOSE_ENV=$(compose_env_file)
COMPOSE=(docker compose --env-file "${COMPOSE_ENV}" -f "${FOYER_DEPLOY_ROOT}/compose.yaml")
SEED_TMP=$(mktemp -d /var/tmp/foyer-backup-test.XXXXXX)
chmod 700 "${SEED_TMP}"
trap 'wipe_bounded_dir "${SEED_TMP}" remove-root || true' EXIT

mkdir -p "$(dirname -- "${REPORT}")"
: > "${REPORT}"

report() {
  printf '%s\n' "$*" | tee -a "${REPORT}"
}

uuid() {
  python3 -c 'import uuid; print(uuid.uuid4())'
}

api() {
  local method=$1
  local path=$2
  local body=${3:-}
  if [ -n "${body}" ]; then
    curl -sfS --max-time 20 -X "${method}" \
      -H "Authorization: Bearer ${TOKEN}" \
      -H "Content-Type: application/json" \
      -d "${body}" \
      "${API}${path}"
  else
    curl -sfS --max-time 20 -X "${method}" \
      -H "Authorization: Bearer ${TOKEN}" \
      "${API}${path}"
  fi
}

try_api() {
  local label=$1
  shift
  if output=$("$@" 2>"${SEED_TMP}/api.err"); then
    report "seeded ${label}"
    printf '%s' "${output}"
    return 0
  fi
  report "skipped ${label}: $(tr '\n' ' ' <"${SEED_TMP}/api.err")"
  return 1
}

FOLDER_ID=aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa1
NOTE_ID=aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa2
BOOKMARK_FOLDER_ID=aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa3
BOOKMARK_ID=aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa4
CALENDAR_ID=aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa5
EVENT_ID=aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa6
TASK_LIST_ID=aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa7
TASK_ID=aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa8
ADDRESS_BOOK_ID=aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa9
CONTACT_ID=aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaa10

seed_notes() {
  try_api "notes folder" api POST /v1/folders "$(cat <<EOF
{"operationId":"$(uuid)","id":"${FOLDER_ID}","name":"Backup drill notes"}
EOF
)" >/dev/null || return 0
  try_api "note" api POST /v1/notes "$(cat <<EOF
{"operationId":"$(uuid)","id":"${NOTE_ID}","folderId":"${FOLDER_ID}","title":"Backup drill","body":"Canonical note used by the restore drill.\n"}
EOF
)" >/dev/null || true
}

seed_bookmarks() {
  try_api "bookmark folder" api POST /v1/bookmark-folders "$(cat <<EOF
{"operationId":"$(uuid)","id":"${BOOKMARK_FOLDER_ID}","name":"Backup drill bookmarks"}
EOF
)" >/dev/null || return 0
  try_api "bookmark" api POST /v1/bookmarks "$(cat <<EOF
{"operationId":"$(uuid)","id":"${BOOKMARK_ID}","folderId":"${BOOKMARK_FOLDER_ID}","url":"https://example.test/foyer-backup","title":"Backup drill bookmark","description":"Canonical bookmark","tags":["backup","drill"],"favorite":true}
EOF
)" >/dev/null || true
}

seed_calendar() {
  try_api "calendar" api POST /v1/calendars "$(cat <<EOF
{"operationId":"$(uuid)","id":"${CALENDAR_ID}","displayName":"Backup drill calendar","description":"Canonical calendar"}
EOF
)" >/dev/null || return 0
  try_api "event" api POST /v1/events "$(cat <<EOF
{"operationId":"$(uuid)","id":"${EVENT_ID}","calendarId":"${CALENDAR_ID}","summary":"Backup drill event","allDay":true,"dtstart":"2026-08-15","description":"Canonical event"}
EOF
)" >/dev/null || true
}

seed_tasks() {
  try_api "task list" api POST /v1/task-lists "$(cat <<EOF
{"operationId":"$(uuid)","id":"${TASK_LIST_ID}","name":"Backup drill tasks"}
EOF
)" >/dev/null || return 0
  try_api "task" api POST /v1/tasks "$(cat <<EOF
{"operationId":"$(uuid)","id":"${TASK_ID}","listId":"${TASK_LIST_ID}","title":"Backup drill task","description":"Canonical task","priority":1}
EOF
)" >/dev/null || true
}

seed_contacts() {
  try_api "address book" api POST /v1/address-books "$(cat <<EOF
{"operationId":"$(uuid)","id":"${ADDRESS_BOOK_ID}","displayName":"Backup drill contacts"}
EOF
)" >/dev/null || return 0
  try_api "contact" api POST /v1/contacts "$(cat <<EOF
{"operationId":"$(uuid)","id":"${CONTACT_ID}","addressBookId":"${ADDRESS_BOOK_ID}","displayName":"Backup Drill","name":{"givenName":"Backup","familyName":"Drill"},"emails":[{"value":"backup@example.test","type":"work"}]}
EOF
)" >/dev/null || true
}

put_dav() {
  local href=$1
  local type=$2
  local file=$3
  local label=$4
  if curl -sfS --max-time 10 -u "${DAV_USER}:${DAV_PASSWORD}" \
    -H "Content-Type: ${type}" \
    -T "${file}" \
    "${href}" >/dev/null 2>"${SEED_TMP}/dav.err"; then
    report "seeded ${label}"
    return 0
  fi
  report "skipped ${label}: $(tr '\n' ' ' <"${SEED_TMP}/dav.err")"
  return 1
}

ensure_dav_collection() {
  local href=$1
  curl -sfS --max-time 10 -u "${DAV_USER}:${DAV_PASSWORD}" -X MKCOL "${href}" \
    >/dev/null 2>"${SEED_TMP}/dav.err" && return 0
  curl -sfS --max-time 10 -u "${DAV_USER}:${DAV_PASSWORD}" -o /dev/null \
    -w '%{http_code}' "${href}" 2>/dev/null | grep -Eq '200|204|207'
}

seed_direct_dav() {
  local cal_href="${DAV_URL}/${DAV_USER_PATH}/calendars/backup-drill-direct/"
  local task_href="${DAV_URL}/${DAV_USER_PATH}/tasks/backup-drill-direct/"
  local book_href="${DAV_URL}/${DAV_USER_PATH}/addressbooks/backup-drill-direct/"
  if ! ensure_dav_collection "${DAV_URL}/${DAV_USER_PATH}/" \
    && ! ensure_dav_collection "${DAV_URL}/${DAV_USER_PATH}/calendars/"; then
    report "skipped direct Radicale collections: $(tr '\n' ' ' <"${SEED_TMP}/dav.err")"
    return 0
  fi
  ensure_dav_collection "${DAV_URL}/${DAV_USER_PATH}/calendars/" || true
  ensure_dav_collection "${DAV_URL}/${DAV_USER_PATH}/tasks/" || true
  ensure_dav_collection "${DAV_URL}/${DAV_USER_PATH}/addressbooks/" || true
  if ensure_dav_collection "${cal_href}"; then
    cat > "${SEED_TMP}/event.ics" <<'ICS'
BEGIN:VCALENDAR
VERSION:2.0
PRODID:-//Foyer//Backup drill//EN
BEGIN:VEVENT
UID:backup-drill-direct@example.test
DTSTAMP:20260815T000000Z
DTSTART;VALUE=DATE:20260816
SUMMARY:Direct Radicale backup drill event
END:VEVENT
END:VCALENDAR
ICS
    put_dav "${cal_href}backup-drill-direct.ics" text/calendar "${SEED_TMP}/event.ics" \
      "direct Radicale calendar file" || true
  else
    report "skipped direct Radicale calendar collection: $(tr '\n' ' ' <"${SEED_TMP}/dav.err")"
  fi
  if ensure_dav_collection "${task_href}"; then
    cat > "${SEED_TMP}/task.ics" <<'ICS'
BEGIN:VCALENDAR
VERSION:2.0
PRODID:-//Foyer//Backup drill//EN
BEGIN:VTODO
UID:backup-drill-direct-task@example.test
DTSTAMP:20260815T000000Z
SUMMARY:Direct Radicale backup drill task
STATUS:NEEDS-ACTION
END:VTODO
END:VCALENDAR
ICS
    put_dav "${task_href}backup-drill-direct.ics" text/calendar "${SEED_TMP}/task.ics" \
      "direct Radicale task file" || true
  else
    report "skipped direct Radicale task collection: $(tr '\n' ' ' <"${SEED_TMP}/dav.err")"
  fi
  if ensure_dav_collection "${book_href}"; then
    cat > "${SEED_TMP}/contact.vcf" <<'VCF'
BEGIN:VCARD
VERSION:4.0
UID:backup-drill-direct-contact
FN:Direct Backup Drill
EMAIL:direct-backup@example.test
END:VCARD
VCF
    put_dav "${book_href}backup-drill-direct.vcf" text/vcard "${SEED_TMP}/contact.vcf" \
      "direct Radicale contact file" || true
  else
    report "skipped direct Radicale address book: $(tr '\n' ' ' <"${SEED_TMP}/dav.err")"
  fi
}

seed_devices_if_present() {
  local sql
  sql=$(cat <<'SQL'
DO $$
DECLARE
  rec record;
  inserted boolean := false;
  cols text;
BEGIN
  FOR rec IN
    SELECT table_name
    FROM information_schema.tables
    WHERE table_schema = 'public'
      AND table_type = 'BASE TABLE'
      AND (
        table_name IN ('devices', 'enrolled_devices', 'device_keys', 'auth_devices')
        OR table_name LIKE '%device%'
      )
  LOOP
    SELECT string_agg(column_name, ',' ORDER BY ordinal_position)
      INTO cols
    FROM information_schema.columns
    WHERE table_schema = 'public' AND table_name = rec.table_name;

    IF cols LIKE '%thumbprint%' AND cols LIKE '%user_id%' THEN
      BEGIN
        EXECUTE format(
          'INSERT INTO %I (thumbprint, user_id) VALUES ($1, $2) ON CONFLICT DO NOTHING',
          rec.table_name
        ) USING 'backup-drill-device-thumbprint', 'dev-user';
        inserted := true;
        RAISE NOTICE 'seeded % with thumbprint/user_id', rec.table_name;
      EXCEPTION WHEN OTHERS THEN
        RAISE NOTICE 'skipped %: %', rec.table_name, SQLERRM;
      END;
    ELSIF cols LIKE '%public_jwk%' AND cols LIKE '%user_id%' THEN
      BEGIN
        EXECUTE format(
          'INSERT INTO %I (user_id, public_jwk) VALUES ($1, $2::jsonb) ON CONFLICT DO NOTHING',
          rec.table_name
        ) USING 'dev-user', '{"kty":"EC","crv":"P-256","x":"backup","y":"drill"}';
        inserted := true;
        RAISE NOTICE 'seeded % with public_jwk/user_id', rec.table_name;
      EXCEPTION WHEN OTHERS THEN
        RAISE NOTICE 'skipped %: %', rec.table_name, SQLERRM;
      END;
    ELSE
      RAISE NOTICE 'skipped %: columns not recognized (% )', rec.table_name, cols;
    END IF;
  END LOOP;

  IF NOT inserted THEN
    RAISE NOTICE 'device/auth tables not present or not recognized';
  END IF;
END
$$;
SQL
)
  if "${COMPOSE[@]}" exec -T postgres psql -v ON_ERROR_STOP=1 -U "${POSTGRES_USER:-foyer}" -d "${POSTGRES_DATABASE:-foyer}" -c "${sql}" \
    >"${SEED_TMP}/devices.out" 2>"${SEED_TMP}/devices.err"; then
    report "device/auth seed result: $(tr '\n' ' ' <"${SEED_TMP}/devices.out")"
  else
    report "skipped device/auth SQL seed: $(tr '\n' ' ' <"${SEED_TMP}/devices.err")"
  fi
}

# Insert notes/bookmarks only when those tables already exist. Column names are
# read from information_schema so a concurrent server migration cannot make
# this script guess a stale table shape.
seed_sql_fallback() {
  local sql
  sql=$(cat <<SQL
DO \$\$
DECLARE
  note_cols text;
  folder_cols text;
  bookmark_cols text;
  bookmark_folder_cols text;
BEGIN
  SELECT string_agg(column_name, ',' ORDER BY ordinal_position) INTO folder_cols
    FROM information_schema.columns
    WHERE table_schema='public' AND table_name='notes_folders';
  SELECT string_agg(column_name, ',' ORDER BY ordinal_position) INTO note_cols
    FROM information_schema.columns
    WHERE table_schema='public' AND table_name='notes';
  IF folder_cols IS NOT NULL AND note_cols IS NOT NULL
     AND folder_cols LIKE '%id%' AND folder_cols LIKE '%user_id%' AND folder_cols LIKE '%name%'
     AND note_cols LIKE '%id%' AND note_cols LIKE '%folder_id%' AND note_cols LIKE '%title%' AND note_cols LIKE '%body%' THEN
    BEGIN
      EXECUTE \$q\$
        INSERT INTO notes_folders (id, user_id, parent_id, name, position, revision, created_at, updated_at)
        VALUES ('${FOLDER_ID}', '${FOYER_DEV_USER_ID:-dev-user}', NULL, 'Backup drill notes', 0, 1, NOW(), NOW())
        ON CONFLICT (id) DO NOTHING
      \$q\$;
      EXECUTE \$q\$
        INSERT INTO notes (id, user_id, folder_id, title, body, revision, created_at, updated_at)
        VALUES ('${NOTE_ID}', '${FOYER_DEV_USER_ID:-dev-user}', '${FOLDER_ID}', 'Backup drill',
                'Canonical note used by the restore drill.' || chr(10), 1, NOW(), NOW())
        ON CONFLICT (id) DO NOTHING
      \$q\$;
      RAISE NOTICE 'seeded notes tables';
    EXCEPTION WHEN OTHERS THEN
      RAISE NOTICE 'skipped notes SQL: %', SQLERRM;
    END;
  ELSE
    RAISE NOTICE 'notes tables not present or columns not recognized';
  END IF;

  SELECT string_agg(column_name, ',' ORDER BY ordinal_position) INTO bookmark_folder_cols
    FROM information_schema.columns
    WHERE table_schema='public' AND table_name='bookmarks_folders';
  SELECT string_agg(column_name, ',' ORDER BY ordinal_position) INTO bookmark_cols
    FROM information_schema.columns
    WHERE table_schema='public' AND table_name='bookmarks';
  IF bookmark_folder_cols IS NOT NULL AND bookmark_cols IS NOT NULL
     AND bookmark_cols LIKE '%url%' THEN
    BEGIN
      EXECUTE \$q\$
        INSERT INTO bookmarks_folders (id, user_id, parent_id, name, position, revision, created_at, updated_at)
        VALUES ('${BOOKMARK_FOLDER_ID}', '${FOYER_DEV_USER_ID:-dev-user}', NULL, 'Backup drill bookmarks', 0, 1, NOW(), NOW())
        ON CONFLICT (id) DO NOTHING
      \$q\$;
      EXECUTE \$q\$
        INSERT INTO bookmarks (id, user_id, folder_id, url, title, description, tags, favorite, archived, position, revision, created_at, updated_at)
        VALUES ('${BOOKMARK_ID}', '${FOYER_DEV_USER_ID:-dev-user}', '${BOOKMARK_FOLDER_ID}',
                'https://example.test/foyer-backup', 'Backup drill bookmark',
                'Canonical bookmark', '["backup","drill"]'::jsonb, true, false, 0, 1, NOW(), NOW())
        ON CONFLICT (id) DO NOTHING
      \$q\$;
      RAISE NOTICE 'seeded bookmarks tables';
    EXCEPTION WHEN OTHERS THEN
      RAISE NOTICE 'skipped bookmarks SQL: %', SQLERRM;
    END;
  ELSE
    RAISE NOTICE 'bookmarks tables not present or columns not recognized';
  END IF;
END
\$\$;
SQL
)
  if "${COMPOSE[@]}" exec -T postgres psql -v ON_ERROR_STOP=1 -U "${POSTGRES_USER:-foyer}" -d "${POSTGRES_DATABASE:-foyer}" -c "${sql}" \
    >"${SEED_TMP}/sql.out" 2>"${SEED_TMP}/sql.err"; then
    report "SQL fallback result: $(tr '\n' ' ' <"${SEED_TMP}/sql.out")"
  else
    report "skipped SQL fallback: $(tr '\n' ' ' <"${SEED_TMP}/sql.err")"
  fi
}

seed_secrets() {
  local dest="${FOYER_BACKUP_SECRETS_DIR:-${FOYER_BACKUP_ROOT}/.local/secrets}"
  mkdir -p "${dest}"
  chmod 700 "${dest}"
  printf 'backup-drill-signing-key\n' > "${dest}/token-signing.key"
  printf 'FOYER_TOKEN_SIGNING_KEY_FILE=/secrets/token-signing.key\n' > "${dest}/server-signing.env"
  chmod 600 "${dest}/token-signing.key" "${dest}/server-signing.env"
  report "seeded signing/secret files in ${dest}"
}

report "seed start api=${API}"
seed_secrets
seed_notes
seed_bookmarks
seed_calendar
seed_tasks
seed_contacts
seed_sql_fallback
seed_direct_dav
seed_devices_if_present
report "seed finished"
