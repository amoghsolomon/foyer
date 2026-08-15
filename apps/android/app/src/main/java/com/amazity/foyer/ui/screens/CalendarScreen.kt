package com.amazity.foyer.ui.screens

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Switch
import androidx.compose.material3.SwitchDefaults
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.amazity.foyer.calendar.dayHeading
import com.amazity.foyer.calendar.eventWhenLabel
import com.amazity.foyer.calendar.expandEvent
import com.amazity.foyer.calendar.formatStamp
import com.amazity.foyer.calendar.monthCells
import com.amazity.foyer.calendar.monthTitle
import com.amazity.foyer.calendar.occurrenceTimeLabel
import com.amazity.foyer.calendar.parseExdates
import com.amazity.foyer.calendar.recurrenceSummary
import com.amazity.foyer.calendar.weekdayLabel
import com.amazity.foyer.model.CalendarCatalog
import com.amazity.foyer.model.CalendarStatus
import com.amazity.foyer.model.CalendarSyncBanner
import com.amazity.foyer.model.EventDraft
import com.amazity.foyer.model.EventOccurrence
import com.amazity.foyer.model.FoyerCalendar
import com.amazity.foyer.model.FoyerEvent
import com.amazity.foyer.ui.components.ContentStatePanel
import com.amazity.foyer.ui.components.ErrorStatePanel
import com.amazity.foyer.ui.components.FoyerScreen
import com.amazity.foyer.ui.components.HairlineDivider
import com.amazity.foyer.ui.components.LoadingStatePanel
import com.amazity.foyer.ui.components.NestedScreenHeader
import com.amazity.foyer.ui.components.PlusGlyph
import com.amazity.foyer.ui.components.SectionLabel
import com.amazity.foyer.ui.theme.FoyerBlack
import com.amazity.foyer.ui.theme.FoyerLine
import com.amazity.foyer.ui.theme.FoyerSurface
import com.amazity.foyer.ui.theme.FoyerText
import com.amazity.foyer.ui.theme.FoyerTextDim
import com.amazity.foyer.ui.theme.FoyerTextMuted
import java.time.LocalDate
import java.time.LocalTime

@Composable
fun CalendarPage(
    catalog: CalendarCatalog,
    selectedDate: LocalDate,
    visibleMonth: LocalDate,
    onSelectDate: (LocalDate) -> Unit,
    onShiftMonth: (Long) -> Unit,
    onSelectCalendar: (String?) -> Unit,
    onOpenEvent: (String) -> Unit,
    onCreateEvent: () -> Unit,
    onRetry: () -> Unit = {},
    modifier: Modifier = Modifier,
) {
    val selectedCalendar = catalog.selectedCalendar()
    val monthEnd = visibleMonth.withDayOfMonth(visibleMonth.lengthOfMonth())
    val occurrences = catalog.eventsIn(selectedCalendar?.id)
        .flatMap { expandEvent(it, visibleMonth.withDayOfMonth(1), monthEnd, 128) }
        .sortedWith(compareBy({ it.startLocal }, { it.eventId }))
    val dayItems = occurrences.filter { it.recurrenceId.startsWith(selectedDate.toString().replace("-", "")) }
    val upcoming = occurrences.filter { it.startLocal >= selectedDate.toString().replace("-", "") }

    Column(
        modifier = modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(top = 10.dp, bottom = 88.dp),
    ) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            SectionLabel("Calendar")
            Box(modifier = Modifier.clickable(onClick = onCreateEvent).padding(8.dp)) { PlusGlyph() }
        }
        CalendarStatusBanner(catalog.status)
        when {
            catalog.status.loading -> {
                LoadingStatePanel("Loading calendars")
                return@Column
            }
            catalog.status.lastError != null && catalog.calendars.isEmpty() -> {
                ErrorStatePanel(catalog.status.lastError.orEmpty(), onRetry)
                return@Column
            }
        }
        CalendarPicker(
            calendars = catalog.visibleCalendars(),
            selectedId = selectedCalendar?.id,
            onSelect = onSelectCalendar,
        )
        Spacer(Modifier.height(16.dp))
        MonthBoard(
            month = visibleMonth,
            selected = selectedDate,
            marked = occurrences.map { it.recurrenceId.take(8) }.toSet(),
            onSelectDate = onSelectDate,
            onShiftMonth = onShiftMonth,
        )
        Spacer(Modifier.height(22.dp))
        SectionLabel(dayHeading(selectedDate))
        Spacer(Modifier.height(6.dp))
        if (dayItems.isEmpty()) {
            ContentStatePanel(
                title = "Nothing on this day",
                message = "Create an event or choose another day in the month.",
                actionLabel = "New event",
                onAction = onCreateEvent,
            )
        } else {
            dayItems.forEachIndexed { index, item ->
                OccurrenceRow(item, onClick = { onOpenEvent(item.eventId) })
                if (index != dayItems.lastIndex) HairlineDivider(modifier = Modifier.padding(start = 72.dp))
            }
        }
        if (upcoming.any { it.recurrenceId.take(8) != selectedDate.toString().replace("-", "") }) {
            Spacer(Modifier.height(22.dp))
            SectionLabel("Rest of ${monthTitle(visibleMonth)}")
            Spacer(Modifier.height(6.dp))
            upcoming
                .filter { it.recurrenceId.take(8) != selectedDate.toString().replace("-", "") }
                .take(12)
                .forEachIndexed { index, item ->
                    OccurrenceRow(item, showDate = true, onClick = { onOpenEvent(item.eventId) })
                    if (index != 11) HairlineDivider(modifier = Modifier.padding(start = 72.dp))
                }
        }
    }
}

@Composable
fun EventDetailScreen(
    event: FoyerEvent,
    calendar: FoyerCalendar?,
    onBack: () -> Unit,
    onEdit: () -> Unit,
    onDelete: () -> Unit,
) {
    var confirmingDelete by rememberSaveable { mutableStateOf(false) }
    FoyerScreen {
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(horizontal = 24.dp),
        ) {
            NestedScreenHeader(title = "Event", onBack = onBack)
            HairlineDivider()
            Column(
                modifier = Modifier
                    .weight(1f)
                    .verticalScroll(rememberScrollState())
                    .padding(top = 22.dp, bottom = 28.dp),
                verticalArrangement = Arrangement.spacedBy(18.dp),
            ) {
                Text(event.summary, style = MaterialTheme.typography.headlineMedium, color = FoyerText)
                DetailBlock("When", eventWhenLabel(event))
                DetailBlock("Repeats", recurrenceSummary(event.rrule))
                if (event.location.isNotBlank()) DetailBlock("Where", event.location)
                DetailBlock("Calendar", calendar?.displayName ?: "Unknown calendar")
                if (event.description.isNotBlank()) {
                    DetailBlock("Notes", event.description)
                }
                RecurrenceChip(event)
                Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                    GhostButton("Edit", onClick = onEdit)
                    GhostButton("Delete", onClick = { confirmingDelete = true })
                }
            }
        }
    }
    if (confirmingDelete) {
        ConfirmDeleteDialog(
            title = "Delete this event?",
            body = if (event.isRecurring) {
                "This deletes the whole series. Individual instance exceptions are not split out."
            } else {
                "This removes the event from every Foyer replica after the server write succeeds."
            },
            onConfirm = {
                confirmingDelete = false
                onDelete()
            },
            onDismiss = { confirmingDelete = false },
        )
    }
}

@Composable
fun EventEditorScreen(
    event: FoyerEvent?,
    calendars: List<FoyerCalendar>,
    initialCalendarId: String?,
    status: CalendarStatus = CalendarStatus(loading = false),
    saving: Boolean = false,
    saveError: String? = null,
    onCancel: () -> Unit,
    onSave: (EventDraft) -> Unit,
) {
    val seed = event?.let(::readEditorSeed) ?: EditorSeed()
    var summary by rememberSaveable(event?.id) { mutableStateOf(event?.summary.orEmpty()) }
    var description by rememberSaveable(event?.id) { mutableStateOf(event?.description.orEmpty()) }
    var location by rememberSaveable(event?.id) { mutableStateOf(event?.location.orEmpty()) }
    var allDay by rememberSaveable(event?.id) { mutableStateOf(event?.allDay == true) }
    var date by rememberSaveable(event?.id) { mutableStateOf(seed.date) }
    var startTime by rememberSaveable(event?.id) { mutableStateOf(seed.start) }
    var endTime by rememberSaveable(event?.id) { mutableStateOf(seed.end) }
    var tzid by rememberSaveable(event?.id) { mutableStateOf(event?.tzid ?: "America/New_York") }
    var rrule by rememberSaveable(event?.id) { mutableStateOf(event?.rrule.orEmpty()) }
    var calendarId by rememberSaveable(event?.id) {
        mutableStateOf(event?.calendarId ?: initialCalendarId ?: calendars.firstOrNull()?.id.orEmpty())
    }
    val canSave = !saving && summary.isNotBlank() && calendarId.isNotBlank() && date.isNotBlank()

    FoyerScreen {
        Column(
            modifier = Modifier
                .fillMaxSize()
                .imePadding()
                .padding(horizontal = 24.dp),
        ) {
            NestedScreenHeader(title = if (event == null) "New event" else "Edit event", onBack = onCancel)
            HairlineDivider()
            Column(
                modifier = Modifier
                    .weight(1f)
                    .verticalScroll(rememberScrollState())
                    .padding(top = 18.dp, bottom = 28.dp),
                verticalArrangement = Arrangement.spacedBy(16.dp),
            ) {
                CalendarStatusBanner(status)
                FieldLabel("Title")
                CalendarField(summary, { summary = it }, placeholder = "What’s happening")
                FieldLabel("Calendar")
                CalendarPicker(calendars, calendarId, onSelect = { id -> if (id != null) calendarId = id })
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text("All day", style = MaterialTheme.typography.bodyMedium, color = FoyerText, modifier = Modifier.weight(1f))
                    Switch(
                        checked = allDay,
                        onCheckedChange = { allDay = it },
                        colors = SwitchDefaults.colors(checkedThumbColor = FoyerText, checkedTrackColor = FoyerLine),
                    )
                }
                FieldLabel("Date")
                CalendarField(date, { date = it }, placeholder = "2026-03-15")
                if (!allDay) {
                    Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                        Column(modifier = Modifier.weight(1f)) {
                            FieldLabel("Starts")
                            CalendarField(startTime, { startTime = it }, placeholder = "10:00")
                        }
                        Column(modifier = Modifier.weight(1f)) {
                            FieldLabel("Ends")
                            CalendarField(endTime, { endTime = it }, placeholder = "10:30")
                        }
                    }
                    FieldLabel("Time zone")
                    CalendarField(tzid, { tzid = it }, placeholder = "America/New_York")
                }
                FieldLabel("Repeats")
                RecurrenceEditor(rrule, onChange = { rrule = it })
                FieldLabel("Location")
                CalendarField(location, { location = it }, placeholder = "Optional")
                FieldLabel("Notes")
                CalendarField(description, { description = it }, placeholder = "Kept exactly, including blank lines", singleLine = false)
                saveError?.let { Text(it, style = MaterialTheme.typography.bodySmall, color = FoyerTextMuted) }
                GhostButton(if (saving) "Saving…" else "Save", enabled = canSave) {
                    val localDate = runCatching { LocalDate.parse(date) }.getOrNull() ?: return@GhostButton
                    val start = parseClock(startTime) ?: LocalTime.of(10, 0)
                    val end = parseClock(endTime) ?: start.plusHours(1)
                    onSave(
                        EventDraft(
                            summary = summary.trim(),
                            description = description,
                            location = location.trim(),
                            allDay = allDay,
                            dtstart = formatStamp(localDate, start, allDay),
                            dtend = formatStamp(localDate, end, allDay).let { stamp ->
                                if (allDay) formatStamp(localDate.plusDays(1), null, true) else stamp
                            },
                            tzid = if (allDay) null else tzid.trim().ifBlank { null },
                            rrule = rrule.trim().ifBlank { null },
                            exdates = event?.let { parseExdates(it.exdates) }.orEmpty(),
                            calendarId = calendarId,
                        ),
                    )
                }
            }
        }
    }
}

@Composable
fun CalendarPicker(
    calendars: List<FoyerCalendar>,
    selectedId: String?,
    onSelect: (String?) -> Unit,
    modifier: Modifier = Modifier,
) {
    Column(modifier = modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        if (calendars.isEmpty()) {
            Text("No calendars yet", style = MaterialTheme.typography.bodySmall, color = FoyerTextMuted)
            return
        }
        calendars.forEach { calendar ->
            val selected = calendar.id == selectedId
            Surface(
                modifier = Modifier
                    .fillMaxWidth()
                    .clickable { onSelect(calendar.id) },
                shape = RoundedCornerShape(14.dp),
                color = FoyerBlack,
                border = BorderStroke(1.dp, if (selected) FoyerTextDim else FoyerLine),
            ) {
                Row(
                    modifier = Modifier.padding(horizontal = 14.dp, vertical = 12.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Box(
                        modifier = Modifier
                            .size(8.dp)
                            .clip(CircleShape)
                            .background(if (selected) FoyerText else FoyerTextDim),
                    )
                    Spacer(Modifier.width(10.dp))
                    Column(modifier = Modifier.weight(1f)) {
                        Text(calendar.displayName, style = MaterialTheme.typography.bodyMedium, color = FoyerText)
                        if (calendar.description.isNotBlank()) {
                            Text(calendar.description, style = MaterialTheme.typography.bodySmall, color = FoyerTextMuted, maxLines = 1, overflow = TextOverflow.Ellipsis)
                        }
                    }
                }
            }
        }
    }
}

@Composable
fun ConfirmDeleteDialog(
    title: String,
    body: String,
    confirmLabel: String = "Delete",
    onConfirm: () -> Unit,
    onDismiss: () -> Unit,
) {
    AlertDialog(
        onDismissRequest = onDismiss,
        containerColor = FoyerSurface,
        title = { Text(title, color = FoyerText, style = MaterialTheme.typography.titleMedium) },
        text = { Text(body, color = FoyerTextMuted, style = MaterialTheme.typography.bodyMedium) },
        confirmButton = {
            TextButton(onClick = onConfirm) {
                Text(confirmLabel, color = FoyerText)
            }
        },
        dismissButton = {
            TextButton(onClick = onDismiss) {
                Text("Cancel", color = FoyerTextMuted)
            }
        },
    )
}

@Composable
fun RecurrenceChip(event: FoyerEvent) {
    Surface(
        shape = RoundedCornerShape(16.dp),
        color = FoyerBlack,
        border = BorderStroke(1.dp, FoyerLine),
    ) {
        Text(
            text = recurrenceSummary(event.rrule),
            style = MaterialTheme.typography.labelMedium,
            color = FoyerTextMuted,
            modifier = Modifier.padding(horizontal = 12.dp, vertical = 7.dp),
        )
    }
}

@Composable
private fun RecurrenceEditor(value: String, onChange: (String) -> Unit) {
    val presets = listOf(
        "" to "Does not repeat",
        "FREQ=DAILY" to "Daily",
        "FREQ=WEEKLY;BYDAY=MO" to "Weekly on Monday",
        "FREQ=WEEKLY;BYDAY=MO,WE,FR" to "Monday, Wednesday, Friday",
        "FREQ=MONTHLY;BYDAY=2TU" to "Monthly on the second Tuesday",
        "FREQ=YEARLY" to "Yearly",
    )
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        presets.forEach { (rule, label) ->
            val selected = value == rule
            Text(
                text = label,
                style = MaterialTheme.typography.bodySmall,
                color = if (selected) FoyerText else FoyerTextMuted,
                modifier = Modifier
                    .fillMaxWidth()
                    .border(1.dp, if (selected) FoyerTextDim else FoyerLine, RoundedCornerShape(12.dp))
                    .clickable { onChange(rule) }
                    .padding(horizontal = 12.dp, vertical = 9.dp),
            )
        }
        CalendarField(value, onChange, placeholder = "FREQ=WEEKLY;BYDAY=MO")
    }
}

@Composable
private fun MonthBoard(
    month: LocalDate,
    selected: LocalDate,
    marked: Set<String>,
    onSelectDate: (LocalDate) -> Unit,
    onShiftMonth: (Long) -> Unit,
) {
    Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text("‹", color = FoyerText, modifier = Modifier.clickable { onShiftMonth(-1) }.padding(8.dp))
            Text(monthTitle(month), style = MaterialTheme.typography.titleMedium, color = FoyerText)
            Text("›", color = FoyerText, modifier = Modifier.clickable { onShiftMonth(1) }.padding(8.dp))
        }
        Row(modifier = Modifier.fillMaxWidth()) {
            listOf("M", "T", "W", "T", "F", "S", "S").forEach { label ->
                Text(
                    label,
                    modifier = Modifier.weight(1f),
                    textAlign = TextAlign.Center,
                    style = MaterialTheme.typography.labelSmall,
                    color = FoyerTextDim,
                )
            }
        }
        monthCells(month).chunked(7).forEach { week ->
            Row(modifier = Modifier.fillMaxWidth()) {
                week.forEach { date ->
                    Box(
                        modifier = Modifier
                            .weight(1f)
                            .height(40.dp)
                            .clickable(enabled = date != null) { date?.let(onSelectDate) },
                        contentAlignment = Alignment.Center,
                    ) {
                        if (date != null) {
                            val isSelected = date == selected
                            val hasEvent = marked.contains(date.toString().replace("-", ""))
                            Box(
                                modifier = Modifier
                                    .size(30.dp)
                                    .clip(CircleShape)
                                    .background(if (isSelected) FoyerText else FoyerBlack),
                                contentAlignment = Alignment.Center,
                            ) {
                                Text(
                                    text = date.dayOfMonth.toString(),
                                    style = MaterialTheme.typography.bodySmall,
                                    color = if (isSelected) FoyerBlack else FoyerText,
                                    fontWeight = if (hasEvent) FontWeight.Medium else FontWeight.Normal,
                                )
                            }
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun OccurrenceRow(
    item: EventOccurrence,
    showDate: Boolean = false,
    onClick: () -> Unit,
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .clickable(onClick = onClick)
            .padding(vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(modifier = Modifier.width(72.dp)) {
            Text(
                text = if (showDate) item.recurrenceId.take(8).let {
                    runCatching { LocalDate.parse(it, java.time.format.DateTimeFormatter.BASIC_ISO_DATE) }
                        .getOrNull()
                        ?.let(::weekdayLabel)
                        ?: it
                } else {
                    occurrenceTimeLabel(item)
                },
                style = MaterialTheme.typography.labelMedium,
                color = FoyerTextMuted,
            )
            if (showDate) {
                Text(occurrenceTimeLabel(item), style = MaterialTheme.typography.bodySmall, color = FoyerTextDim)
            }
        }
        Column(modifier = Modifier.weight(1f)) {
            Text(item.summary, style = MaterialTheme.typography.bodyMedium, color = FoyerText)
            val detail = listOfNotNull(
                item.location.takeIf { it.isNotBlank() },
                if (item.isRecurring) recurrenceSummary(null).takeIf { false } else null,
                if (item.isRecurring) "Repeats" else null,
            ).joinToString(" · ")
            if (detail.isNotBlank()) {
                Text(detail, style = MaterialTheme.typography.bodySmall, color = FoyerTextMuted, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
        }
    }
}

@Composable
private fun DetailBlock(label: String, value: String) {
    Column(verticalArrangement = Arrangement.spacedBy(3.dp)) {
        SectionLabel(label)
        Text(value, style = MaterialTheme.typography.bodyLarge, color = FoyerText)
    }
}

@Composable
private fun FieldLabel(text: String) {
    Text(text.uppercase(), style = MaterialTheme.typography.labelSmall, color = FoyerTextDim)
}

@Composable
private fun CalendarField(
    value: String,
    onChange: (String) -> Unit,
    placeholder: String,
    singleLine: Boolean = true,
) {
    BasicTextField(
        value = value,
        onValueChange = onChange,
        singleLine = singleLine,
        textStyle = MaterialTheme.typography.bodyLarge.copy(color = FoyerText),
        cursorBrush = SolidColor(FoyerText),
        decorationBox = { inner ->
            Box(
                modifier = Modifier
                    .fillMaxWidth()
                    .border(1.dp, FoyerLine, RoundedCornerShape(14.dp))
                    .padding(horizontal = 14.dp, vertical = if (singleLine) 12.dp else 14.dp),
            ) {
                if (value.isEmpty()) {
                    Text(placeholder, style = MaterialTheme.typography.bodyLarge, color = FoyerTextDim)
                }
                inner()
            }
        },
    )
}

@Composable
private fun GhostButton(label: String, enabled: Boolean = true, onClick: () -> Unit) {
    Surface(
        modifier = Modifier.clickable(enabled = enabled, onClick = onClick),
        shape = RoundedCornerShape(16.dp),
        color = FoyerBlack,
        border = BorderStroke(1.dp, FoyerLine),
    ) {
        Text(
            text = label,
            style = MaterialTheme.typography.labelMedium,
            color = if (enabled) FoyerText else FoyerTextDim,
            modifier = Modifier.padding(horizontal = 14.dp, vertical = 9.dp),
        )
    }
}

@Composable
fun CalendarStatusBanner(status: CalendarStatus, modifier: Modifier = Modifier) {
    val banner = status.banner() ?: return
    val (title, message) = when (banner) {
        is CalendarSyncBanner.Offline -> "Offline" to if (banner.pendingUploads == 0) {
            "Reading the local replica. New changes upload when Foyer Server is reachable."
        } else {
            "${banner.pendingUploads} change(s) are queued and will upload when you are back online."
        }
        is CalendarSyncBanner.Pending -> "Pending sync" to
            "${banner.pendingUploads} change(s) are waiting to upload to Foyer Server."
        is CalendarSyncBanner.StaleEtag -> "Stale copy" to banner.message
        is CalendarSyncBanner.Error -> "Couldn’t sync" to banner.message
    }
    Surface(
        modifier = modifier.fillMaxWidth().padding(bottom = 12.dp),
        shape = RoundedCornerShape(14.dp),
        color = FoyerBlack,
        border = BorderStroke(1.dp, FoyerLine),
    ) {
        Column(modifier = Modifier.padding(horizontal = 14.dp, vertical = 12.dp)) {
            Text(title, style = MaterialTheme.typography.labelMedium, color = FoyerText)
            Spacer(Modifier.height(3.dp))
            Text(message, style = MaterialTheme.typography.bodySmall, color = FoyerTextMuted)
        }
    }
}

private data class EditorSeed(val date: String = LocalDate.now().toString(), val start: String = "10:00", val end: String = "11:00")

private fun readEditorSeed(event: FoyerEvent): EditorSeed {
    val digits = event.dtstart.filter(Char::isDigit)
    val date = runCatching {
        LocalDate.parse(digits.take(8), java.time.format.DateTimeFormatter.BASIC_ISO_DATE).toString()
    }.getOrDefault(LocalDate.now().toString())
    if (event.allDay || digits.length < 14) return EditorSeed(date = date)
    val start = "${digits.substring(8, 10)}:${digits.substring(10, 12)}"
    val endDigits = event.dtend?.filter(Char::isDigit).orEmpty()
    val end = if (endDigits.length >= 14) {
        "${endDigits.substring(8, 10)}:${endDigits.substring(10, 12)}"
    } else {
        "11:00"
    }
    return EditorSeed(date, start, end)
}

private fun parseClock(value: String): LocalTime? {
    val parts = value.trim().split(':')
    if (parts.size < 2) return null
    val hour = parts[0].toIntOrNull() ?: return null
    val minute = parts[1].take(2).toIntOrNull() ?: return null
    return runCatching { LocalTime.of(hour, minute) }.getOrNull()
}
