package com.amazity.foyer.sync

import android.content.Context
import com.amazity.foyer.bookmarks.BookmarksApi
import com.amazity.foyer.bookmarks.BookmarksStore
import com.amazity.foyer.calendar.CalendarApi
import com.amazity.foyer.calendar.CalendarStore
import com.amazity.foyer.contacts.ContactsApi
import com.amazity.foyer.contacts.ContactsStore
import com.amazity.foyer.auth.foyerApiClient
import com.amazity.foyer.network.FoyerApiClient
import com.amazity.foyer.notes.NotesApi
import com.amazity.foyer.notes.NotesStore
import com.amazity.foyer.tasks.TasksApi
import com.amazity.foyer.tasks.TasksStore
import com.powersync.DatabaseDriverFactory
import com.powersync.PowerSyncDatabase
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.launch

/**
 * Owns the single personal-data PowerSync database and connector. Domain stores
 * attach to [database] and must not disconnect it independently.
 */
class PersonalDataReplica(
    context: Context,
    apiClient: FoyerApiClient = foyerApiClient(context),
) {
    private val appContext = context.applicationContext
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val ready = CompletableDeferred<PowerSyncDatabase>()
    private val _conflicts = MutableStateFlow<Map<String, PersonalDataConflict>>(emptyMap())
    private var database: PowerSyncDatabase? = null
    private var startRequested = false

    val notesApi = NotesApi(apiClient)
    val tasksApi = TasksApi(apiClient)
    val calendarApi = CalendarApi(apiClient)
    val contactsApi = ContactsApi(apiClient)
    val bookmarksApi = BookmarksApi(apiClient)

    val notes = NotesStore.attach(appContext, notesApi) { requireDatabase() }
    val tasks = TasksStore.attach(appContext, tasksApi) { requireDatabase() }
    val calendar = CalendarStore.attach(appContext, calendarApi) { requireDatabase() }
    val contacts = ContactsStore.attach(appContext, contactsApi) { requireDatabase() }
    val bookmarks = BookmarksStore.attach(appContext, bookmarksApi) { requireDatabase() }

    val conflicts: StateFlow<Map<String, PersonalDataConflict>> = _conflicts

    @Synchronized
    fun start() {
        if (startRequested) return
        startRequested = true
        scope.launch { initialize() }
        notes.start()
        tasks.start()
        calendar.start()
        contacts.start()
        bookmarks.start()
    }

    fun stop() {
        notes.stop()
        tasks.stop()
        calendar.stop()
        contacts.stop()
        bookmarks.stop()
        scope.launch { runCatching { database?.disconnect() } }
    }

    fun conflict(domain: String): PersonalDataConflict? = _conflicts.value[domain]

    fun reportConflict(conflict: PersonalDataConflict) {
        _conflicts.value = _conflicts.value + (conflict.domain to conflict)
        when (conflict.domain) {
            "notes" -> notes.reportConflict(conflict.code, conflict.publicMessage)
            "tasks" -> tasks.reportConflict(conflict.code, conflict.publicMessage)
            "calendar" -> calendar.reportConflict(conflict.code, conflict.publicMessage)
            "contacts" -> contacts.reportConflict(conflict.code, conflict.publicMessage)
            "bookmarks" -> bookmarks.reportConflict(conflict.code, conflict.publicMessage)
        }
    }

    fun clearConflict(domain: String) {
        _conflicts.value = _conflicts.value - domain
    }

    private suspend fun initialize() {
        try {
            val db = PowerSyncDatabase(
                factory = DatabaseDriverFactory(appContext),
                schema = personalDataSchema(),
                dbFilename = PERSONAL_REPLICA_FILENAME,
            )
            database = db
            ready.complete(db)
            db.connect(
                PersonalDataConnector(
                    notesApi = notesApi,
                    tasksApi = tasksApi,
                    calendarApi = calendarApi,
                    contactsApi = contactsApi,
                    bookmarksApi = bookmarksApi,
                    lookups = PersonalDataLookups(
                        note = { notes.catalog.value.note(it) },
                        task = { tasks.catalog.value.task(it) },
                        contact = { contacts.catalog.value.contact(it) },
                        bookmark = { bookmarks.catalog.value.bookmark(it) },
                    ),
                    onConflict = { reportConflict(it) },
                ),
            )
        } catch (error: Throwable) {
            if (!ready.isCompleted) ready.completeExceptionally(error)
            val message = "PowerSync replica unavailable: ${error.message}"
            notes.markUnavailable(message)
            tasks.markUnavailable(message)
            calendar.markUnavailable(message)
            contacts.markUnavailable(message)
            bookmarks.markUnavailable(message)
        }
    }

    private suspend fun requireDatabase(): PowerSyncDatabase {
        start()
        return ready.await()
    }
}
