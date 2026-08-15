package com.amazity.foyer.calendar

import android.content.Context
import com.amazity.foyer.BuildConfig
import com.amazity.foyer.model.CalendarCatalog
import com.amazity.foyer.model.CalendarStatus
import com.amazity.foyer.model.EventDraft
import com.amazity.foyer.model.FoyerCalendar
import com.amazity.foyer.model.FoyerEvent
import com.powersync.PowerSyncDatabase
import com.powersync.db.getLongOptional
import com.powersync.db.getString
import com.powersync.db.getStringOptional
import java.time.Instant
import java.util.UUID
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import org.json.JSONObject

/**
 * PowerSync replica adapter. Reads are immutable [CalendarCatalog] snapshots.
 * Uploads go through the shared personal-data connector, never to Radicale.
 */
class CalendarStore(
    @Suppress("UNUSED_PARAMETER") context: Context,
    private val databaseProvider: suspend () -> PowerSyncDatabase,
) {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val _catalog = MutableStateFlow(CalendarCatalog(emptyList(), emptyList()))
    private var watchJob: Job? = null
    private var statusJob: Job? = null
    private var startRequested = false
    @Volatile private var lastConflict: Pair<String, String>? = null
    @Volatile private var attachedDatabase: PowerSyncDatabase? = null

    val catalog: StateFlow<CalendarCatalog> = _catalog
    val sharingReplica: Boolean = true

    @Suppress("UNUSED_PARAMETER")
    constructor(context: Context, api: CalendarApi, databaseProvider: suspend () -> PowerSyncDatabase) : this(
        context,
        databaseProvider,
    )

    @Synchronized
    fun start() {
        if (startRequested) return
        startRequested = true
        scope.launch { initialize() }
    }

    fun stop() {
        watchJob?.cancel()
        statusJob?.cancel()
        startRequested = false
        attachedDatabase = null
    }

    fun reportConflict(code: String, message: String) {
        lastConflict = code to message
        attachedDatabase?.let { publishStatus(it) }
    }

    fun markUnavailable(message: String) {
        _catalog.update {
            it.copy(
                status = it.status.copy(
                    loading = false,
                    connected = false,
                    offline = true,
                    lastError = message,
                    usingPowerSync = false,
                ),
            )
        }
    }

    suspend fun createCalendar(displayName: String, description: String = "", color: String? = null): FoyerCalendar {
        val name = displayName.trim().also { require(it.isNotEmpty()) { "Calendar name is required" } }
        val id = UUID.randomUUID().toString()
        val now = Instant.now().toString()
        requireDatabase().execute(
            """INSERT INTO $CALENDARS_TABLE
                (id, user_id, uid, href, etag, display_name, description, color, ctag, sync_token,
                 revision, created_at, updated_at, client_operation, operation_id, expected_revision,
                 expected_etag, deleted_local)
                VALUES (?, '', ?, '', '', ?, ?, ?, NULL, NULL, 1, ?, ?, 'create', ?, NULL, NULL, 0)""",
            listOf(id, id, name, description, color, now, now, UUID.randomUUID().toString()),
        )
        return FoyerCalendar(id = id, uid = id, href = "", etag = "", displayName = name, description = description, color = color)
    }

    suspend fun renameCalendar(calendar: FoyerCalendar, displayName: String): FoyerCalendar {
        val name = displayName.trim().also { require(it.isNotEmpty()) { "Calendar name is required" } }
        val next = calendar.revision + 1
        requireDatabase().execute(
            """UPDATE $CALENDARS_TABLE
                SET display_name = ?, revision = ?, updated_at = ?, client_operation = 'rename',
                    operation_id = ?, expected_revision = ?, expected_etag = ?
                WHERE id = ?""",
            listOf(name, next, Instant.now().toString(), UUID.randomUUID().toString(), calendar.revision, calendar.etag, calendar.id),
        )
        return calendar.copy(displayName = name, revision = next)
    }

    suspend fun deleteCalendar(calendar: FoyerCalendar) {
        _catalog.value.validateCalendarDelete(calendar)?.let { error(it) }
        requireDatabase().execute(
            """UPDATE $CALENDARS_TABLE
                SET deleted_local = 1, revision = ?, updated_at = ?, client_operation = 'delete',
                    operation_id = ?, expected_revision = ?, expected_etag = ?
                WHERE id = ?""",
            listOf(
                calendar.revision + 1,
                Instant.now().toString(),
                UUID.randomUUID().toString(),
                calendar.revision,
                calendar.etag,
                calendar.id,
            ),
        )
    }

    suspend fun createEvent(draft: EventDraft): FoyerEvent {
        _catalog.value.validateEventDraft(draft)?.let { error(it) }
        val id = UUID.randomUUID().toString()
        val now = Instant.now().toString()
        val operationId = UUID.randomUUID().toString()
        val payload = draftPayload(operationId, draft)
        requireDatabase().execute(
            """INSERT INTO $EVENTS_TABLE
                (id, user_id, calendar_id, uid, href, etag, summary, description, location, all_day,
                 dtstart, dtend, tzid, rrule, exdates, revision, created_at, updated_at,
                 client_operation, operation_id, expected_revision, expected_etag, deleted_local, client_payload)
                VALUES (?, '', ?, ?, '', '', ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?, ?, 'create', ?, NULL, NULL, 0, ?)""",
            listOf(
                id,
                draft.calendarId,
                id,
                draft.summary.trim(),
                draft.description,
                draft.location,
                if (draft.allDay) 1 else 0,
                draft.dtstart,
                draft.dtend,
                draft.tzid,
                draft.rrule,
                encodeExdates(draft.exdates),
                now,
                now,
                operationId,
                payload,
            ),
        )
        return FoyerEvent(
            id = id,
            calendarId = draft.calendarId,
            uid = id,
            href = "",
            etag = "",
            summary = draft.summary.trim(),
            description = draft.description,
            location = draft.location,
            allDay = draft.allDay,
            dtstart = draft.dtstart,
            dtend = draft.dtend,
            tzid = draft.tzid,
            rrule = draft.rrule,
            exdates = encodeExdates(draft.exdates),
        )
    }

    suspend fun updateEvent(event: FoyerEvent, draft: EventDraft): FoyerEvent {
        _catalog.value.validateEventDraft(draft)?.let { error(it) }
        val next = event.revision + 1
        val operationId = UUID.randomUUID().toString()
        requireDatabase().execute(
            """UPDATE $EVENTS_TABLE
                SET summary = ?, description = ?, location = ?, all_day = ?, dtstart = ?, dtend = ?,
                    tzid = ?, rrule = ?, exdates = ?, revision = ?, updated_at = ?,
                    client_operation = 'update', operation_id = ?, expected_revision = ?,
                    expected_etag = ?, client_payload = ?
                WHERE id = ?""",
            listOf(
                draft.summary.trim(),
                draft.description,
                draft.location,
                if (draft.allDay) 1 else 0,
                draft.dtstart,
                draft.dtend,
                draft.tzid,
                draft.rrule,
                encodeExdates(draft.exdates),
                next,
                Instant.now().toString(),
                operationId,
                event.revision,
                event.etag,
                draftPayload(operationId, draft),
                event.id,
            ),
        )
        var revision = next
        if (draft.calendarId != event.calendarId) {
            requireDatabase().execute(
                """UPDATE $EVENTS_TABLE
                    SET calendar_id = ?, revision = ?, updated_at = ?, client_operation = 'move',
                        operation_id = ?, expected_revision = ?, expected_etag = ?
                    WHERE id = ?""",
                listOf(
                    draft.calendarId,
                    revision + 1,
                    Instant.now().toString(),
                    UUID.randomUUID().toString(),
                    revision,
                    event.etag,
                    event.id,
                ),
            )
            revision += 1
        }
        return event.copy(
            calendarId = draft.calendarId,
            summary = draft.summary.trim(),
            description = draft.description,
            location = draft.location,
            allDay = draft.allDay,
            dtstart = draft.dtstart,
            dtend = draft.dtend,
            tzid = draft.tzid,
            rrule = draft.rrule,
            exdates = encodeExdates(draft.exdates),
            revision = revision,
        )
    }

    suspend fun deleteEvent(event: FoyerEvent) {
        requireDatabase().execute(
            """UPDATE $EVENTS_TABLE
                SET deleted_local = 1, revision = ?, updated_at = ?, client_operation = 'delete',
                    operation_id = ?, expected_revision = ?, expected_etag = ?
                WHERE id = ?""",
            listOf(
                event.revision + 1,
                Instant.now().toString(),
                UUID.randomUUID().toString(),
                event.revision,
                event.etag,
                event.id,
            ),
        )
    }

    fun selectCalendar(calendarId: String?) {
        _catalog.update { it.copy(selectedCalendarId = calendarId) }
    }

    private suspend fun initialize() {
        try {
            val db = requireDatabase()
            watchReplica(db)
            watchStatus(db)
        } catch (error: Throwable) {
            markUnavailable("PowerSync replica unavailable: ${error.message}")
        }
    }

    private fun watchReplica(db: PowerSyncDatabase) {
        val calendars = db.watch(
            """SELECT id, user_id, uid, href, etag, display_name, description, color, revision,
                      created_at, updated_at
                FROM $CALENDARS_TABLE WHERE COALESCE(deleted_local, 0) = 0""",
        ) { cursor ->
            FoyerCalendar(
                id = cursor.getString("id"),
                uid = cursor.getStringOptional("uid").orEmpty(),
                href = cursor.getStringOptional("href").orEmpty(),
                etag = cursor.getStringOptional("etag").orEmpty(),
                displayName = cursor.getString("display_name"),
                description = cursor.getStringOptional("description").orEmpty(),
                color = cursor.getStringOptional("color"),
                revision = cursor.getLongOptional("revision") ?: 1L,
                createdAt = cursor.getStringOptional("created_at").orEmpty(),
                updatedAt = cursor.getStringOptional("updated_at").orEmpty(),
            )
        }
        val events = db.watch(
            """SELECT id, calendar_id, uid, href, etag, summary, description, location, all_day,
                      dtstart, dtend, tzid, rrule, exdates, revision, created_at, updated_at
                FROM $EVENTS_TABLE WHERE COALESCE(deleted_local, 0) = 0""",
        ) { cursor ->
            FoyerEvent(
                id = cursor.getString("id"),
                calendarId = cursor.getString("calendar_id"),
                uid = cursor.getStringOptional("uid").orEmpty(),
                href = cursor.getStringOptional("href").orEmpty(),
                etag = cursor.getStringOptional("etag").orEmpty(),
                summary = cursor.getString("summary"),
                description = cursor.getStringOptional("description").orEmpty(),
                location = cursor.getStringOptional("location").orEmpty(),
                allDay = (cursor.getLongOptional("all_day") ?: 0L) != 0L,
                dtstart = cursor.getString("dtstart"),
                dtend = cursor.getStringOptional("dtend"),
                tzid = cursor.getStringOptional("tzid"),
                rrule = cursor.getStringOptional("rrule"),
                exdates = cursor.getStringOptional("exdates") ?: "[]",
                revision = cursor.getLongOptional("revision") ?: 1L,
                createdAt = cursor.getStringOptional("created_at").orEmpty(),
                updatedAt = cursor.getStringOptional("updated_at").orEmpty(),
            )
        }
        val pending = db.watch(
            """SELECT
                (SELECT COUNT(*) FROM $CALENDARS_TABLE WHERE operation_id IS NOT NULL) +
                (SELECT COUNT(*) FROM $EVENTS_TABLE WHERE operation_id IS NOT NULL) AS count""",
        ) { cursor -> cursor.getLongOptional("count")?.toInt() ?: 0 }
        watchJob = scope.launch {
            combine(calendars, events, pending) { calendarRows, eventRows, pendingRows ->
                Triple(calendarRows, eventRows, pendingRows.firstOrNull() ?: 0)
            }.collect { (calendarRows, eventRows, pendingCount) ->
                val status = db.currentStatus
                _catalog.update { current ->
                    CalendarCatalog(
                        calendars = calendarRows.sortedWith(compareBy(FoyerCalendar::displayName, FoyerCalendar::id)),
                        events = eventRows.sortedWith(compareBy(FoyerEvent::dtstart, FoyerEvent::id)),
                        status = CalendarStatus(
                            loading = false,
                            connected = status.connected,
                            offline = !status.connected,
                            pendingUploads = pendingCount,
                            lastError = status.anyError?.toString(),
                            conflictCode = lastConflict?.first,
                            conflictMessage = lastConflict?.second,
                            developmentAuth = BuildConfig.FOYER_DEVELOPMENT_AUTH,
                            usingPowerSync = true,
                        ),
                        selectedCalendarId = current.selectedCalendarId,
                    )
                }
            }
        }
    }

    private fun watchStatus(db: PowerSyncDatabase) {
        statusJob = scope.launch {
            db.currentStatus.asFlow().collect { publishStatus(db) }
        }
    }

    private fun publishStatus(db: PowerSyncDatabase) {
        val sync = db.currentStatus
        _catalog.update { current ->
            current.copy(
                status = current.status.copy(
                    loading = false,
                    connected = sync.connected,
                    offline = !sync.connected,
                    lastError = sync.anyError?.toString(),
                    conflictCode = lastConflict?.first,
                    conflictMessage = lastConflict?.second,
                    developmentAuth = BuildConfig.FOYER_DEVELOPMENT_AUTH,
                    usingPowerSync = true,
                ),
            )
        }
    }

    private suspend fun requireDatabase(): PowerSyncDatabase {
        attachedDatabase?.let { return it }
        val db = databaseProvider()
        attachedDatabase = db
        return db
    }

    companion object {
        fun attach(
            context: Context,
            api: CalendarApi,
            databaseProvider: suspend () -> PowerSyncDatabase,
        ): CalendarStore = CalendarStore(context, api, databaseProvider)
    }
}

private fun draftPayload(operationId: String, draft: EventDraft): String = JSONObject()
    .put("operationId", operationId)
    .put("calendarId", draft.calendarId)
    .put("summary", draft.summary)
    .put("description", draft.description)
    .put("location", draft.location)
    .put("allDay", draft.allDay)
    .put("dtstart", draft.dtstart)
    .put("dtend", draft.dtend)
    .put("tzid", draft.tzid)
    .put("rrule", draft.rrule)
    .put("exdates", encodeExdates(draft.exdates))
    .toString()
