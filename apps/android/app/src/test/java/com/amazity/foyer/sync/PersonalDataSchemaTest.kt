package com.amazity.foyer.sync

import com.amazity.foyer.bookmarks.BOOKMARKS_FOLDERS_TABLE
import com.amazity.foyer.bookmarks.BOOKMARKS_TABLE
import com.amazity.foyer.calendar.CALENDARS_TABLE
import com.amazity.foyer.calendar.EVENTS_TABLE
import com.amazity.foyer.contacts.ADDRESS_BOOKS_TABLE
import com.amazity.foyer.contacts.CONTACTS_TABLE
import com.amazity.foyer.notes.NOTES_FOLDERS_TABLE
import com.amazity.foyer.notes.NOTES_TABLE
import com.amazity.foyer.tasks.SHARED_REPLICA_FILENAME
import com.amazity.foyer.tasks.TASKS_TABLE
import com.amazity.foyer.tasks.TASK_LISTS_TABLE
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class PersonalDataSchemaTest {
    @Test
    fun sharedReplicaUsesOnePersonalDataFilename() {
        assertEquals("foyer-personal-powersync.db", PERSONAL_REPLICA_FILENAME)
        assertEquals(PERSONAL_REPLICA_FILENAME, SHARED_REPLICA_FILENAME)
    }

    @Test
    fun schemaContainsEveryDomainTableAndNoDuplicates() {
        val names = personalDataTables().map { it.name }
        assertEquals(names.toSet().size, names.size)
        assertEquals(
            setOf(
                NOTES_FOLDERS_TABLE,
                NOTES_TABLE,
                TASK_LISTS_TABLE,
                TASKS_TABLE,
                CALENDARS_TABLE,
                EVENTS_TABLE,
                ADDRESS_BOOKS_TABLE,
                CONTACTS_TABLE,
                BOOKMARKS_FOLDERS_TABLE,
                BOOKMARKS_TABLE,
            ),
            personalDataTableNames(),
        )
    }

    @Test
    fun everyTableCarriesClientOnlyUploadMetadata() {
        personalDataTables().forEach { table ->
            val columns = table.columns.map { it.name }.toSet()
            assertTrue(table.name, CLIENT_OPERATION in columns)
            assertTrue(table.name, OPERATION_ID in columns)
            assertTrue(table.name, EXPECTED_REVISION in columns)
            assertTrue(table.name, DELETED_LOCAL in columns)
            assertTrue(table.name, CLIENT_PAYLOAD in columns)
        }
    }

    @Test
    fun syncedColumnsMatchServerStreams() {
        val byName = personalDataTables().associateBy { it.name }
        assertEquals(
            setOf("user_id", "parent_id", "name", "position", "revision", "created_at", "updated_at"),
            syncedColumns(byName.getValue(NOTES_FOLDERS_TABLE)),
        )
        assertEquals(
            setOf("user_id", "folder_id", "title", "body", "revision", "created_at", "updated_at"),
            syncedColumns(byName.getValue(NOTES_TABLE)),
        )
        assertTrue(syncedColumns(byName.getValue(TASK_LISTS_TABLE)).containsAll(setOf("name", "href", "etag", "ctag", "sync_token")))
        assertTrue(syncedColumns(byName.getValue(TASKS_TABLE)).containsAll(setOf("list_id", "title", "due_local", "due_all_day", "completed")))
        assertTrue(syncedColumns(byName.getValue(EVENTS_TABLE)).containsAll(setOf("calendar_id", "summary", "dtstart", "rrule", "exdates")))
        assertTrue(syncedColumns(byName.getValue(CONTACTS_TABLE)).containsAll(setOf("address_book_id", "display_name", "emails", "phones")))
        assertTrue(syncedColumns(byName.getValue(BOOKMARKS_TABLE)).containsAll(setOf("url", "title", "tags", "favorite", "archived")))
    }

    private fun syncedColumns(table: com.powersync.db.schema.Table): Set<String> {
        val client = setOf(
            CLIENT_OPERATION,
            OPERATION_ID,
            EXPECTED_REVISION,
            EXPECTED_ETAG,
            DELETED_LOCAL,
            CLIENT_PAYLOAD,
        )
        return table.columns.map { it.name }.filterNot { it in client }.toSet()
    }
}
