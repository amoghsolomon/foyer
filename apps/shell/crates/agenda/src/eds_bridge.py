import json
import os
import sys
import time

import gi

gi.require_version("ECal", "2.0")
gi.require_version("EDataServer", "1.2")
gi.require_version("ICalGLib", "3.0")

from gi.repository import ECal, EDataServer, ICalGLib


PROTOCOL_VERSION = 1
MAX_SOURCES = 32
MAX_EVENTS = 768
MAX_TASKS = 256
CONNECT_TIMEOUT_SECONDS = 4
DISCOVERY_DEADLINE = time.monotonic() + 24
START_SECONDS = int(os.environ["FOYER_SHELL_AGENDA_START_SECONDS"])
END_SECONDS = int(os.environ["FOYER_SHELL_AGENDA_END_SECONDS"])
DEBUG = os.environ.get("FOYER_SHELL_AGENDA_DEBUG") == "1"


def debug(message):
    if DEBUG:
        print(message, file=sys.stderr, flush=True)


def bounded(value, length):
    if value is None:
        return ""
    return str(value)[:length]


def text_value(value):
    if value is None:
        return ""
    try:
        return bounded(value.get_value(), 512)
    except Exception:
        return bounded(value, 512)


def component_description(component):
    try:
        descriptions = component.get_descriptions() or []
        return text_value(descriptions[0]) if descriptions else ""
    except Exception:
        return ""


def component_time(component_date_time, client):
    if component_date_time is None:
        return None
    value = component_date_time.get_value()
    if value is None or value.is_null_time() or not value.is_valid_time():
        return None
    zone = value.get_timezone()
    if zone is None:
        tzid = component_date_time.get_tzid()
        if tzid:
            try:
                _success, zone = client.get_timezone_sync(tzid, None)
            except Exception:
                zone = None
    if zone is None:
        try:
            zone = client.get_default_timezone()
        except Exception:
            zone = None
    seconds = value.as_timet_with_zone(zone) if zone else value.as_timet()
    return {"milliseconds": int(seconds) * 1000, "all_day": value.is_date()}


def source_record(source, kind):
    return {
        "id": bounded(source.get_uid(), 256),
        "name": bounded(source.get_display_name(), 160) or "Unnamed source",
        "kind": kind,
        "writable": bool(source.get_writable()),
    }


def event_record(source, component, client, instance_start, instance_end):
    uid = bounded(component.get_uid(), 256)
    if not uid:
        return None
    start = component_time(component.get_dtstart(), client)
    end = component_time(component.get_dtend(), client)
    recurrence_id = bounded(component.get_recurid_as_string(), 256)
    return {
        "source_id": source["id"],
        "component_uid": uid,
        "recurrence_id": recurrence_id or None,
        "kind": "event",
        "title": text_value(component.get_summary()) or "Untitled event",
        "description": component_description(component),
        "location": bounded(component.get_location(), 256),
        "start_ms": int(instance_start) * 1000 if instance_start else (start or {}).get("milliseconds"),
        "end_ms": int(instance_end) * 1000 if instance_end else (end or {}).get("milliseconds"),
        "all_day": start["all_day"] if start else False,
        "due_ms": None,
        "completed": False,
    }


def task_record(source, component, client):
    uid = bounded(component.get_uid(), 256)
    if not uid:
        return None
    due = component_time(component.get_due(), client)
    completed = component.get_percent_complete() >= 100
    try:
        completed = completed or component.get_status() == ICalGLib.PropertyStatus.COMPLETED
    except Exception:
        pass
    try:
        completed = completed or component.get_completed() is not None
    except Exception:
        pass
    return {
        "source_id": source["id"],
        "component_uid": uid,
        "recurrence_id": bounded(component.get_recurid_as_string(), 256) or None,
        "kind": "task",
        "title": text_value(component.get_summary()) or "Untitled task",
        "description": component_description(component),
        "location": "",
        "start_ms": None,
        "end_ms": None,
        "all_day": False,
        "due_ms": due["milliseconds"] if due else None,
        "completed": completed,
    }


def load_source(source, kind, source_type, output, counts):
    source_info = source_record(source, kind)
    output["sources"].append(source_info)
    debug(f"{kind}:connecting")
    try:
        client = ECal.Client.connect_sync(source, source_type, CONNECT_TIMEOUT_SECONDS, None)
        debug(f"{kind}:connected")
        if kind == "calendar":
            def add_event(component, instance_start, instance_end, *_unused):
                if counts["events"] >= MAX_EVENTS:
                    return False
                item = event_record(source_info, component, client, instance_start, instance_end)
                if item:
                    output["items"].append(item)
                    counts["events"] += 1
                return True

            client.generate_instances_sync(
                START_SECONDS, END_SECONDS, None, add_event, None
            )
            debug(f"{kind}:instances-loaded")
        else:
            _success, components = client.get_object_list_as_comps_sync("#t", None)
            debug(f"{kind}:objects-loaded")
            for component in components:
                if counts["tasks"] >= MAX_TASKS:
                    break
                item = task_record(source_info, component, client)
                if item:
                    output["items"].append(item)
                    counts["tasks"] += 1
    except Exception as error:
        output["errors"].append(
            f"{source_info['name']}: {bounded(error, 240)}"
        )


def main():
    output = {"protocol_version": PROTOCOL_VERSION, "sources": [], "items": [], "errors": []}
    counts = {"events": 0, "tasks": 0}
    try:
        debug("registry:connecting")
        registry = EDataServer.SourceRegistry.new_sync(None)
        debug("registry:connected")
        calendars = registry.list_enabled(EDataServer.SOURCE_EXTENSION_CALENDAR)[:MAX_SOURCES]
        tasks = registry.list_enabled(EDataServer.SOURCE_EXTENSION_TASK_LIST)[:MAX_SOURCES]
        for source in calendars:
            if time.monotonic() >= DISCOVERY_DEADLINE:
                output["errors"].append("Calendar source discovery reached its time budget")
                break
            load_source(source, "calendar", ECal.ClientSourceType.EVENTS, output, counts)
        for source in tasks:
            if time.monotonic() >= DISCOVERY_DEADLINE:
                output["errors"].append("Task source discovery reached its time budget")
                break
            load_source(source, "task_list", ECal.ClientSourceType.TASKS, output, counts)
    except Exception as error:
        output["errors"].append(bounded(error, 240))

    debug("output:serializing")
    print(json.dumps(output, ensure_ascii=False, separators=(",", ":")), flush=True)
    debug("output:complete")
    # Avoid finalizing introspected ECal clients in the short-lived helper. EDS owns all state and
    # the process has no pending writes; raw exit prevents runtime-specific teardown faults.
    os._exit(0)


if __name__ == "__main__":
    main()
