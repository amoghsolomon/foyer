package com.amazity.foyer.sync

import com.amazity.foyer.BuildConfig
import com.amazity.foyer.bookmarks.BookmarksApi
import com.amazity.foyer.bookmarks.BookmarksConflictException
import com.amazity.foyer.calendar.CalendarApi
import com.amazity.foyer.calendar.CalendarConflictException
import com.amazity.foyer.contacts.ContactsApi
import com.amazity.foyer.contacts.ContactsConflictException
import com.amazity.foyer.notes.NotesApi
import com.amazity.foyer.notes.NotesConflictException
import com.amazity.foyer.tasks.TasksApi
import com.amazity.foyer.tasks.TasksConflictException
import com.powersync.PowerSyncDatabase
import com.powersync.connectors.PowerSyncBackendConnector
import com.powersync.connectors.PowerSyncCredentials
import com.powersync.db.crud.CrudEntry

class PersonalDataConflict(
    val domain: String,
    val code: String,
    val publicMessage: String,
) : IllegalStateException(publicMessage)

class PersonalDataConnector(
    private val notesApi: NotesApi,
    private val tasksApi: TasksApi,
    private val calendarApi: CalendarApi,
    private val contactsApi: ContactsApi,
    private val bookmarksApi: BookmarksApi,
    private val lookups: PersonalDataLookups,
    private val onConflict: (PersonalDataConflict) -> Unit,
) : PowerSyncBackendConnector() {
    private val dispatch = PersonalDataDispatch(
        notesApi = notesApi,
        tasksApi = tasksApi,
        calendarApi = calendarApi,
        contactsApi = contactsApi,
        bookmarksApi = bookmarksApi,
        lookups = lookups,
    )

    override suspend fun fetchCredentials(): PowerSyncCredentials {
        val credentials = notesApi.syncCredentials()
        return PowerSyncCredentials(
            endpoint = BuildConfig.FOYER_POWERSYNC_URL.ifBlank { credentials.endpoint },
            token = credentials.token,
            userId = credentials.userId,
        )
    }

    override suspend fun uploadData(database: PowerSyncDatabase) {
        val transaction = database.getNextCrudTransaction() ?: return
        try {
            transaction.crud.forEach { entry -> dispatch.upload(entry.toReplicaOp()) }
            transaction.complete(null)
        } catch (conflict: Throwable) {
            val mapped = mapConflict(conflict) ?: throw conflict
            onConflict(mapped)
            transaction.complete(null)
        }
    }
}

internal fun CrudEntry.toReplicaOp(): ReplicaCrudOp = ReplicaCrudOp(
    table = table,
    id = id,
    data = opData?.typed.orEmpty(),
)

internal fun mapConflict(error: Throwable): PersonalDataConflict? = when (error) {
    is NotesConflictException -> PersonalDataConflict("notes", error.code, error.publicMessage())
    is TasksConflictException -> PersonalDataConflict("tasks", error.code, error.publicMessage())
    is CalendarConflictException -> PersonalDataConflict("calendar", error.code, error.publicMessage())
    is ContactsConflictException -> PersonalDataConflict("contacts", error.code, error.publicMessage())
    is BookmarksConflictException -> PersonalDataConflict("bookmarks", error.code, error.publicMessage())
    is PersonalDataConflict -> error
    else -> null
}
