package com.amazity.foyer.model

/** Normalized calendar and event rows from the shared PowerSync replica. */
data class FoyerCalendar(
    val id: String,
    val uid: String,
    val href: String,
    val etag: String,
    val displayName: String,
    val description: String = "",
    val color: String? = null,
    val revision: Long = 1,
    val createdAt: String = "",
    val updatedAt: String = "",
)

data class FoyerEvent(
    val id: String,
    val calendarId: String,
    val uid: String,
    val href: String,
    val etag: String,
    val summary: String,
    val description: String = "",
    val location: String = "",
    val allDay: Boolean = false,
    val dtstart: String,
    val dtend: String? = null,
    val tzid: String? = null,
    val rrule: String? = null,
    val exdates: String = "[]",
    val revision: Long = 1,
    val createdAt: String = "",
    val updatedAt: String = "",
) {
    val isRecurring: Boolean get() = !rrule.isNullOrBlank()
}

data class EventDraft(
    val summary: String,
    val description: String = "",
    val location: String = "",
    val allDay: Boolean = false,
    val dtstart: String,
    val dtend: String? = null,
    val tzid: String? = null,
    val rrule: String? = null,
    val exdates: List<String> = emptyList(),
    val calendarId: String,
)

data class EventOccurrence(
    val eventId: String,
    val calendarId: String,
    val uid: String,
    val summary: String,
    val description: String,
    val location: String,
    val allDay: Boolean,
    val tzid: String?,
    val startEpochMillis: Long?,
    val endEpochMillis: Long?,
    val startLocal: String,
    val endLocal: String?,
    val recurrenceId: String,
    val isRecurring: Boolean,
)

data class CalendarStatus(
    val loading: Boolean = true,
    val connected: Boolean = false,
    val offline: Boolean = false,
    val pendingUploads: Int = 0,
    val lastError: String? = null,
    val conflictCode: String? = null,
    val conflictMessage: String? = null,
    val developmentAuth: Boolean = false,
    val usingPowerSync: Boolean = false,
) {
    fun banner(): CalendarSyncBanner? = calendarSyncBanner(this)
}

sealed class CalendarSyncBanner {
    data class Offline(val pendingUploads: Int) : CalendarSyncBanner()
    data class Pending(val pendingUploads: Int) : CalendarSyncBanner()
    data class StaleEtag(val message: String) : CalendarSyncBanner()
    data class Error(val message: String) : CalendarSyncBanner()
}

fun calendarSyncBanner(status: CalendarStatus): CalendarSyncBanner? {
    val conflict = status.conflictMessage?.takeIf { it.isNotBlank() }
    if (conflict != null) {
        return if (
            status.conflictCode == "stale_etag" ||
            status.conflictCode == "stale_revision" ||
            conflict.contains("stale", ignoreCase = true)
        ) {
            CalendarSyncBanner.StaleEtag(conflict)
        } else {
            CalendarSyncBanner.Error(conflict)
        }
    }
    status.lastError?.takeIf { it.isNotBlank() }?.let { return CalendarSyncBanner.Error(it) }
    if (status.offline) return CalendarSyncBanner.Offline(status.pendingUploads)
    if (status.pendingUploads > 0) return CalendarSyncBanner.Pending(status.pendingUploads)
    return null
}

data class CalendarCatalog(
    val calendars: List<FoyerCalendar>,
    val events: List<FoyerEvent>,
    val status: CalendarStatus = CalendarStatus(loading = false),
    val selectedCalendarId: String? = null,
) {
    fun calendar(id: String): FoyerCalendar? = calendars.firstOrNull { it.id == id }

    fun event(id: String): FoyerEvent? = events.firstOrNull { it.id == id }

    fun visibleCalendars(): List<FoyerCalendar> =
        calendars.sortedWith(compareBy(FoyerCalendar::displayName, FoyerCalendar::id))

    fun eventsIn(calendarId: String?): List<FoyerEvent> =
        events.filter { calendarId == null || it.calendarId == calendarId }

    fun selectedCalendar(): FoyerCalendar? =
        selectedCalendarId?.let(::calendar) ?: calendars.firstOrNull()

    fun validateCalendarDelete(calendar: FoyerCalendar): String? =
        if (eventsIn(calendar.id).isEmpty()) {
            null
        } else {
            "This calendar still has events. Move or delete them first."
        }

    fun validateEventDraft(draft: EventDraft): String? {
        if (draft.summary.isBlank()) return "A title is required."
        if (calendar(draft.calendarId) == null) return "Choose a calendar."
        if (draft.dtstart.isBlank()) return "A start date is required."
        return null
    }
}
