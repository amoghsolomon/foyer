package com.amazity.foyer.sync

import com.amazity.foyer.bookmarks.BookmarksConflictException
import com.amazity.foyer.calendar.CalendarConflictException
import com.amazity.foyer.contacts.ContactsConflictException
import com.amazity.foyer.model.BookmarkItem
import com.amazity.foyer.model.TaskDue
import com.amazity.foyer.model.VaultNote
import com.amazity.foyer.model.VaultTask
import com.amazity.foyer.notes.NOTES_TABLE
import com.amazity.foyer.notes.NotesConflictException
import com.amazity.foyer.tasks.TASKS_TABLE
import com.amazity.foyer.tasks.TasksConflictException
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class PersonalDataDispatchTest {
    @Test
    fun tableDomainRoutesEachReplicaTable() {
        assertEquals("notes", tableDomain("notes"))
        assertEquals("notes", tableDomain("notes_folders"))
        assertEquals("tasks", tableDomain("tasks"))
        assertEquals("tasks", tableDomain("task_lists"))
        assertEquals("calendar", tableDomain("calendar_events"))
        assertEquals("contacts", tableDomain("contacts"))
        assertEquals("bookmarks", tableDomain("bookmarks"))
        assertEquals("personal", tableDomain("unknown"))
    }

    @Test
    fun notePartialPatchPrefersDurableClientPayload() {
        val data = mapOf(
            CLIENT_OPERATION to "update",
            OPERATION_ID to "op-1",
            EXPECTED_REVISION to 3L,
            CLIENT_PAYLOAD to """{"operationId":"op-1","title":"Kept title","body":"body-only\n"}""",
        )
        val lookups = PersonalDataLookups(
            note = {
                VaultNote(
                    id = "n1",
                    folderId = "inbox",
                    title = "stale title",
                    summary = "stale",
                    updatedLabel = "",
                    body = "stale body",
                )
            },
        )
        assertEquals("Kept title", data.noteField("n1", "title", lookups))
        assertEquals("body-only\n", data.noteField("n1", "body", lookups))
    }

    @Test
    fun notePartialPatchDoesNotInventOmittedBodyFromCrudColumns() {
        val data = mapOf(
            CLIENT_OPERATION to "update",
            OPERATION_ID to "op-2",
            EXPECTED_REVISION to 4L,
            CLIENT_PAYLOAD to """{"operationId":"op-2","title":"Title only","body":""}""",
        )
        assertEquals("", data.noteField("n1", "body", PersonalDataLookups()))
    }

    @Test
    fun taskDueNullInPayloadClearsTheDueDate() {
        val data = mapOf(
            CLIENT_OPERATION to "update",
            OPERATION_ID to "op-3",
            CLIENT_PAYLOAD to """{"operationId":"op-3","title":"Task","description":"","due":null,"priority":0,"position":0}""",
        )
        val lookups = PersonalDataLookups(
            task = {
                VaultTask(
                    id = "t1",
                    listId = "inbox",
                    title = "Task",
                    description = "",
                    due = TaskDue.dateOnly("2026-08-15"),
                )
            },
        )
        assertNull(data.resolvedTaskDue("t1", lookups))
    }

    @Test
    fun taskDueUsesPayloadWhenCrudOmitsDueColumns() {
        val data = mapOf(
            CLIENT_OPERATION to "update",
            OPERATION_ID to "op-4",
            CLIENT_PAYLOAD to """{"operationId":"op-4","title":"Task","description":"notes","due":{"local":"2026-08-20T18:00:00","timeZone":"America/Chicago","allDay":false},"priority":1,"position":2}""",
        )
        val due = data.resolvedTaskDue("t1", PersonalDataLookups())
        assertEquals("2026-08-20T18:00:00", due?.local)
        assertEquals("America/Chicago", due?.timeZone)
        assertEquals(false, due?.allDay)
    }

    @Test
    fun bookmarkFavoriteUsesPayloadWhenCrudIsPartial() {
        val data = mapOf(
            CLIENT_OPERATION to "favorite",
            OPERATION_ID to "op-5",
            EXPECTED_REVISION to 2L,
            CLIENT_PAYLOAD to """{"operationId":"op-5","favorite":true}""",
        )
        assertEquals(true, data.resolvedBookmarkFlag("favorite"))
    }

    @Test
    fun eventDraftReadsCompletePayloadForPartialPatch() {
        val data = mapOf(
            CLIENT_OPERATION to "update",
            OPERATION_ID to "op-6",
            CLIENT_PAYLOAD to """{"operationId":"op-6","calendarId":"cal-1","summary":"Standup","description":"weekly","location":"HQ","allDay":false,"dtstart":"20260316T100000","dtend":"20260316T103000","tzid":"America/New_York","rrule":"FREQ=WEEKLY;BYDAY=MO","exdates":"[]"}""",
        )
        val draft = data.resolvedEventDraft()
        assertEquals("Standup", draft.summary)
        assertEquals("weekly", draft.description)
        assertEquals("cal-1", draft.calendarId)
        assertEquals("FREQ=WEEKLY;BYDAY=MO", draft.rrule)
        assertEquals("20260316T100000", draft.dtstart)
    }

    @Test
    fun contactDraftPreservesStructuredFieldsFromPayload() {
        val data = mapOf(
            CLIENT_OPERATION to "update",
            OPERATION_ID to "op-7",
            "address_book_id" to "book-1",
            CLIENT_PAYLOAD to """{"operationId":"op-7","displayName":"Ada Lovelace","name":{"givenName":"Ada","familyName":"Lovelace","additionalNames":"","honorificPrefix":"","honorificSuffix":""},"emails":[{"value":"ada@example.com","type":"work","pref":true}],"phones":[],"organization":"Analytical","jobTitle":"Countess","addresses":[],"birthday":null,"notes":"First programmer"}""",
        )
        val draft = data.resolvedContactDraft("c1", PersonalDataLookups())
        assertEquals("Ada Lovelace", draft.displayName)
        assertEquals("Ada", draft.name.givenName)
        assertEquals("ada@example.com", draft.emails.single().value)
        assertEquals("Analytical", draft.organization)
        assertEquals("First programmer", draft.notes)
        assertEquals("book-1", draft.addressBookId)
    }

    @Test
    fun bookmarkUpdateFallsBackToCatalogOnlyWhenPayloadOmitsField() {
        val data = mapOf(
            CLIENT_OPERATION to "update",
            OPERATION_ID to "op-8",
            CLIENT_PAYLOAD to """{"operationId":"op-8","title":"Changed"}""",
        )
        val lookups = PersonalDataLookups(
            bookmark = {
                BookmarkItem(
                    id = "b1",
                    folderId = "inbox",
                    url = "https://example.com",
                    title = "Old",
                    description = "kept",
                )
            },
        )
        assertEquals("Changed", data.bookmarkField("b1", "title", lookups))
        assertEquals("https://example.com", data.bookmarkField("b1", "url", lookups))
        assertEquals("kept", data.bookmarkField("b1", "description", lookups))
    }

    @Test
    fun mapConflictSurfacesDomainSpecificMessages() {
        val notes = mapConflict(NotesConflictException("stale_revision", "rev"))
        assertEquals("notes", notes?.domain)
        assertTrue(notes?.publicMessage.orEmpty().contains("server copy"))

        val tasks = mapConflict(TasksConflictException("gone", "gone"))
        assertEquals("tasks", tasks?.domain)

        val calendar = mapConflict(CalendarConflictException("stale_etag", "etag"))
        assertEquals("calendar", calendar?.domain)
        assertTrue(calendar?.publicMessage.orEmpty().contains("Someone else"))

        val contacts = mapConflict(ContactsConflictException("address_book_not_empty", "kids"))
        assertEquals("contacts", contacts?.domain)

        val bookmarks = mapConflict(BookmarksConflictException("cycle", "loop"))
        assertEquals("bookmarks", bookmarks?.domain)

        assertNull(mapConflict(IllegalStateException("network down")))
    }

    @Test
    fun replicaOpKeepsTableAndIdForConnectorDispatch() {
        val op = ReplicaCrudOp(
            table = NOTES_TABLE,
            id = "note-1",
            data = mapOf(CLIENT_OPERATION to "update", OPERATION_ID to "op"),
        )
        assertEquals(NOTES_TABLE, op.table)
        assertEquals("note-1", op.id)
        assertEquals("tasks", tableDomain(TASKS_TABLE))
    }
}
