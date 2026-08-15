#!/usr/bin/env python3
"""Compare normalized API-visible and Radicale canonical state."""

from __future__ import annotations

import json
import os
import sys
import urllib.error
import urllib.request
from pathlib import Path


def fetch(url: str, token: str) -> object:
    request = urllib.request.Request(
        url, headers={"Authorization": f"Bearer {token}"}
    )
    try:
        with urllib.request.urlopen(request, timeout=20) as response:
            return json.load(response)
    except urllib.error.HTTPError as error:
        return {"_error": error.code, "_body": error.read().decode("utf-8", "replace")}
    except Exception as error:  # noqa: BLE001
        return {"_error": str(error)}


def items(payload: object, key: str) -> list:
    if isinstance(payload, dict) and key in payload:
        value = payload[key]
        return value if isinstance(value, list) else []
    if isinstance(payload, list):
        return payload
    return []


def pick(row: dict, keys: list[str]) -> dict:
    return {key: row.get(key) for key in keys if key in row}


def normalize_notes(payload: object) -> list:
    keys = ["id", "title", "body", "folderId"]
    return sorted((pick(row, keys) for row in items(payload, "notes")), key=lambda r: json.dumps(r, sort_keys=True))


def normalize_folders(payload: object) -> list:
    keys = ["id", "name", "parentId"]
    return sorted((pick(row, keys) for row in items(payload, "folders")), key=lambda r: json.dumps(r, sort_keys=True))


def normalize_bookmarks(payload: object) -> list:
    keys = ["id", "url", "title", "description", "tags", "favorite", "archived"]
    return sorted((pick(row, keys) for row in items(payload, "bookmarks")), key=lambda r: json.dumps(r, sort_keys=True))


def normalize_calendars(payload: object) -> list:
    keys = ["displayName", "description", "uid"]
    return sorted((pick(row, keys) for row in items(payload, "calendars")), key=lambda r: json.dumps(r, sort_keys=True))


def normalize_events(payload: object) -> list:
    keys = ["summary", "description", "dtstart", "allDay", "uid"]
    return sorted((pick(row, keys) for row in items(payload, "events")), key=lambda r: json.dumps(r, sort_keys=True))


def normalize_task_lists(payload: object) -> list:
    keys = ["name"]
    return sorted((pick(row, keys) for row in items(payload, "taskLists") or items(payload, "lists")), key=lambda r: json.dumps(r, sort_keys=True))


def normalize_tasks(payload: object) -> list:
    keys = ["title", "description", "priority", "completed"]
    return sorted((pick(row, keys) for row in items(payload, "tasks")), key=lambda r: json.dumps(r, sort_keys=True))


def normalize_books(payload: object) -> list:
    keys = ["displayName", "description"]
    return sorted((pick(row, keys) for row in items(payload, "addressBooks") or items(payload, "books")), key=lambda r: json.dumps(r, sort_keys=True))


def normalize_contacts(payload: object) -> list:
    keys = ["displayName", "organization", "emails", "phones"]
    return sorted((pick(row, keys) for row in items(payload, "contacts")), key=lambda r: json.dumps(r, sort_keys=True))


ENDPOINTS = (
    ("notes", "/v1/notes", normalize_notes),
    ("note_folders", "/v1/folders", normalize_folders),
    ("bookmarks", "/v1/bookmarks", normalize_bookmarks),
    ("bookmark_folders", "/v1/bookmark-folders", normalize_folders),
    ("calendars", "/v1/calendars", normalize_calendars),
    ("events", "/v1/events", normalize_events),
    ("task_lists", "/v1/task-lists", normalize_task_lists),
    ("tasks", "/v1/tasks", normalize_tasks),
    ("address_books", "/v1/address-books", normalize_books),
    ("contacts", "/v1/contacts", normalize_contacts),
)


def snapshot(base: str, token: str) -> dict:
    result = {}
    for name, path, normalizer in ENDPOINTS:
        payload = fetch(base.rstrip("/") + path, token)
        if isinstance(payload, dict) and payload.get("_error"):
            result[name] = {"available": False, "error": payload["_error"]}
        else:
            result[name] = {"available": True, "items": normalizer(payload)}
    return result


def compare(left: dict, right: dict) -> tuple[list[str], list[str]]:
    mismatches: list[str] = []
    skipped: list[str] = []
    for name, _, _ in ENDPOINTS:
        lval = left.get(name, {})
        rval = right.get(name, {})
        if not lval.get("available"):
            skipped.append(f"{name}: source unavailable ({lval.get('error')})")
            continue
        if not rval.get("available"):
            skipped.append(f"{name}: restore unavailable ({rval.get('error')})")
            continue
        if lval.get("items") == [] and rval.get("items") == []:
            skipped.append(f"{name}: both empty (not exercised or projector not rebuilt)")
            continue
        if lval.get("items") != rval.get("items"):
            mismatches.append(name)
    return mismatches, skipped


def radicale_names(root: Path) -> list[str]:
    if not root.exists():
        return []
    names = []
    for path in root.rglob("*"):
        if path.is_file() and path.suffix in {".ics", ".vcf"}:
            names.append(str(path.relative_to(root)))
    return sorted(names)


def main() -> int:
    source = os.environ["FOYER_DRILL_API"]
    restored = os.environ["FOYER_DRILL_RESTORE_API"]
    token = os.environ.get(
        "FOYER_DEV_TOKEN", "foyer-dev-token-do-not-use-outside-development"
    )
    report_path = Path(os.environ["FOYER_DRILL_REPORT"])
    source_snap = snapshot(source, token)
    restore_snap = snapshot(restored, token)
    mismatches, skipped = compare(source_snap, restore_snap)

    source_radicale = [
        line for line in os.environ.get("FOYER_DRILL_SOURCE_RADICALE", "").splitlines() if line
    ]
    restore_radicale = [
        line for line in os.environ.get("FOYER_DRILL_RESTORE_RADICALE", "").splitlines() if line
    ]
    if source_radicale or restore_radicale:
        if source_radicale != restore_radicale:
            mismatches.append("radicale_files")
        elif not source_radicale:
            skipped.append("radicale_files: none collected")

    secrets_src = Path(os.environ.get("FOYER_BACKUP_SECRETS_DIR", ""))
    secrets_dst = Path(os.environ.get("FOYER_RESTORE_SECRETS_HOST_DIR", ""))
    if secrets_src.is_dir() and secrets_dst.is_dir():
        src_files = sorted(p.name for p in secrets_src.iterdir() if p.is_file())
        dst_files = sorted(p.name for p in secrets_dst.iterdir() if p.is_file())
        if src_files != dst_files:
            mismatches.append("secrets_filenames")
        else:
            for name in src_files:
                if (secrets_src / name).read_bytes() != (secrets_dst / name).read_bytes():
                    mismatches.append(f"secrets:{name}")

    payload = {
        "source": source_snap,
        "restored": restore_snap,
        "radicale_source": source_radicale,
        "radicale_restored": restore_radicale,
        "mismatches": mismatches,
        "skipped": skipped,
    }
    report_path.parent.mkdir(parents=True, exist_ok=True)
    report_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(f"compared API-visible state; mismatches={len(mismatches)} skipped={len(skipped)}")
    for item in skipped:
        print(f"  skip: {item}")
    for item in mismatches:
        print(f"  mismatch: {item}", file=sys.stderr)
    exercised = [
        name
        for name, data in source_snap.items()
        if data.get("available") and data.get("items")
    ]
    print("exercised: " + (", ".join(exercised) if exercised else "none"))
    return 1 if mismatches else 0


if __name__ == "__main__":
    raise SystemExit(main())
