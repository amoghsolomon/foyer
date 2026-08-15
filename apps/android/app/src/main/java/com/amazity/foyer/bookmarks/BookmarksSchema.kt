package com.amazity.foyer.bookmarks

import com.powersync.db.schema.Column
import com.powersync.db.schema.Schema
import com.powersync.db.schema.Table

const val BOOKMARKS_FOLDERS_TABLE = "bookmarks_folders"
const val BOOKMARKS_TABLE = "bookmarks"
const val CLIENT_OPERATION = "client_operation"
const val OPERATION_ID = "operation_id"
const val EXPECTED_REVISION = "expected_revision"
const val DELETED_LOCAL = "deleted_local"
const val CLIENT_PAYLOAD = "client_payload"

private fun clientColumns() = listOf(
    Column.text(CLIENT_OPERATION),
    Column.text(OPERATION_ID),
    Column.integer(EXPECTED_REVISION),
    Column.integer(DELETED_LOCAL),
    Column.text(CLIENT_PAYLOAD),
)

fun bookmarkFolderTable(): Table = Table(
    name = BOOKMARKS_FOLDERS_TABLE,
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

fun bookmarkTable(): Table = Table(
    name = BOOKMARKS_TABLE,
    columns = listOf(
        Column.text("user_id"),
        Column.text("folder_id"),
        Column.text("url"),
        Column.text("title"),
        Column.text("description"),
        Column.text("tags"),
        Column.integer("favorite"),
        Column.integer("archived"),
        Column.integer("position"),
        Column.integer("revision"),
        Column.text("created_at"),
        Column.text("updated_at"),
    ) + clientColumns(),
)

fun bookmarkTables(): List<Table> = listOf(bookmarkFolderTable(), bookmarkTable())

fun bookmarksSchema(): Schema = Schema(*bookmarkTables().toTypedArray())
