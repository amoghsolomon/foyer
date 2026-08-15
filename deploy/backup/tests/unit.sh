#!/usr/bin/env bash
# Unit tests for path policy, wipe safety, and manifest validation.
set -euo pipefail

SCRIPT_DIR=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
# shellcheck source=../scripts/lib.sh
. "${SCRIPT_DIR}/../scripts/lib.sh"
# shellcheck source=../scripts/foyer-backup
TEST_SCRIPT_DIR=${SCRIPT_DIR}
. "${SCRIPT_DIR}/../scripts/foyer-backup"
SCRIPT_DIR=${TEST_SCRIPT_DIR}

failures=0
assert_ok() {
  local name=$1
  shift
  if ( "$@" >/dev/null ); then
    log "ok: ${name}"
  else
    log_err "FAIL: ${name}"
    failures=$((failures + 1))
  fi
}

assert_fail() {
  local name=$1
  shift
  if ( "$@" >/dev/null 2>&1 ); then
    log_err "FAIL: ${name} (expected failure)"
    failures=$((failures + 1))
  else
    log "ok: ${name}"
  fi
}

assert_fail "refuse root" assert_safe_target /
assert_fail "refuse /tmp" assert_safe_target /tmp
assert_fail "refuse /var" assert_safe_target /var
assert_fail "refuse relative" assert_safe_target relative/path
assert_fail "refuse glob" assert_safe_target '/var/tmp/foyer-backup.*'
assert_fail "refuse parent traversal" assert_safe_target /var/tmp/foyer-backup.x/../etc
assert_ok "allow container staging" assert_safe_target /staging
assert_ok "allow isolated secrets mount" assert_safe_target /restore/secrets
assert_fail "refuse live radicale restore target" assert_not_source_volume /var/lib/radicale
assert_fail "refuse live postgres data dir" assert_not_source_volume /var/lib/postgresql/data
assert_ok "allow prefixed object key" validate_object_key foyer/foyer-canonical-20260815T000000Z.tar.zst.age
assert_fail "refuse absolute object key" validate_object_key /foyer/bundle.tar.zst.age
assert_fail "refuse parent object key" validate_object_key foyer/../secret
assert_fail "refuse glob object key" validate_object_key 'foyer/*.age'

WORKDIR=$(mktemp -d /var/tmp/foyer-backup-test.XXXXXX)
chmod 700 "${WORKDIR}"
printf 'plain\n' > "${WORKDIR}/keep-me"
assert_fail "wipe refuses unrelated dir" wipe_bounded_dir /var/tmp
assert_ok "wipe allows bounded test dir" wipe_bounded_dir "${WORKDIR}"
if [ -e "${WORKDIR}/keep-me" ]; then
  log_err "FAIL: bounded wipe left contents"
  failures=$((failures + 1))
else
  log "ok: bounded wipe removed only staging contents"
fi
rmdir "${WORKDIR}" || true

MANIFEST_DIR=$(mktemp -d /var/tmp/foyer-backup-test.XXXXXX)
mkdir -p "${MANIFEST_DIR}/bundle/postgres" "${MANIFEST_DIR}/bundle/radicale" \
  "${MANIFEST_DIR}/bundle/secrets" "${MANIFEST_DIR}/radicale-source"
printf 'dump' > "${MANIFEST_DIR}/bundle/postgres/foyer.dump"
printf 'calendar' > "${MANIFEST_DIR}/radicale-source/item.ics"
tar -C "${MANIFEST_DIR}/radicale-source" -cf "${MANIFEST_DIR}/bundle/radicale/storage.tar" .
printf 'signing-key' > "${MANIFEST_DIR}/bundle/secrets/token-signing.key"
python3 - "${MANIFEST_DIR}/bundle" <<'PY'
import hashlib, json, os, sys
bundle = sys.argv[1]
files = []
for rel in ("postgres/foyer.dump", "radicale/storage.tar", "secrets/token-signing.key"):
    path = os.path.join(bundle, rel)
    data = open(path, "rb").read()
    files.append({
        "path": rel,
        "authority": rel.split("/")[0].replace("postgres", "postgresql"),
        "size": len(data),
        "sha256": hashlib.sha256(data).hexdigest(),
    })
manifest = {
    "format": "foyer-canonical-backup",
    "format_version": 1,
    "created_at": "2026-08-15T00:00:00Z",
    "bundle_id": "foyer-canonical-test",
    "authorities": ["postgresql", "radicale", "secrets"],
    "files": files,
    "schema": {"postgres_major": 18},
}
open(os.path.join(bundle, "manifest.json"), "w").write(json.dumps(manifest))
PY

if validate_unpacked "${MANIFEST_DIR}/bundle" >/dev/null; then
  log "ok: valid manifest checksums"
else
  log_err "FAIL: valid manifest was rejected"
  failures=$((failures + 1))
fi

printf 'tampered' > "${MANIFEST_DIR}/bundle/postgres/foyer.dump"
if validate_unpacked "${MANIFEST_DIR}/bundle" >/dev/null 2>&1; then
  log_err "FAIL: tampered dump was accepted"
  failures=$((failures + 1))
else
  log "ok: tampered dump is rejected"
fi

printf 'dump' > "${MANIFEST_DIR}/bundle/postgres/foyer.dump"
python3 - "${MANIFEST_DIR}/bundle" <<'PY'
import hashlib, json, os, sys, tarfile
bundle = sys.argv[1]
archive_path = os.path.join(bundle, "radicale", "storage.tar")
with tarfile.open(archive_path, "w") as archive:
    member = tarfile.TarInfo("escape-link")
    member.type = tarfile.SYMTYPE
    member.linkname = "../../outside"
    archive.addfile(member)
manifest_path = os.path.join(bundle, "manifest.json")
manifest = json.load(open(manifest_path, encoding="utf-8"))
for item in manifest["files"]:
    path = os.path.join(bundle, item["path"])
    data = open(path, "rb").read()
    item["size"] = len(data)
    item["sha256"] = hashlib.sha256(data).hexdigest()
open(manifest_path, "w", encoding="utf-8").write(json.dumps(manifest))
PY
if validate_unpacked "${MANIFEST_DIR}/bundle" >/dev/null 2>&1; then
  log_err "FAIL: unsafe Radicale archive member was accepted"
  failures=$((failures + 1))
else
  log "ok: unsafe Radicale archive member is rejected"
fi
rm -rf -- "${MANIFEST_DIR}"

# Staging cleanup on injected failure.
FAILDIR=$(mktemp -d /var/tmp/foyer-backup.XXXXXX)
(
  set -euo pipefail
  # shellcheck source=../scripts/lib.sh
  . "${SCRIPT_DIR}/../scripts/lib.sh"
  trap 'wipe_bounded_dir "${FAILDIR}" remove-root' EXIT
  printf 'secret-dump\n' > "${FAILDIR}/foyer.dump"
  false
) || true
if [ -e "${FAILDIR}" ]; then
  log_err "FAIL: staging directory remained after injected failure"
  failures=$((failures + 1))
  rm -rf -- "${FAILDIR}"
else
  log "ok: trap removed staging after injected failure"
fi

if python3 - "${SCRIPT_DIR}/../scripts/s3.py" <<'PY'
import importlib.util
import sys
from pathlib import Path

spec = importlib.util.spec_from_file_location("foyer_s3", sys.argv[1])
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
digest = module.sha256_hex(b"foyer")
assert len(digest) == 64
key = module.signing_key("test-secret", "20260815", "us-east-1", "s3")
assert isinstance(key, bytes) and len(key) == 32
print("ok")
PY
then
  log "ok: generic SigV4 helpers"
else
  log_err "FAIL: generic SigV4 helpers"
  failures=$((failures + 1))
fi

if [ "${failures}" -ne 0 ]; then
  die "${failures} unit test(s) failed"
fi
log "unit tests passed"
