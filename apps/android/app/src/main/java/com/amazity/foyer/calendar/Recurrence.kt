package com.amazity.foyer.calendar

import com.amazity.foyer.model.EventOccurrence
import com.amazity.foyer.model.FoyerEvent
import java.time.DateTimeException
import java.time.DayOfWeek
import java.time.Duration
import java.time.Instant
import java.time.LocalDate
import java.time.LocalDateTime
import java.time.LocalTime
import java.time.ZoneId
import java.time.ZoneOffset
import java.time.format.DateTimeFormatter
import java.time.temporal.TemporalAdjusters
import kotlin.math.abs

const val MAX_EXPANSION_INSTANCES: Int = 512
const val MAX_WINDOW_DAYS: Long = 366L * 2L

private val DATE = DateTimeFormatter.BASIC_ISO_DATE
private val LOCAL_DATE_TIME = DateTimeFormatter.ofPattern("yyyyMMdd'T'HHmmss")
private val UTC_DATE_TIME = DateTimeFormatter.ofPattern("yyyyMMdd'T'HHmmss'Z'")

data class RecurrenceRule(
    val freq: Freq,
    val interval: Int = 1,
    val count: Int? = null,
    val untilDate: LocalDate? = null,
    val byDay: List<ByDay> = emptyList(),
    val byMonthDay: List<Int> = emptyList(),
    val byMonth: List<Int> = emptyList(),
    val bySetPos: List<Int> = emptyList(),
    val weekStart: DayOfWeek = DayOfWeek.MONDAY,
) {
    fun ical(): String = buildString {
        append("FREQ=").append(freq.name)
        if (interval != 1) append(";INTERVAL=").append(interval)
        count?.let { append(";COUNT=").append(it) }
        untilDate?.let { append(";UNTIL=").append(it.format(DATE)) }
        if (byDay.isNotEmpty()) {
            append(";BYDAY=").append(byDay.joinToString(",") { it.ical() })
        }
        if (byMonthDay.isNotEmpty()) {
            append(";BYMONTHDAY=").append(byMonthDay.joinToString(","))
        }
        if (byMonth.isNotEmpty()) {
            append(";BYMONTH=").append(byMonth.joinToString(","))
        }
        if (bySetPos.isNotEmpty()) {
            append(";BYSETPOS=").append(bySetPos.joinToString(","))
        }
        if (weekStart != DayOfWeek.MONDAY) {
            append(";WKST=").append(weekStart.ical())
        }
    }
}

enum class Freq { DAILY, WEEKLY, MONTHLY, YEARLY }

data class ByDay(val weekday: DayOfWeek, val nth: Int? = null) {
    fun ical(): String = (nth?.toString() ?: "") + weekday.ical()
}

fun parseRrule(value: String): RecurrenceRule {
    var freq: Freq? = null
    var interval = 1
    var count: Int? = null
    var untilDate: LocalDate? = null
    val byDay = mutableListOf<ByDay>()
    val byMonthDay = mutableListOf<Int>()
    val byMonth = mutableListOf<Int>()
    val bySetPos = mutableListOf<Int>()
    var weekStart = DayOfWeek.MONDAY
    value.split(';').filter { it.isNotBlank() }.forEach { part ->
        val (key, raw) = part.split('=', limit = 2)
        when (key.uppercase()) {
            "FREQ" -> freq = Freq.valueOf(raw)
            "INTERVAL" -> interval = raw.toInt().also { require(it >= 1) { "INTERVAL must be at least 1" } }
            "COUNT" -> count = raw.toInt()
            "UNTIL" -> untilDate = parseDateToken(raw)
            "BYDAY" -> raw.split(',').forEach { byDay += parseByDay(it) }
            "BYMONTHDAY" -> raw.split(',').forEach { byMonthDay += it.toInt() }
            "BYMONTH" -> raw.split(',').forEach { byMonth += it.toInt() }
            "BYSETPOS" -> raw.split(',').forEach { bySetPos += it.toInt() }
            "WKST" -> weekStart = parseWeekday(raw)
            else -> error("unsupported RRULE part $key")
        }
    }
    return RecurrenceRule(
        freq = requireNotNull(freq) { "RRULE requires FREQ" },
        interval = interval,
        count = count,
        untilDate = untilDate,
        byDay = byDay,
        byMonthDay = byMonthDay,
        byMonth = byMonth,
        bySetPos = bySetPos,
        weekStart = weekStart,
    )
}

fun parseExdates(raw: String): List<String> {
    val trimmed = raw.trim()
    if (trimmed.isEmpty() || trimmed == "[]") return emptyList()
    return if (trimmed.startsWith('[')) {
        trimmed.trim('[', ']').split(',').map { it.trim().trim('"') }.filter { it.isNotEmpty() }
    } else {
        trimmed.split(',', ';').map { it.trim() }.filter { it.isNotEmpty() }
    }
}

fun encodeExdates(values: List<String>): String =
    values.joinToString(prefix = "[", postfix = "]") { "\"$it\"" }

fun expandEvent(
    event: FoyerEvent,
    windowStart: LocalDate,
    windowEnd: LocalDate,
    limit: Int = MAX_EXPANSION_INSTANCES,
): List<EventOccurrence> {
    require(!windowEnd.isBefore(windowStart)) { "expansion window end must not precede start" }
    require(windowEnd.toEpochDay() - windowStart.toEpochDay() <= MAX_WINDOW_DAYS) {
        "expansion window may be at most $MAX_WINDOW_DAYS days"
    }
    val cap = limit.coerceIn(1, MAX_EXPANSION_INSTANCES)
    val seed = parseStart(event)
    val duration = eventDuration(event, seed)
    val excluded = parseExdates(event.exdates).map { parseDateToken(it) }.toSet()
    val dates = if (event.rrule.isNullOrBlank()) {
        listOf(seed.date).filter { it in windowStart..windowEnd }
    } else {
        expandRule(parseRrule(event.rrule), seed.date, windowStart, windowEnd, cap * 4)
    }
    return dates
        .filter { it !in excluded && it in windowStart..windowEnd }
        .distinct()
        .sorted()
        .take(cap)
        .mapNotNull { date -> occurrence(event, seed, date, duration) }
}

fun recurrenceSummary(rrule: String?): String {
    if (rrule.isNullOrBlank()) return "Does not repeat"
    val rule = runCatching { parseRrule(rrule) }.getOrNull() ?: return rrule
    val interval = if (rule.interval == 1) "" else "every ${rule.interval} "
    val unit = when (rule.freq) {
        Freq.DAILY -> if (rule.interval == 1) "Daily" else "${interval}days"
        Freq.WEEKLY -> if (rule.interval == 1) "Weekly" else "${interval}weeks"
        Freq.MONTHLY -> if (rule.interval == 1) "Monthly" else "${interval}months"
        Freq.YEARLY -> if (rule.interval == 1) "Yearly" else "${interval}years"
    }
    val days = if (rule.byDay.isNotEmpty()) {
        " on " + rule.byDay.joinToString(", ") {
            val name = it.weekday.name.lowercase().replaceFirstChar(Char::uppercase)
            if (it.nth == null) name else "${ordinal(it.nth)} $name"
        }
    } else {
        ""
    }
    val bound = when {
        rule.count != null -> ", ${rule.count} times"
        rule.untilDate != null -> ", until ${rule.untilDate}"
        else -> ""
    }
    return "$unit$days$bound"
}

private data class Seed(val date: LocalDate, val time: LocalTime)

private fun parseStart(event: FoyerEvent): Seed {
    return if (event.allDay || event.dtstart.length == 8) {
        Seed(parseDateToken(event.dtstart), LocalTime.MIDNIGHT)
    } else {
        val local = parseLocalDateTime(event.dtstart)
        Seed(local.toLocalDate(), local.toLocalTime())
    }
}

private fun eventDuration(event: FoyerEvent, seed: Seed): Duration {
    val end = event.dtend?.takeIf { it.isNotBlank() } ?: return if (event.allDay) {
        Duration.ofDays(1)
    } else {
        Duration.ofHours(1)
    }
    return if (event.allDay) {
        Duration.ofDays(parseDateToken(end).toEpochDay() - seed.date.toEpochDay())
    } else {
        val endLocal = parseLocalDateTime(end)
        Duration.between(LocalDateTime.of(seed.date, seed.time), endLocal).abs()
    }
}

private fun occurrence(
    event: FoyerEvent,
    seed: Seed,
    date: LocalDate,
    duration: Duration,
): EventOccurrence? {
    val local = LocalDateTime.of(date, seed.time)
    val (startMillis, startLocal) = if (event.allDay) {
        null to date.toString()
    } else {
        val zone = zoneId(event.tzid)
        val zoned = runCatching { local.atZone(zone) }.getOrNull() ?: return null
        if (zoned.toLocalDateTime() != local && zone.rules.getValidOffsets(local).isEmpty()) {
            return null
        }
        val instant = resolveInstant(local, zone) ?: return null
        instant.toEpochMilli() to local.format(LOCAL_DATE_TIME)
    }
    val endMillis = startMillis?.plus(duration.toMillis())
    val endLocal = if (event.allDay) {
        event.dtend?.let { parseDateToken(it).toString() }
    } else {
        endMillis?.let { millis ->
            val zone = zoneId(event.tzid)
            Instant.ofEpochMilli(millis).atZone(zone).toLocalDateTime().format(LOCAL_DATE_TIME)
        }
    }
    return EventOccurrence(
        eventId = event.id,
        calendarId = event.calendarId,
        uid = event.uid,
        summary = event.summary,
        description = event.description,
        location = event.location,
        allDay = event.allDay,
        tzid = event.tzid,
        startEpochMillis = startMillis,
        endEpochMillis = endMillis,
        startLocal = startLocal,
        endLocal = endLocal,
        recurrenceId = if (event.allDay) date.format(DATE) else local.format(LOCAL_DATE_TIME),
        isRecurring = event.isRecurring,
    )
}

fun zoneId(tzid: String?): ZoneId {
    val name = tzid?.takeIf { it.isNotBlank() && it != "UTC" && it != "Z" } ?: return ZoneOffset.UTC
    return runCatching { ZoneId.of(name) }.getOrDefault(ZoneOffset.UTC)
}

fun resolveInstant(local: LocalDateTime, zone: ZoneId): Instant? {
    val rules = zone.rules
    val offsets = rules.getValidOffsets(local)
    return when {
        offsets.isEmpty() -> null
        offsets.size == 1 -> local.toInstant(offsets.first())
        else -> local.toInstant(offsets.first())
    }
}

private fun expandRule(
    rule: RecurrenceRule,
    seed: LocalDate,
    windowStart: LocalDate,
    windowEnd: LocalDate,
    safety: Int,
): List<LocalDate> {
    val dates = mutableListOf<LocalDate>()
    var emitted = 0
    var cursor = seed
    var iterations = 0
    val hard = (safety.coerceAtLeast(16) * 8).coerceAtLeast(64)
    while (iterations < hard && dates.size < safety) {
        iterations += 1
        var set = periodCandidates(rule, seed, cursor)
        if (rule.byMonth.isNotEmpty()) set = set.filter { it.monthValue in rule.byMonth }
        if (rule.byMonthDay.isNotEmpty()) set = set.filter { matchesMonthDay(it, rule.byMonthDay) }
        if (rule.byDay.isNotEmpty() && rule.freq != Freq.WEEKLY) {
            set = set.filter { matchesByDay(it, rule.byDay) }
        }
        if (rule.freq == Freq.WEEKLY && rule.byDay.isNotEmpty()) {
            set = weeklyByDay(cursor, seed, rule)
        }
        if (rule.bySetPos.isNotEmpty()) {
            val ordered = set.distinct().sorted()
            set = applySetPos(ordered, rule.bySetPos)
        }
        for (date in set.distinct().sorted()) {
            if (date.isBefore(seed)) continue
            rule.untilDate?.let { if (date.isAfter(it)) return dates }
            emitted += 1
            rule.count?.let { if (emitted > it) return dates }
            if (!date.isBefore(windowStart) && !date.isAfter(windowEnd)) dates += date
        }
        cursor = advance(rule, cursor) ?: break
        if (cursor.isAfter(windowEnd.plusDays(400))) break
    }
    return dates
}

private fun periodCandidates(rule: RecurrenceRule, seed: LocalDate, cursor: LocalDate): List<LocalDate> {
    return when (rule.freq) {
        Freq.DAILY -> listOf(cursor)
        Freq.WEEKLY -> if (rule.byDay.isEmpty()) listOf(cursor) else weeklyByDay(cursor, seed, rule)
        Freq.MONTHLY -> when {
            rule.byDay.any { it.nth != null } -> rule.byDay.mapNotNull { nthWeekday(cursor.year, cursor.monthValue, it) }
            rule.byMonthDay.isNotEmpty() -> rule.byMonthDay.mapNotNull { monthDay(cursor.year, cursor.monthValue, it) }
            else -> runCatching { LocalDate.of(cursor.year, cursor.month, seed.dayOfMonth) }.getOrNull()?.let(::listOf).orEmpty()
        }
        Freq.YEARLY -> if (rule.byMonth.isNotEmpty()) {
            rule.byMonth.mapNotNull { runCatching { LocalDate.of(cursor.year, it, seed.dayOfMonth) }.getOrNull() }
        } else {
            runCatching { seed.withYear(cursor.year) }.getOrNull()?.let(::listOf).orEmpty()
        }
    }
}

private fun weeklyByDay(cursor: LocalDate, seed: LocalDate, rule: RecurrenceRule): List<LocalDate> {
    val weekStart = weekStartDate(cursor, rule.weekStart)
    val seedWeek = weekStartDate(seed, rule.weekStart)
    val weeks = (weekStart.toEpochDay() - seedWeek.toEpochDay()) / 7
    if (Math.floorMod(weeks, rule.interval.toLong()) != 0L) return emptyList()
    return rule.byDay.map { day ->
        val delta = Math.floorMod(day.weekday.value - rule.weekStart.value, 7).toLong()
        weekStart.plusDays(delta)
    }
}

private fun weekStartDate(date: LocalDate, weekStart: DayOfWeek): LocalDate {
    val delta = Math.floorMod(date.dayOfWeek.value - weekStart.value, 7).toLong()
    return date.minusDays(delta)
}

private fun matchesMonthDay(date: LocalDate, days: List<Int>): Boolean {
    val last = date.lengthOfMonth()
    return days.any { day -> date.dayOfMonth == if (day > 0) day else last + day + 1 }
}

private fun matchesByDay(date: LocalDate, days: List<ByDay>): Boolean =
    days.any { spec ->
        date.dayOfWeek == spec.weekday && (spec.nth == null || nthWeekday(date.year, date.monthValue, spec) == date)
    }

private fun nthWeekday(year: Int, month: Int, spec: ByDay): LocalDate? {
    val nth = spec.nth ?: return null
    return try {
        if (nth > 0) {
            LocalDate.of(year, month, 1).with(TemporalAdjusters.dayOfWeekInMonth(nth, spec.weekday))
        } else {
            LocalDate.of(year, month, 1).with(TemporalAdjusters.lastInMonth(spec.weekday)).plusWeeks((nth + 1).toLong())
        }
    } catch (_: DateTimeException) {
        null
    }
}

private fun monthDay(year: Int, month: Int, day: Int): LocalDate? {
    val last = LocalDate.of(year, month, 1).lengthOfMonth()
    val resolved = if (day > 0) day else last + day + 1
    return runCatching { LocalDate.of(year, month, resolved) }.getOrNull()
}

private fun applySetPos(dates: List<LocalDate>, positions: List<Int>): List<LocalDate> =
    positions.mapNotNull { pos ->
        val index = if (pos > 0) pos - 1 else dates.size + pos
        dates.getOrNull(index)
    }

private fun advance(rule: RecurrenceRule, cursor: LocalDate): LocalDate? = when (rule.freq) {
    Freq.DAILY -> cursor.plusDays(rule.interval.toLong())
    Freq.WEEKLY -> cursor.plusWeeks(rule.interval.toLong())
    Freq.MONTHLY -> cursor.plusMonths(rule.interval.toLong())
    Freq.YEARLY -> cursor.plusYears(rule.interval.toLong())
}

private fun parseByDay(token: String): ByDay {
    val day = token.takeLast(2)
    val nth = token.dropLast(2).takeIf { it.isNotEmpty() }?.toInt()
    require(nth != 0) { "BYDAY occurrence cannot be 0" }
    return ByDay(parseWeekday(day), nth)
}

private fun parseWeekday(token: String): DayOfWeek = when (token) {
    "MO" -> DayOfWeek.MONDAY
    "TU" -> DayOfWeek.TUESDAY
    "WE" -> DayOfWeek.WEDNESDAY
    "TH" -> DayOfWeek.THURSDAY
    "FR" -> DayOfWeek.FRIDAY
    "SA" -> DayOfWeek.SATURDAY
    "SU" -> DayOfWeek.SUNDAY
    else -> error("unknown weekday $token")
}

private fun DayOfWeek.ical(): String = when (this) {
    DayOfWeek.MONDAY -> "MO"
    DayOfWeek.TUESDAY -> "TU"
    DayOfWeek.WEDNESDAY -> "WE"
    DayOfWeek.THURSDAY -> "TH"
    DayOfWeek.FRIDAY -> "FR"
    DayOfWeek.SATURDAY -> "SA"
    DayOfWeek.SUNDAY -> "SU"
}

private fun parseDateToken(raw: String): LocalDate {
    val digits = raw.filter(Char::isDigit).take(8)
    return LocalDate.parse(digits, DATE)
}

private fun parseLocalDateTime(raw: String): LocalDateTime {
    val compact = raw.replace("-", "").replace(":", "")
    return if (compact.endsWith('Z')) {
        LocalDateTime.parse(compact, UTC_DATE_TIME)
    } else if ('T' in compact) {
        LocalDateTime.parse(compact.take(15), LOCAL_DATE_TIME)
    } else {
        parseDateToken(compact).atStartOfDay()
    }
}

private fun ordinal(value: Int): String {
    val abs = abs(value)
    val suffix = when (abs % 100) {
        11, 12, 13 -> "th"
        else -> when (abs % 10) {
            1 -> "st"
            2 -> "nd"
            3 -> "rd"
            else -> "th"
        }
    }
    return if (value < 0) "last" else "$abs$suffix"
}

fun formatStamp(date: LocalDate, time: LocalTime?, allDay: Boolean): String =
    if (allDay) date.format(DATE) else LocalDateTime.of(date, time ?: LocalTime.MIDNIGHT).format(LOCAL_DATE_TIME)
