package com.amazity.foyer.calendar

import com.powersync.db.schema.Column
import com.powersync.db.schema.Schema
import com.powersync.db.schema.Table

const val CLIENT_OPERATION = "client_operation"
const val OPERATION_ID = "operation_id"
const val EXPECTED_REVISION = "expected_revision"
const val EXPECTED_ETAG = "expected_etag"
const val DELETED_LOCAL = "deleted_local"
const val CLIENT_PAYLOAD = "client_payload"

const val CALENDARS_TABLE = "calendar_calendars"
const val EVENTS_TABLE = "calendar_events"

private fun clientColumns() = listOf(
    Column.text(CLIENT_OPERATION),
    Column.text(OPERATION_ID),
    Column.integer(EXPECTED_REVISION),
    Column.text(EXPECTED_ETAG),
    Column.integer(DELETED_LOCAL),
    Column.text(CLIENT_PAYLOAD),
)

fun calendarTable(): Table = Table(
    name = CALENDARS_TABLE,
    columns = listOf(
        Column.text("user_id"),
        Column.text("uid"),
        Column.text("href"),
        Column.text("etag"),
        Column.text("display_name"),
        Column.text("description"),
        Column.text("color"),
        Column.text("ctag"),
        Column.text("sync_token"),
        Column.integer("revision"),
        Column.text("created_at"),
        Column.text("updated_at"),
    ) + clientColumns(),
)

fun eventTable(): Table = Table(
    name = EVENTS_TABLE,
    columns = listOf(
        Column.text("user_id"),
        Column.text("calendar_id"),
        Column.text("uid"),
        Column.text("href"),
        Column.text("etag"),
        Column.text("summary"),
        Column.text("description"),
        Column.text("location"),
        Column.integer("all_day"),
        Column.text("dtstart"),
        Column.text("dtend"),
        Column.text("tzid"),
        Column.text("rrule"),
        Column.text("exdates"),
        Column.integer("revision"),
        Column.text("created_at"),
        Column.text("updated_at"),
    ) + clientColumns(),
)

fun calendarTables(): List<Table> = listOf(calendarTable(), eventTable())

/**
 * Shared replica schema. Synced columns match the PostgreSQL projection;
 * client-only columns stay local to the upload queue.
 */
val calendarSchema: Schema = Schema(*calendarTables().toTypedArray())
