package com.amazity.foyer.notes

import com.powersync.db.schema.Column
import com.powersync.db.schema.Schema
import com.powersync.db.schema.Table

const val NOTES_FOLDERS_TABLE = "notes_folders"
const val NOTES_TABLE = "notes"
const val CLIENT_OPERATION = "client_operation"
const val OPERATION_ID = "operation_id"
const val EXPECTED_REVISION = "expected_revision"
const val DELETED_LOCAL = "deleted_local"
const val CLIENT_PAYLOAD = "client_payload"

/**
 * Client-only columns are absent from PowerSync streams. They persist the semantic
 * Foyer API command in the same SQLite transaction as the optimistic row change.
 */
private fun clientColumns() = listOf(
    Column.text(CLIENT_OPERATION),
    Column.text(OPERATION_ID),
    Column.integer(EXPECTED_REVISION),
    Column.integer(DELETED_LOCAL),
    Column.text(CLIENT_PAYLOAD),
)

fun notesFolderTable(): Table = Table(
    name = NOTES_FOLDERS_TABLE,
    columns = listOf(
        Column.text("user_id"),
        Column.text("parent_id"),
        Column.text("name"),
        Column.integer("position"),
        Column.integer("revision"),
        Column.text("created_at"),
        Column.text("updated_at"),
    ) + clientColumns(),
)

fun noteTable(): Table = Table(
    name = NOTES_TABLE,
    columns = listOf(
        Column.text("user_id"),
        Column.text("folder_id"),
        Column.text("title"),
        Column.text("body"),
        Column.integer("revision"),
        Column.text("created_at"),
        Column.text("updated_at"),
    ) + clientColumns(),
)

fun notesTables(): List<Table> = listOf(notesFolderTable(), noteTable())

fun notesSchema(): Schema = Schema(*notesTables().toTypedArray())
