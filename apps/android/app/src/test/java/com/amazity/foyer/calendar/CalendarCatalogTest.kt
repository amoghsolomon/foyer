package com.amazity.foyer.calendar

import com.amazity.foyer.model.CalendarCatalog
import com.amazity.foyer.model.CalendarStatus
import com.amazity.foyer.model.EventDraft
import com.amazity.foyer.model.FoyerCalendar
import com.amazity.foyer.model.FoyerEvent
import com.amazity.foyer.model.calendarSyncBanner
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class CalendarCatalogTest {
    @Test
    fun eventsStayOnTheirCalendarAfterAMove() {
        val catalog = CalendarCatalog(
            calendars = listOf(cal("home", "Home"), cal("work", "Work")),
            events = listOf(event("e1", "work", "Standup")),
        )
        assertEquals(0, catalog.eventsIn("home").size)
        assertEquals("Standup", catalog.eventsIn("work").single().summary)
    }

    @Test
    fun calendarDeleteIsRejectedWhileEventsRemain() {
        val catalog = CalendarCatalog(
            calendars = listOf(cal("home", "Home"), cal("empty", "Empty")),
            events = listOf(event("e1", "home", "Standup")),
        )
        assertEquals(
            "This calendar still has events. Move or delete them first.",
            catalog.validateCalendarDelete(catalog.calendar("home")!!),
        )
        assertNull(catalog.validateCalendarDelete(catalog.calendar("empty")!!))
    }

    @Test
    fun draftValidationRequiresTitleAndCalendar() {
        val catalog = CalendarCatalog(calendars = listOf(cal("home", "Home")), events = emptyList())
        assertEquals(
            "A title is required.",
            catalog.validateEventDraft(
                EventDraft(summary = "  ", dtstart = "20260315", calendarId = "home"),
            ),
        )
        assertEquals(
            "Choose a calendar.",
            catalog.validateEventDraft(
                EventDraft(summary = "Lunch", dtstart = "20260315", calendarId = "missing"),
            ),
        )
        assertNull(
            catalog.validateEventDraft(
                EventDraft(summary = "Lunch", dtstart = "20260315", calendarId = "home"),
            ),
        )
    }

    @Test
    fun staleEtagBecomesAConflictBanner() {
        val banner = calendarSyncBanner(
            CalendarStatus(
                loading = false,
                conflictCode = "stale_etag",
                conflictMessage = "Someone else changed this item.",
            ),
        )
        assertEquals("Someone else changed this item.", (banner as com.amazity.foyer.model.CalendarSyncBanner.StaleEtag).message)
    }

    private fun cal(id: String, name: String) = FoyerCalendar(
        id = id,
        uid = id,
        href = "/$id/",
        etag = "\"1\"",
        displayName = name,
    )

    private fun event(id: String, calendarId: String, summary: String) = FoyerEvent(
        id = id,
        calendarId = calendarId,
        uid = id,
        href = "/$calendarId/$id.ics",
        etag = "\"1\"",
        summary = summary,
        dtstart = "20260302T100000",
    )
}
