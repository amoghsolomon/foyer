package com.amazity.foyer.calendar

import com.amazity.foyer.model.EventOccurrence
import com.amazity.foyer.model.FoyerEvent
import java.time.LocalDate
import java.time.LocalTime
import java.time.format.DateTimeFormatter
import java.util.Locale

private val MONTH_TITLE = DateTimeFormatter.ofPattern("LLLL yyyy", Locale.getDefault())
private val WEEKDAY = DateTimeFormatter.ofPattern("EEE", Locale.getDefault())
private val DAY_HEADING = DateTimeFormatter.ofPattern("EEEE, d MMMM", Locale.getDefault())
private val TIME = DateTimeFormatter.ofPattern("h:mm a", Locale.getDefault())

fun monthTitle(date: LocalDate): String = date.format(MONTH_TITLE)

fun weekdayLabel(date: LocalDate): String = date.format(WEEKDAY)

fun dayHeading(date: LocalDate): String = date.format(DAY_HEADING)

fun occurrenceTimeLabel(occurrence: EventOccurrence): String {
    if (occurrence.allDay) return "All day"
    val start = parseOccurrenceTime(occurrence.startLocal) ?: return occurrence.startLocal
    val end = occurrence.endLocal?.let(::parseOccurrenceTime)
    return if (end == null) start.format(TIME) else "${start.format(TIME)} – ${end.format(TIME)}"
}

fun eventWhenLabel(event: FoyerEvent): String {
    val seedDate = runCatching {
        val digits = event.dtstart.filter(Char::isDigit).take(8)
        LocalDate.parse(digits, DateTimeFormatter.BASIC_ISO_DATE)
    }.getOrNull() ?: return event.dtstart
    val time = if (event.allDay) {
        "All day"
    } else {
        runCatching {
            val compact = event.dtstart.replace("-", "").replace(":", "")
            LocalTime.parse(compact.substringAfter('T').take(6), DateTimeFormatter.ofPattern("HHmmss"))
                .format(TIME)
        }.getOrDefault(event.dtstart)
    }
    val zone = event.tzid?.takeIf { it.isNotBlank() && it != "UTC" }?.let { " · $it" }.orEmpty()
    return "${dayHeading(seedDate)} · $time$zone"
}

fun monthCells(visibleMonth: LocalDate): List<LocalDate?> {
    val first = visibleMonth.withDayOfMonth(1)
    val lead = Math.floorMod(first.dayOfWeek.value - 1, 7)
    val days = first.lengthOfMonth()
    return buildList {
        repeat(lead) { add(null) }
        for (day in 1..days) add(first.withDayOfMonth(day))
        while (size % 7 != 0) add(null)
    }
}

private fun parseOccurrenceTime(stamp: String): LocalTime? {
    val compact = stamp.replace("-", "").replace(":", "")
    val time = compact.substringAfter('T', missingDelimiterValue = "").take(6)
    if (time.length != 6) return null
    return runCatching { LocalTime.parse(time, DateTimeFormatter.ofPattern("HHmmss")) }.getOrNull()
}
