package com.amazity.foyer.calendar

import com.amazity.foyer.model.FoyerEvent
import java.time.LocalDate
import java.time.LocalDateTime
import java.time.ZoneId
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class RecurrenceTest {
    @Test
    fun weeklyRruleSkipsExdate() {
        val event = sample(
            dtstart = "20260302T100000",
            dtend = "20260302T103000",
            tzid = "America/New_York",
            rrule = "FREQ=WEEKLY;BYDAY=MO",
            exdates = encodeExdates(listOf("20260309T100000")),
        )
        val days = expandEvent(event, LocalDate.of(2026, 3, 1), LocalDate.of(2026, 3, 31), 20)
            .map { it.recurrenceId.take(8) }
        assertEquals(listOf("20260302", "20260316", "20260323", "20260330"), days)
        assertFalse(days.contains("20260309"))
    }

    @Test
    fun allDayDateStaysADate() {
        val event = sample(
            summary = "Holiday",
            allDay = true,
            dtstart = "20260315",
            dtend = "20260316",
            tzid = null,
        )
        val items = expandEvent(event, LocalDate.of(2026, 3, 1), LocalDate.of(2026, 3, 31), 8)
        assertEquals(1, items.size)
        assertTrue(items.single().allDay)
        assertEquals("20260315", items.single().recurrenceId)
        assertEquals(null, items.single().startEpochMillis)
    }

    @Test
    fun tzidKeepsLocalWallTimeAcrossDst() {
        val event = sample(
            dtstart = "20260302T100000",
            dtend = "20260302T110000",
            tzid = "America/New_York",
            rrule = "FREQ=WEEKLY;BYDAY=MO",
        )
        val items = expandEvent(event, LocalDate.of(2026, 3, 1), LocalDate.of(2026, 3, 15), 8)
        assertEquals(2, items.size)
        val zone = ZoneId.of("America/New_York")
        val before = LocalDateTime.of(2026, 3, 2, 10, 0).atZone(zone).toInstant().toEpochMilli()
        val after = LocalDateTime.of(2026, 3, 9, 10, 0).atZone(zone).toInstant().toEpochMilli()
        assertEquals(before, items[0].startEpochMillis)
        assertEquals(after, items[1].startEpochMillis)
        assertEquals(7L * 86_400_000L - 3_600_000L, after - before)
    }

    @Test
    fun fallBackUsesFirstValidOffset() {
        val local = LocalDateTime.of(2026, 11, 1, 1, 30)
        val instant = resolveInstant(local, ZoneId.of("America/New_York"))
        assertEquals(
            LocalDateTime.of(2026, 11, 1, 1, 30).atZone(ZoneId.of("America/New_York")).toInstant(),
            instant,
        )
    }

    @Test
    fun boundedDailyExpansionStaysInsideWindow() {
        val event = sample(
            allDay = true,
            dtstart = "20200101",
            dtend = "20200102",
            tzid = null,
            rrule = "FREQ=DAILY",
        )
        val items = expandEvent(event, LocalDate.of(2026, 3, 1), LocalDate.of(2026, 3, 7), 512)
        assertEquals(7, items.size)
        assertEquals("20260301", items.first().recurrenceId)
        assertEquals("20260307", items.last().recurrenceId)
    }

    @Test
    fun monthlyNthWeekdayHonorsCount() {
        val event = sample(
            allDay = true,
            dtstart = "20260310",
            dtend = "20260311",
            tzid = null,
            rrule = "FREQ=MONTHLY;COUNT=4;BYDAY=2TU",
        )
        val days = expandEvent(event, LocalDate.of(2026, 3, 1), LocalDate.of(2026, 8, 1), 16)
            .map { it.recurrenceId }
        assertEquals(listOf("20260310", "20260414", "20260512", "20260609"), days)
    }

    @Test
    fun recurrenceSummaryIsReadable() {
        assertEquals("Does not repeat", recurrenceSummary(null))
        assertEquals("Weekly on Monday", recurrenceSummary("FREQ=WEEKLY;BYDAY=MO"))
    }

    @Test
    fun occurrenceDisplayKeepsAllDayAndTimedLabels() {
        val timed = expandEvent(
            sample(
                dtstart = "20260302T100000",
                dtend = "20260302T103000",
                tzid = "America/New_York",
            ),
            LocalDate.of(2026, 3, 2),
            LocalDate.of(2026, 3, 2),
            4,
        ).single()
        assertTrue(occurrenceTimeLabel(timed).contains("10:00"))
        assertTrue(occurrenceTimeLabel(timed).contains("10:30"))

        val allDay = expandEvent(
            sample(
                summary = "Holiday",
                allDay = true,
                dtstart = "20260315",
                dtend = "20260316",
                tzid = null,
            ),
            LocalDate.of(2026, 3, 15),
            LocalDate.of(2026, 3, 15),
            4,
        ).single()
        assertEquals("All day", occurrenceTimeLabel(allDay))
        val whenLabel = eventWhenLabel(
            sample(
                summary = "Holiday",
                allDay = true,
                dtstart = "20260315",
                dtend = "20260316",
                tzid = null,
            ),
        )
        assertTrue(whenLabel.contains("15"))
        assertTrue(whenLabel.contains("All day"))
    }

    private fun sample(
        summary: String = "Standup",
        allDay: Boolean = false,
        dtstart: String,
        dtend: String?,
        tzid: String?,
        rrule: String? = null,
        exdates: String = "[]",
    ) = FoyerEvent(
        id = "00000000-0000-4000-a000-000000000003",
        calendarId = "00000000-0000-4000-a000-000000000002",
        uid = "sample",
        href = "/cal/sample.ics",
        etag = "\"1\"",
        summary = summary,
        allDay = allDay,
        dtstart = dtstart,
        dtend = dtend,
        tzid = tzid,
        rrule = rrule,
        exdates = exdates,
    )
}
