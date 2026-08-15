package com.amazity.foyer.sync

import com.amazity.foyer.bookmarks.BOOKMARKS_FOLDERS_TABLE
import com.amazity.foyer.bookmarks.BOOKMARKS_TABLE
import com.amazity.foyer.bookmarks.BookmarksApi
import com.amazity.foyer.bookmarks.decodeTags
import com.amazity.foyer.calendar.CALENDARS_TABLE
import com.amazity.foyer.calendar.CalendarApi
import com.amazity.foyer.calendar.EVENTS_TABLE
import com.amazity.foyer.calendar.parseExdates
import com.amazity.foyer.contacts.ADDRESS_BOOKS_TABLE
import com.amazity.foyer.contacts.CONTACTS_TABLE
import com.amazity.foyer.contacts.ContactDraft
import com.amazity.foyer.contacts.ContactsApi
import com.amazity.foyer.contacts.parseAddresses
import com.amazity.foyer.contacts.parseEmails
import com.amazity.foyer.contacts.parsePhones
import com.amazity.foyer.model.BookmarkItem
import com.amazity.foyer.model.Contact
import com.amazity.foyer.model.EventDraft
import com.amazity.foyer.model.StructuredContactName
import com.amazity.foyer.model.TaskDue
import com.amazity.foyer.model.VaultNote
import com.amazity.foyer.model.VaultTask
import com.amazity.foyer.notes.NOTES_FOLDERS_TABLE
import com.amazity.foyer.notes.NOTES_TABLE
import com.amazity.foyer.notes.NotesApi
import com.amazity.foyer.tasks.TASKS_TABLE
import com.amazity.foyer.tasks.TASK_LISTS_TABLE
import com.amazity.foyer.tasks.TasksApi
import org.json.JSONObject

data class ReplicaCrudOp(
    val table: String,
    val id: String,
    val data: Map<String, Any?>,
)

data class PersonalDataLookups(
    val note: (String) -> VaultNote? = { null },
    val task: (String) -> VaultTask? = { null },
    val contact: (String) -> Contact? = { null },
    val bookmark: (String) -> BookmarkItem? = { null },
)

class PersonalDataDispatch(
    private val notesApi: NotesApi,
    private val tasksApi: TasksApi,
    private val calendarApi: CalendarApi,
    private val contactsApi: ContactsApi,
    private val bookmarksApi: BookmarksApi,
    private val lookups: PersonalDataLookups,
) {
    suspend fun upload(op: ReplicaCrudOp) {
        val command = op.data.text(CLIENT_OPERATION)
            ?: error("Missing durable command for ${op.table}/${op.id}")
        val operationId = op.data.text(OPERATION_ID)
            ?: error("Missing durable operation id for ${op.table}/${op.id}")
        when (op.table) {
            NOTES_FOLDERS_TABLE, NOTES_TABLE -> uploadNotes(op, command, operationId)
            TASK_LISTS_TABLE, TASKS_TABLE -> uploadTasks(op, command, operationId)
            CALENDARS_TABLE, EVENTS_TABLE -> uploadCalendar(op, command, operationId)
            ADDRESS_BOOKS_TABLE, CONTACTS_TABLE -> uploadContacts(op, command, operationId)
            BOOKMARKS_FOLDERS_TABLE, BOOKMARKS_TABLE -> uploadBookmarks(op, command, operationId)
            else -> error("Unexpected PowerSync upload table: ${op.table}")
        }
    }

    private suspend fun uploadNotes(op: ReplicaCrudOp, command: String, operationId: String) {
        val data = op.data
        when (op.table) {
            NOTES_FOLDERS_TABLE -> when (command) {
                "create" -> notesApi.createFolder(
                    id = op.id,
                    operationId = operationId,
                    name = data.requiredText("name", "notes"),
                    parentId = data.text("parent_id"),
                    position = data.int("position"),
                )
                "rename" -> notesApi.renameFolder(
                    id = op.id,
                    expectedRevision = data.requiredLong(EXPECTED_REVISION, "notes"),
                    name = data.requiredText("name", "notes"),
                    operationId = operationId,
                )
                "move" -> notesApi.moveFolder(
                    id = op.id,
                    expectedRevision = data.requiredLong(EXPECTED_REVISION, "notes"),
                    parentId = data.text("parent_id"),
                    position = data.int("position"),
                    operationId = operationId,
                )
                "delete" -> notesApi.deleteFolder(
                    id = op.id,
                    expectedRevision = data.requiredLong(EXPECTED_REVISION, "notes"),
                    operationId = operationId,
                )
                else -> error("Unknown folder command: $command")
            }
            NOTES_TABLE -> when (command) {
                "create" -> notesApi.createNote(
                    id = op.id,
                    operationId = operationId,
                    folderId = data.requiredText("folder_id", "notes"),
                    title = data.requiredText("title", "notes"),
                    body = data.text("body").orEmpty(),
                )
                "update" -> notesApi.updateNote(
                    id = op.id,
                    expectedRevision = data.requiredLong(EXPECTED_REVISION, "notes"),
                    title = data.noteField(op.id, "title", lookups),
                    body = data.noteField(op.id, "body", lookups),
                    operationId = operationId,
                )
                "move" -> notesApi.moveNote(
                    id = op.id,
                    expectedRevision = data.requiredLong(EXPECTED_REVISION, "notes"),
                    folderId = data.requiredText("folder_id", "notes"),
                    operationId = operationId,
                )
                "delete" -> notesApi.deleteNote(
                    id = op.id,
                    expectedRevision = data.requiredLong(EXPECTED_REVISION, "notes"),
                    operationId = operationId,
                )
                else -> error("Unknown note command: $command")
            }
        }
    }

    private suspend fun uploadTasks(op: ReplicaCrudOp, command: String, operationId: String) {
        val data = op.data
        when (op.table) {
            TASK_LISTS_TABLE -> when (command) {
                "create" -> tasksApi.createList(
                    id = op.id,
                    operationId = operationId,
                    name = data.requiredText("name", "tasks"),
                    position = data.int("position"),
                )
                "rename" -> tasksApi.renameList(
                    id = op.id,
                    expectedRevision = data.requiredLong(EXPECTED_REVISION, "tasks"),
                    name = data.requiredText("name", "tasks"),
                    operationId = operationId,
                )
                "delete" -> tasksApi.deleteList(
                    id = op.id,
                    expectedRevision = data.requiredLong(EXPECTED_REVISION, "tasks"),
                    operationId = operationId,
                )
                else -> error("Unknown task list command: $command")
            }
            TASKS_TABLE -> when (command) {
                "create" -> tasksApi.createTask(
                    id = op.id,
                    operationId = operationId,
                    listId = data.requiredText("list_id", "tasks"),
                    title = data.taskText(op.id, "title", lookups) { it.title },
                    description = data.taskText(op.id, "description", lookups) { it.description },
                    due = data.resolvedTaskDue(op.id, lookups),
                    priority = data.taskInt(op.id, "priority", lookups) { it.priority },
                    position = data.taskInt(op.id, "position", lookups) { it.position },
                )
                "update" -> tasksApi.updateTask(
                    id = op.id,
                    expectedRevision = data.requiredLong(EXPECTED_REVISION, "tasks"),
                    title = data.taskText(op.id, "title", lookups) { it.title },
                    description = data.taskText(op.id, "description", lookups) { it.description },
                    due = data.resolvedTaskDue(op.id, lookups),
                    priority = data.taskInt(op.id, "priority", lookups) { it.priority },
                    position = data.taskInt(op.id, "position", lookups) { it.position },
                    operationId = operationId,
                )
                "move" -> tasksApi.moveTask(
                    id = op.id,
                    expectedRevision = data.requiredLong(EXPECTED_REVISION, "tasks"),
                    listId = data.requiredText("list_id", "tasks"),
                    position = data.int("position"),
                    operationId = operationId,
                )
                "complete" -> tasksApi.completeTask(
                    id = op.id,
                    expectedRevision = data.requiredLong(EXPECTED_REVISION, "tasks"),
                    operationId = operationId,
                )
                "reopen" -> tasksApi.reopenTask(
                    id = op.id,
                    expectedRevision = data.requiredLong(EXPECTED_REVISION, "tasks"),
                    operationId = operationId,
                )
                "delete" -> tasksApi.deleteTask(
                    id = op.id,
                    expectedRevision = data.requiredLong(EXPECTED_REVISION, "tasks"),
                    operationId = operationId,
                )
                else -> error("Unknown task command: $command")
            }
        }
    }

    private suspend fun uploadCalendar(op: ReplicaCrudOp, command: String, operationId: String) {
        val data = op.data
        when (op.table) {
            CALENDARS_TABLE -> when (command) {
                "create" -> calendarApi.createCalendar(
                    id = op.id,
                    operationId = operationId,
                    displayName = data.requiredText("display_name", "calendar"),
                    description = data.text("description").orEmpty(),
                    color = data.text("color"),
                )
                "rename" -> calendarApi.renameCalendar(
                    id = op.id,
                    expectedRevision = data.requiredLong(EXPECTED_REVISION, "calendar"),
                    expectedEtag = data.text(EXPECTED_ETAG),
                    displayName = data.requiredText("display_name", "calendar"),
                    operationId = operationId,
                )
                "delete" -> calendarApi.deleteCalendar(
                    id = op.id,
                    expectedRevision = data.requiredLong(EXPECTED_REVISION, "calendar"),
                    expectedEtag = data.text(EXPECTED_ETAG),
                    operationId = operationId,
                )
                else -> error("Unknown calendar command: $command")
            }
            EVENTS_TABLE -> when (command) {
                "create" -> calendarApi.createEvent(
                    id = op.id,
                    operationId = operationId,
                    uid = data.text("uid"),
                    draft = data.resolvedEventDraft(),
                )
                "update" -> calendarApi.updateEvent(
                    id = op.id,
                    expectedRevision = data.requiredLong(EXPECTED_REVISION, "calendar"),
                    expectedEtag = data.text(EXPECTED_ETAG),
                    draft = data.resolvedEventDraft(),
                    operationId = operationId,
                )
                "move" -> calendarApi.moveEvent(
                    id = op.id,
                    expectedRevision = data.requiredLong(EXPECTED_REVISION, "calendar"),
                    expectedEtag = data.text(EXPECTED_ETAG),
                    calendarId = data.requiredText("calendar_id", "calendar"),
                    operationId = operationId,
                )
                "delete" -> calendarApi.deleteEvent(
                    id = op.id,
                    expectedRevision = data.requiredLong(EXPECTED_REVISION, "calendar"),
                    expectedEtag = data.text(EXPECTED_ETAG),
                    operationId = operationId,
                )
                else -> error("Unknown event command: $command")
            }
        }
    }

    private suspend fun uploadContacts(op: ReplicaCrudOp, command: String, operationId: String) {
        val data = op.data
        when (op.table) {
            ADDRESS_BOOKS_TABLE -> when (command) {
                "create" -> contactsApi.createAddressBook(
                    id = op.id,
                    operationId = operationId,
                    displayName = data.requiredText("display_name", "contacts"),
                    description = data.text("description"),
                )
                "update" -> contactsApi.updateAddressBook(
                    id = op.id,
                    expectedEtag = data.text(EXPECTED_ETAG),
                    expectedRevision = data.requiredLong(EXPECTED_REVISION, "contacts"),
                    displayName = data.requiredText("display_name", "contacts"),
                    operationId = operationId,
                )
                "delete" -> contactsApi.deleteAddressBook(
                    id = op.id,
                    expectedEtag = data.text(EXPECTED_ETAG),
                    expectedRevision = data.requiredLong(EXPECTED_REVISION, "contacts"),
                    operationId = operationId,
                )
                else -> error("Unknown address book command: $command")
            }
            CONTACTS_TABLE -> when (command) {
                "create" -> contactsApi.createContact(
                    id = op.id,
                    operationId = operationId,
                    addressBookId = data.requiredText("address_book_id", "contacts"),
                    draft = data.resolvedContactDraft(op.id, lookups),
                )
                "update" -> contactsApi.updateContact(
                    id = op.id,
                    expectedEtag = data.text(EXPECTED_ETAG),
                    expectedRevision = data.requiredLong(EXPECTED_REVISION, "contacts"),
                    draft = data.resolvedContactDraft(op.id, lookups),
                    operationId = operationId,
                )
                "move" -> contactsApi.moveContact(
                    id = op.id,
                    expectedEtag = data.text(EXPECTED_ETAG),
                    expectedRevision = data.requiredLong(EXPECTED_REVISION, "contacts"),
                    addressBookId = data.requiredText("address_book_id", "contacts"),
                    operationId = operationId,
                )
                "delete" -> contactsApi.deleteContact(
                    id = op.id,
                    expectedEtag = data.text(EXPECTED_ETAG),
                    expectedRevision = data.requiredLong(EXPECTED_REVISION, "contacts"),
                    operationId = operationId,
                )
                else -> error("Unknown contact command: $command")
            }
        }
    }

    private suspend fun uploadBookmarks(op: ReplicaCrudOp, command: String, operationId: String) {
        val data = op.data
        when (op.table) {
            BOOKMARKS_FOLDERS_TABLE -> when (command) {
                "create" -> bookmarksApi.createFolder(
                    id = op.id,
                    operationId = operationId,
                    name = data.requiredText("name", "bookmarks"),
                    parentId = data.text("parent_id"),
                    position = data.int("position"),
                )
                "rename" -> bookmarksApi.renameFolder(
                    id = op.id,
                    expectedRevision = data.requiredLong(EXPECTED_REVISION, "bookmarks"),
                    name = data.requiredText("name", "bookmarks"),
                    operationId = operationId,
                )
                "move" -> bookmarksApi.moveFolder(
                    id = op.id,
                    expectedRevision = data.requiredLong(EXPECTED_REVISION, "bookmarks"),
                    parentId = data.text("parent_id"),
                    position = data.int("position"),
                    operationId = operationId,
                )
                "delete" -> bookmarksApi.deleteFolder(
                    id = op.id,
                    expectedRevision = data.requiredLong(EXPECTED_REVISION, "bookmarks"),
                    operationId = operationId,
                )
                else -> error("Unknown folder command: $command")
            }
            BOOKMARKS_TABLE -> when (command) {
                "create" -> bookmarksApi.createBookmark(
                    id = op.id,
                    operationId = operationId,
                    folderId = data.requiredText("folder_id", "bookmarks"),
                    url = data.requiredText("url", "bookmarks"),
                    title = data.requiredText("title", "bookmarks"),
                    description = data.text("description").orEmpty(),
                    tags = decodeTags(data.text("tags")),
                    favorite = data.flag("favorite"),
                    archived = data.flag("archived"),
                    position = data.int("position"),
                )
                "update" -> bookmarksApi.updateBookmark(
                    id = op.id,
                    expectedRevision = data.requiredLong(EXPECTED_REVISION, "bookmarks"),
                    url = data.bookmarkField(op.id, "url", lookups),
                    title = data.bookmarkField(op.id, "title", lookups),
                    description = data.bookmarkField(op.id, "description", lookups),
                    tags = decodeTags(
                        data.payloadObject()?.opt("tags")?.toString() ?: data.text("tags"),
                    ),
                    operationId = operationId,
                )
                "move" -> bookmarksApi.moveBookmark(
                    id = op.id,
                    expectedRevision = data.requiredLong(EXPECTED_REVISION, "bookmarks"),
                    folderId = data.requiredText("folder_id", "bookmarks"),
                    position = data.int("position"),
                    operationId = operationId,
                )
                "favorite" -> bookmarksApi.favoriteBookmark(
                    id = op.id,
                    expectedRevision = data.requiredLong(EXPECTED_REVISION, "bookmarks"),
                    favorite = data.resolvedBookmarkFlag("favorite"),
                    operationId = operationId,
                )
                "archive" -> bookmarksApi.archiveBookmark(
                    id = op.id,
                    expectedRevision = data.requiredLong(EXPECTED_REVISION, "bookmarks"),
                    archived = data.resolvedBookmarkFlag("archived"),
                    operationId = operationId,
                )
                "delete" -> bookmarksApi.deleteBookmark(
                    id = op.id,
                    expectedRevision = data.requiredLong(EXPECTED_REVISION, "bookmarks"),
                    operationId = operationId,
                )
                else -> error("Unknown bookmark command: $command")
            }
        }
    }

}

internal fun Map<String, *>.noteField(noteId: String, field: String, lookups: PersonalDataLookups): String {
    payloadObject()?.takeIf { it.has(field) }?.let { return it.getString(field) }
    text(field)?.let { return it }
    val note = lookups.note(noteId)
    return when (field) {
        "title" -> note?.title
        "body" -> note?.body
        else -> null
    } ?: error("Missing durable note update field: $field")
}

internal fun Map<String, *>.taskText(
    taskId: String,
    field: String,
    lookups: PersonalDataLookups,
    fromTask: (VaultTask) -> String,
): String {
    payloadObject()?.takeIf { it.has(field) }?.let { return it.getString(field) }
    text(field)?.let { return it }
    return lookups.task(taskId)?.let(fromTask).orEmpty()
}

internal fun Map<String, *>.taskInt(
    taskId: String,
    field: String,
    lookups: PersonalDataLookups,
    fromTask: (VaultTask) -> Int,
): Int {
    payloadObject()?.takeIf { it.has(field) }?.let { return it.optInt(field) }
    int(field)?.let { return it }
    return lookups.task(taskId)?.let(fromTask) ?: 0
}

internal fun Map<String, *>.resolvedTaskDue(taskId: String, lookups: PersonalDataLookups): TaskDue? {
    payloadObject()?.let { payload ->
        payload.optJSONObject("due")?.let { due ->
            return TaskDue.parse(
                due.optString("local"),
                due.optionalText("timeZone"),
                due.optBoolean("allDay"),
            )?.copy(at = due.optionalText("at"))
        }
        if (payload.has("due") && payload.isNull("due")) return null
    }
    return lookups.task(taskId)?.due
}

internal fun Map<String, *>.resolvedEventDraft(): EventDraft {
    payloadObject()?.let { payload ->
        return EventDraft(
            summary = payload.optString("summary"),
            description = payload.optString("description"),
            location = payload.optString("location"),
            allDay = payload.optBoolean("allDay"),
            dtstart = payload.optString("dtstart"),
            dtend = payload.optionalText("dtend"),
            tzid = payload.optionalText("tzid"),
            rrule = payload.optionalText("rrule"),
            exdates = parseExdates(payload.optString("exdates", "[]")),
            calendarId = payload.optString("calendarId"),
        )
    }
    return EventDraft(
        summary = requiredText("summary", "calendar"),
        description = text("description").orEmpty(),
        location = text("location").orEmpty(),
        allDay = (this["all_day"] as? Number)?.toInt() == 1,
        dtstart = requiredText("dtstart", "calendar"),
        dtend = text("dtend"),
        tzid = text("tzid"),
        rrule = text("rrule"),
        exdates = parseExdates(text("exdates") ?: "[]"),
        calendarId = requiredText("calendar_id", "calendar"),
    )
}

internal fun Map<String, *>.resolvedContactDraft(contactId: String, lookups: PersonalDataLookups): ContactDraft {
    payloadObject()?.let { payload ->
        return ContactDraft(
            displayName = payload.optString("displayName"),
            name = payload.optJSONObject("name")?.let {
                StructuredContactName(
                    familyName = it.optString("familyName"),
                    givenName = it.optString("givenName"),
                    additionalNames = it.optString("additionalNames"),
                    honorificPrefix = it.optString("honorificPrefix"),
                    honorificSuffix = it.optString("honorificSuffix"),
                )
            } ?: StructuredContactName(),
            emails = parseEmails(payload.optJSONArray("emails")?.toString()),
            phones = parsePhones(payload.optJSONArray("phones")?.toString()),
            organization = payload.optString("organization"),
            jobTitle = payload.optString("jobTitle"),
            addresses = parseAddresses(payload.optJSONArray("addresses")?.toString()),
            birthday = payload.optionalText("birthday"),
            notes = payload.optString("notes"),
            addressBookId = text("address_book_id")
                ?: lookups.contact(contactId)?.addressBookId.orEmpty(),
        )
    }
    val existing = lookups.contact(contactId)
    return ContactDraft(
        displayName = text("display_name") ?: existing?.displayName.orEmpty(),
        name = StructuredContactName(
            familyName = text("family_name") ?: existing?.name?.familyName.orEmpty(),
            givenName = text("given_name") ?: existing?.name?.givenName.orEmpty(),
            additionalNames = text("additional_names") ?: existing?.name?.additionalNames.orEmpty(),
            honorificPrefix = text("honorific_prefix") ?: existing?.name?.honorificPrefix.orEmpty(),
            honorificSuffix = text("honorific_suffix") ?: existing?.name?.honorificSuffix.orEmpty(),
        ),
        emails = parseEmails(text("emails")).ifEmpty { existing?.emails.orEmpty() },
        phones = parsePhones(text("phones")).ifEmpty { existing?.phones.orEmpty() },
        organization = text("organization") ?: existing?.organization.orEmpty(),
        jobTitle = text("job_title") ?: existing?.jobTitle.orEmpty(),
        addresses = parseAddresses(text("addresses")).ifEmpty { existing?.addresses.orEmpty() },
        birthday = text("birthday") ?: existing?.birthday,
        notes = text("notes") ?: existing?.notes.orEmpty(),
        addressBookId = text("address_book_id") ?: existing?.addressBookId.orEmpty(),
    )
}

internal fun Map<String, *>.bookmarkField(
    bookmarkId: String,
    field: String,
    lookups: PersonalDataLookups,
): String {
    payloadObject()?.takeIf { it.has(field) && !it.isNull(field) }?.let { return it.getString(field) }
    text(field)?.let { return it }
    val bookmark = lookups.bookmark(bookmarkId)
    return when (field) {
        "url" -> bookmark?.url
        "title" -> bookmark?.title
        "description" -> bookmark?.description
        else -> null
    } ?: error("Missing durable bookmark update field: $field")
}

internal fun Map<String, *>.resolvedBookmarkFlag(field: String): Boolean =
    payloadObject()?.takeIf { it.has(field) }?.optBoolean(field) ?: flag(field)

fun tableDomain(table: String): String = when (table) {
    NOTES_FOLDERS_TABLE, NOTES_TABLE -> "notes"
    TASK_LISTS_TABLE, TASKS_TABLE -> "tasks"
    CALENDARS_TABLE, EVENTS_TABLE -> "calendar"
    ADDRESS_BOOKS_TABLE, CONTACTS_TABLE -> "contacts"
    BOOKMARKS_FOLDERS_TABLE, BOOKMARKS_TABLE -> "bookmarks"
    else -> "personal"
}
