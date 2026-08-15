package com.amazity.foyer.sync

import com.amazity.foyer.bookmarks.bookmarkTables
import com.amazity.foyer.calendar.calendarTables
import com.amazity.foyer.contacts.contactsTables
import com.amazity.foyer.notes.notesTables
import com.amazity.foyer.tasks.taskTables
import com.powersync.db.schema.Schema
import com.powersync.db.schema.Table

const val PERSONAL_REPLICA_FILENAME = "foyer-personal-powersync.db"

const val CLIENT_OPERATION = "client_operation"
const val OPERATION_ID = "operation_id"
const val EXPECTED_REVISION = "expected_revision"
const val EXPECTED_ETAG = "expected_etag"
const val DELETED_LOCAL = "deleted_local"
const val CLIENT_PAYLOAD = "client_payload"

/**
 * One PowerSync schema for every hosted personal-data surface. Synced columns match
 * the server streams; client-only metadata stays local to the upload queue.
 */
fun personalDataTables(): List<Table> =
    notesTables() + taskTables() + calendarTables() + contactsTables() + bookmarkTables()

fun personalDataSchema(): Schema = Schema(*personalDataTables().toTypedArray())

fun personalDataTableNames(): Set<String> = personalDataTables().map { it.name }.toSet()
