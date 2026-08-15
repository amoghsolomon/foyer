package com.amazity.foyer.tasks

import com.powersync.db.schema.Column
import com.powersync.db.schema.Schema
import com.powersync.db.schema.Table

const val TASK_LISTS_TABLE = "task_lists"
const val TASKS_TABLE = "tasks"
const val CLIENT_OPERATION = "client_operation"
const val OPERATION_ID = "operation_id"
const val EXPECTED_REVISION = "expected_revision"
const val DELETED_LOCAL = "deleted_local"
const val CLIENT_PAYLOAD = "client_payload"

/**
 * Shared personal-data replica. All base apps attach to this one SQLite file.
 */
const val SHARED_REPLICA_FILENAME = "foyer-personal-powersync.db"

private fun clientColumns() = listOf(
    Column.text(CLIENT_OPERATION),
    Column.text(OPERATION_ID),
    Column.integer(EXPECTED_REVISION),
    Column.integer(DELETED_LOCAL),
    Column.text(CLIENT_PAYLOAD),
)

fun taskListTable(): Table = Table(
    name = TASK_LISTS_TABLE,
    columns = listOf(
        Column.text("user_id"),
        Column.text("name"),
        Column.integer("position"),
        Column.text("href"),
        Column.text("etag"),
        Column.text("ctag"),
        Column.text("sync_token"),
        Column.integer("revision"),
        Column.text("created_at"),
        Column.text("updated_at"),
    ) + clientColumns(),
)

fun taskTable(): Table = Table(
    name = TASKS_TABLE,
    columns = listOf(
        Column.text("user_id"),
        Column.text("list_id"),
        Column.text("title"),
        Column.text("description"),
        Column.text("due_at"),
        Column.text("due_local"),
        Column.text("due_time_zone"),
        Column.integer("due_all_day"),
        Column.integer("priority"),
        Column.integer("completed"),
        Column.text("completed_at"),
        Column.integer("position"),
        Column.text("href"),
        Column.text("etag"),
        Column.integer("revision"),
        Column.text("created_at"),
        Column.text("updated_at"),
    ) + clientColumns(),
)

fun taskTables(): List<Table> = listOf(taskListTable(), taskTable())

fun tasksSchema(extra: List<Table> = emptyList()): Schema =
    Schema(*(taskTables() + extra).toTypedArray())
