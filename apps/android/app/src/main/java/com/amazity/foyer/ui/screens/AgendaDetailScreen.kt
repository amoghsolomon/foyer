package com.amazity.foyer.ui.screens

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.DatePicker
import androidx.compose.material3.DatePickerDialog
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TimePicker
import androidx.compose.material3.rememberDatePickerState
import androidx.compose.material3.rememberTimePickerState
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.unit.dp
import com.amazity.foyer.model.AgendaDay
import com.amazity.foyer.model.AgendaItem
import com.amazity.foyer.model.TodoItem
import com.amazity.foyer.ui.components.HairlineDivider
import com.amazity.foyer.ui.theme.FoyerBlack
import com.amazity.foyer.ui.theme.FoyerLine
import com.amazity.foyer.ui.theme.FoyerText
import com.amazity.foyer.ui.theme.FoyerTextDim
import java.time.Instant
import java.time.LocalDate
import java.time.LocalDateTime
import java.time.LocalTime
import java.time.ZoneId
import java.time.ZoneOffset
import java.time.format.DateTimeFormatter

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun AgendaDetailSheet(
    agendaItem: AgendaItem?,
    todoItem: TodoItem?,
    onDismiss: () -> Unit,
    onSaveAgenda: (AgendaItem) -> Unit,
    onSaveTodo: (TodoItem) -> Unit,
    onDelete: () -> Unit,
) {
    val originalTitle = agendaItem?.title ?: todoItem?.title.orEmpty()
    val initialDateTime = remember(agendaItem?.id) { initialDateTime(agendaItem) }
    val initialSelectedDate = remember(agendaItem?.id, todoItem?.id) {
        agendaItem?.let { initialDateTime.toLocalDate() }
            ?: todoItem?.dueAt?.take(10)?.let { runCatching { LocalDate.parse(it) }.getOrNull() }
            ?: LocalDate.now()
    }
    val eventDurationMillis = remember(agendaItem?.id) {
        val start = agendaItem?.startsAtEpochMillis
        val end = agendaItem?.endsAtEpochMillis
        if (start != null && end != null && end > start) end - start else 60 * 60 * 1_000L
    }
    var title by rememberSaveable(agendaItem?.id, todoItem?.id) { mutableStateOf(originalTitle) }
    var description by rememberSaveable(agendaItem?.id, todoItem?.id) {
        mutableStateOf(agendaItem?.detail ?: todoItem?.description.orEmpty())
    }
    var dateEpochDay by rememberSaveable(agendaItem?.id, todoItem?.id) {
        mutableLongStateOf(initialSelectedDate.toEpochDay())
    }
    var hour by rememberSaveable(agendaItem?.id) { mutableStateOf(initialDateTime.hour) }
    var minute by rememberSaveable(agendaItem?.id) { mutableStateOf(initialDateTime.minute) }
    var completed by rememberSaveable(todoItem?.id) { mutableStateOf(todoItem?.completed == true) }
    var hasDueDate by rememberSaveable(todoItem?.id) { mutableStateOf(todoItem?.dueAt != null) }
    var showDatePicker by rememberSaveable { mutableStateOf(false) }
    var showTimePicker by rememberSaveable { mutableStateOf(false) }
    val isAgenda = agendaItem != null
    val selectedDate = LocalDate.ofEpochDay(dateEpochDay)
    val selectedTime = LocalTime.of(hour, minute)
    val datePickerState = rememberDatePickerState(
        initialSelectedDateMillis = selectedDate
            .atStartOfDay(ZoneOffset.UTC)
            .toInstant()
            .toEpochMilli(),
    )
    val timePickerState = rememberTimePickerState(initialHour = hour, initialMinute = minute)
    val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    val save: () -> Unit = {
        if (isAgenda) {
            val dateTime = LocalDateTime.of(selectedDate, selectedTime)
            val startsAtEpochMillis = dateTime
                .atZone(ZoneId.systemDefault())
                .toInstant()
                .toEpochMilli()
            onSaveAgenda(
                agendaItem!!.copy(
                    title = title.trim(),
                    detail = description.trim().ifBlank { null },
                    time = selectedTime.format(timeFormatter),
                    day = agendaDay(selectedDate),
                    startsAtEpochMillis = startsAtEpochMillis,
                    endsAtEpochMillis = startsAtEpochMillis + eventDurationMillis,
                ),
            )
        } else {
            onSaveTodo(
                todoItem!!.copy(
                    title = title.trim(),
                    completed = completed,
                    description = description.trim().ifBlank { null },
                    dueAt = selectedDate.toString().takeIf { hasDueDate },
                ),
            )
        }
    }

    ModalBottomSheet(
        onDismissRequest = onDismiss,
        sheetState = sheetState,
        containerColor = FoyerBlack,
    ) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .fillMaxHeight(0.92f)
                .imePadding()
                .padding(horizontal = 24.dp),
        ) {
            Row(
                modifier = Modifier.fillMaxWidth().height(68.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.SpaceBetween,
            ) {
                Spacer(Modifier.width(40.dp))
                Text(
                    if (isAgenda) "Agenda item" else "To do",
                    style = MaterialTheme.typography.titleMedium,
                    color = FoyerText,
                )
                Spacer(Modifier.width(40.dp))
            }
            HairlineDivider()
            Spacer(Modifier.height(24.dp))
            EditorField("TITLE", title, { title = it })
            Spacer(Modifier.height(22.dp))
            EditorField("DESCRIPTION", description, { description = it }, minHeight = 72.dp)
            if (isAgenda) {
                Spacer(Modifier.height(18.dp))
                PickerRow(
                    label = "DATE",
                    value = selectedDate.format(dateFormatter),
                    onClick = { showDatePicker = true },
                )
                HairlineDivider()
                PickerRow(
                    label = "TIME",
                    value = selectedTime.format(timeFormatter),
                    onClick = { showTimePicker = true },
                )
                HairlineDivider()
            } else {
                Spacer(Modifier.height(18.dp))
                PickerRow(
                    label = "DUE DATE",
                    value = if (hasDueDate) selectedDate.format(dateFormatter) else "No due date",
                    onClick = {
                        hasDueDate = true
                        showDatePicker = true
                    },
                )
                if (hasDueDate) {
                    Text(
                        text = "Remove due date",
                        style = MaterialTheme.typography.labelMedium,
                        color = FoyerTextDim,
                        modifier = Modifier.clickable { hasDueDate = false }.padding(vertical = 8.dp),
                    )
                }
                HairlineDivider()
                PickerRow(
                    label = "STATUS",
                    value = if (completed) "Completed" else "Open",
                    onClick = { completed = !completed },
                )
                HairlineDivider()
            }
            Spacer(Modifier.weight(1f))
            HairlineDivider()
            Row(
                modifier = Modifier.fillMaxWidth().padding(vertical = 12.dp),
                horizontalArrangement = Arrangement.spacedBy(10.dp),
            ) {
                SheetActionButton("Delete", filled = false, enabled = true, onClick = onDelete, modifier = Modifier.weight(1f))
                SheetActionButton("Save", filled = true, enabled = title.isNotBlank(), onClick = save, modifier = Modifier.weight(1f))
            }
        }
    }

    if (showDatePicker) {
        DatePickerDialog(
            onDismissRequest = { showDatePicker = false },
            confirmButton = {
                DialogAction("Done") {
                    datePickerState.selectedDateMillis?.let { millis ->
                        dateEpochDay = Instant.ofEpochMilli(millis)
                            .atZone(ZoneOffset.UTC)
                            .toLocalDate()
                            .toEpochDay()
                    }
                    showDatePicker = false
                }
            },
            dismissButton = { DialogAction("Cancel") { showDatePicker = false } },
        ) {
            DatePicker(state = datePickerState)
        }
    }

    if (showTimePicker) {
        AlertDialog(
            onDismissRequest = { showTimePicker = false },
            confirmButton = {
                DialogAction("Done") {
                    hour = timePickerState.hour
                    minute = timePickerState.minute
                    showTimePicker = false
                }
            },
            dismissButton = { DialogAction("Cancel") { showTimePicker = false } },
            text = { TimePicker(state = timePickerState) },
        )
    }
}

@Composable
private fun SheetActionButton(
    label: String,
    filled: Boolean,
    enabled: Boolean,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
) {
    Surface(
        modifier = modifier.height(50.dp).clickable(enabled = enabled, onClick = onClick),
        shape = RoundedCornerShape(25.dp),
        color = if (filled && enabled) FoyerText else FoyerBlack,
        contentColor = if (filled && enabled) FoyerBlack else if (enabled) FoyerText else FoyerTextDim,
        border = BorderStroke(1.dp, if (filled && enabled) FoyerText else FoyerLine),
    ) {
        Box(contentAlignment = Alignment.Center) {
            Text(label, style = MaterialTheme.typography.labelMedium)
        }
    }
}

@Composable
private fun EditorField(
    label: String,
    value: String,
    onValueChange: (String) -> Unit,
    minHeight: androidx.compose.ui.unit.Dp = 24.dp,
) {
    Column(verticalArrangement = Arrangement.spacedBy(7.dp)) {
        Text(label, style = MaterialTheme.typography.labelSmall, color = FoyerTextDim)
        BasicTextField(
            value = value,
            onValueChange = onValueChange,
            textStyle = MaterialTheme.typography.bodyLarge.copy(color = FoyerText),
            cursorBrush = SolidColor(FoyerText),
            modifier = Modifier.fillMaxWidth().height(minHeight),
        )
        Spacer(Modifier.height(5.dp))
        HairlineDivider()
    }
}

@Composable
private fun PickerRow(label: String, value: String, onClick: () -> Unit) {
    Row(
        modifier = Modifier.fillMaxWidth().clickable(onClick = onClick).padding(vertical = 13.dp),
        horizontalArrangement = Arrangement.SpaceBetween,
    ) {
        Text(label, style = MaterialTheme.typography.labelSmall, color = FoyerTextDim)
        Text(value, style = MaterialTheme.typography.bodyMedium, color = FoyerText)
    }
}

@Composable
private fun DialogAction(label: String, onClick: () -> Unit) {
    Text(
        text = label,
        style = MaterialTheme.typography.labelMedium,
        color = FoyerText,
        modifier = Modifier.clickable(onClick = onClick).padding(16.dp),
    )
}

private fun initialDateTime(item: AgendaItem?): LocalDateTime {
    item?.startsAtEpochMillis?.let { millis ->
        return Instant.ofEpochMilli(millis).atZone(ZoneId.systemDefault()).toLocalDateTime()
    }
    val date = when (item?.day) {
        AgendaDay.Tomorrow -> LocalDate.now().plusDays(1)
        AgendaDay.Upcoming -> LocalDate.now().plusDays(2)
        else -> LocalDate.now()
    }
    val time = item?.time?.let(::parseTime) ?: LocalTime.of(9, 0)
    return LocalDateTime.of(date, time)
}

private fun parseTime(value: String): LocalTime = runCatching {
    LocalTime.parse(value, DateTimeFormatter.ofPattern("H:mm"))
}.recoverCatching {
    LocalTime.parse(value, DateTimeFormatter.ofPattern("h:mm a"))
}.getOrDefault(LocalTime.of(9, 0))

private fun agendaDay(date: LocalDate): AgendaDay = when (date) {
    LocalDate.now() -> AgendaDay.Today
    LocalDate.now().plusDays(1) -> AgendaDay.Tomorrow
    else -> AgendaDay.Upcoming
}

private val dateFormatter = DateTimeFormatter.ofPattern("EEE, d MMM yyyy")
private val timeFormatter = DateTimeFormatter.ofPattern("h:mm a")
