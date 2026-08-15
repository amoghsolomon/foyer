package com.amazity.foyer.ui.screens

import androidx.compose.animation.animateContentSize
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.DatePicker
import androidx.compose.material3.DatePickerDialog
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.IconButton
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TimePicker
import androidx.compose.material3.rememberDatePickerState
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.material3.rememberTimePickerState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.amazity.foyer.model.AgentTask
import com.amazity.foyer.model.ChatMessage
import com.amazity.foyer.model.ChatMessageState
import com.amazity.foyer.model.ChatRole
import com.amazity.foyer.ui.components.BackGlyph
import com.amazity.foyer.ui.components.ExpandGlyph
import com.amazity.foyer.ui.components.FoyerScreen
import com.amazity.foyer.ui.components.HairlineDivider
import com.amazity.foyer.ui.components.MoreGlyph
import com.amazity.foyer.ui.components.RetryGlyph
import com.amazity.foyer.ui.components.RichAssistantMessage
import com.amazity.foyer.ui.components.AssistantReadAloudButton
import com.amazity.foyer.ui.components.RunNowGlyph
import com.amazity.foyer.ui.components.ScheduleGlyph
import com.amazity.foyer.ui.components.TimeGlyph
import com.amazity.foyer.ui.theme.FoyerBlack
import com.amazity.foyer.ui.theme.FoyerLine
import com.amazity.foyer.ui.theme.FoyerSurfaceRaised
import com.amazity.foyer.ui.theme.FoyerText
import com.amazity.foyer.ui.theme.FoyerTextDim
import com.amazity.foyer.ui.theme.FoyerTextMuted
import java.time.LocalDateTime
import java.time.Instant
import java.time.LocalTime
import java.time.ZoneId
import java.time.ZoneOffset
import java.time.format.DateTimeFormatter
import com.amazity.foyer.voice.MoonshineKokoroReadAloud
import com.amazity.foyer.voice.ReadAloudState

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ActivityChatScreen(
    task: AgentTask,
    messages: List<ChatMessage>,
    onBack: () -> Unit,
    onSendMessage: (String) -> Unit,
    onSchedule: (String, String, Int, String) -> Unit,
    onCancelSchedule: () -> Unit,
    onRename: (String) -> Unit,
    onDelete: () -> Unit,
    onRunNow: () -> Unit,
    onRetry: (String) -> Unit,
    readAloud: MoonshineKokoroReadAloud,
    readAloudState: ReadAloudState,
    activeReadAloudMessageId: String?,
    onToggleReadAloud: (String, String) -> Unit,
    modifier: Modifier = Modifier,
) {
    var draft by rememberSaveable(task.id) { mutableStateOf("") }
    var showScheduleSheet by rememberSaveable(task.id) { mutableStateOf(false) }
    var showActions by remember { mutableStateOf(false) }
    var showRenameDialog by rememberSaveable(task.id) { mutableStateOf(false) }
    var showDeleteDialog by rememberSaveable(task.id) { mutableStateOf(false) }
    var renameDraft by rememberSaveable(task.id) { mutableStateOf(task.title) }
    val listState = rememberLazyListState()
    val submitMessage = {
        val message = draft.trim()
        if (message.isNotEmpty()) {
            onSendMessage(message)
            draft = ""
        }
    }

    DisposableEffect(readAloud, task.id) {
        onDispose { readAloud.stop() }
    }

    LaunchedEffect(messages.size) {
        if (messages.isNotEmpty()) {
            listState.animateScrollToItem(messages.lastIndex)
        }
    }

    FoyerScreen(modifier = modifier) {
        Column(
            modifier = Modifier
                .fillMaxSize()
                .imePadding(),
        ) {
            ActivityChatHeader(
                task = task,
                onBack = onBack,
                onScheduleClick = { showScheduleSheet = true },
                onCancelSchedule = onCancelSchedule,
                onRunNow = onRunNow,
                showActions = showActions,
                onShowActions = { showActions = it },
                onRename = {
                    renameDraft = task.title
                    showRenameDialog = true
                },
                onDelete = { showDeleteDialog = true },
                modifier = Modifier.padding(horizontal = 20.dp),
            )
            HairlineDivider()

            if (task.kind == "job") {
                JobDefinitionAccordion(
                    task = task,
                    onRetry = onRetry,
                )
                HairlineDivider()
            }

            LazyColumn(
                state = listState,
                modifier = Modifier
                    .fillMaxWidth()
                    .weight(1f),
                contentPadding = PaddingValues(horizontal = 20.dp, vertical = 16.dp),
                verticalArrangement = Arrangement.spacedBy(16.dp),
            ) {
                items(messages, key = ChatMessage::id) { message ->
                    val readAloudId = "activity:${task.id}:${message.id}"
                    ChatMessageBubble(
                        message = message,
                        readAloudActive = activeReadAloudMessageId == readAloudId &&
                            readAloudState.isActiveReadAloud(),
                        onReadAloud = { onToggleReadAloud(readAloudId, message.content) },
                    )
                }
            }

            ChatComposer(
                value = draft,
                onValueChange = { draft = it },
                onSend = submitMessage,
                modifier = Modifier.padding(horizontal = 20.dp, vertical = 12.dp),
            )
        }
    }

    if (showRenameDialog) {
        AlertDialog(
            onDismissRequest = { showRenameDialog = false },
            title = { Text("Rename thread") },
            text = {
                BasicTextField(
                    value = renameDraft,
                    onValueChange = { renameDraft = it },
                    textStyle = MaterialTheme.typography.bodyMedium.copy(color = FoyerText),
                    cursorBrush = SolidColor(FoyerText),
                    modifier = Modifier
                        .fillMaxWidth()
                        .border(1.dp, FoyerLine, RoundedCornerShape(12.dp))
                        .padding(12.dp),
                )
            },
            confirmButton = {
                TextButton(
                    enabled = renameDraft.isNotBlank(),
                    onClick = {
                        onRename(renameDraft.trim())
                        showRenameDialog = false
                    },
                ) { Text("Save") }
            },
            dismissButton = {
                TextButton(onClick = { showRenameDialog = false }) { Text("Cancel") }
            },
        )
    }

    if (showDeleteDialog) {
        AlertDialog(
            onDismissRequest = { showDeleteDialog = false },
            title = { Text("Delete this thread?") },
            text = { Text("Its messages, job definition, schedule, and run history will be permanently removed from Foyer.") },
            confirmButton = {
                TextButton(onClick = { showDeleteDialog = false; onDelete() }) { Text("Delete") }
            },
            dismissButton = {
                TextButton(onClick = { showDeleteDialog = false }) { Text("Cancel") }
            },
        )
    }

    if (showScheduleSheet && task.kind == "job") {
        ScheduleSheet(
            task = task,
            onDismiss = { showScheduleSheet = false },
            onSave = { runAt, frequency, interval, timezone ->
                onSchedule(runAt, frequency, interval, timezone)
                showScheduleSheet = false
            },
        )
    }
}

@Composable
private fun ActivityChatHeader(
    task: AgentTask,
    onBack: () -> Unit,
    onScheduleClick: () -> Unit,
    onCancelSchedule: () -> Unit,
    onRunNow: () -> Unit,
    showActions: Boolean,
    onShowActions: (Boolean) -> Unit,
    onRename: () -> Unit,
    onDelete: () -> Unit,
    modifier: Modifier = Modifier,
) {
    Row(
        modifier = modifier
            .fillMaxWidth()
            .height(70.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(
            modifier = Modifier
                .size(40.dp)
                .clip(CircleShape)
                .clickable(onClick = onBack),
            contentAlignment = Alignment.CenterStart,
        ) {
            BackGlyph()
        }
        Spacer(Modifier.width(4.dp))
        Column(modifier = Modifier.weight(1f)) {
            Text(
                text = if (task.kind == "job") "JOB" else task.title,
                style = MaterialTheme.typography.titleMedium,
                color = FoyerText,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            Text(
                text = "${if (task.kind == "job") "" else "CHAT · "}${task.subtitle.substringAfter(" · ", task.subtitle)}",
                style = MaterialTheme.typography.bodySmall,
                color = FoyerTextDim,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
        if (task.kind == "job") {
            ActivityIconButton(description = "Run now", onClick = onRunNow) {
                RunNowGlyph(color = FoyerText)
            }
            ActivityIconButton(
                description = if (task.nextRunAt != null) "Edit schedule" else "Set schedule",
                onClick = onScheduleClick,
            ) {
                ScheduleGlyph(color = FoyerText)
            }
        }
        Box {
            ActivityIconButton(description = "More activity actions", onClick = { onShowActions(true) }) {
                MoreGlyph()
            }
            DropdownMenu(expanded = showActions, onDismissRequest = { onShowActions(false) }) {
                DropdownMenuItem(
                    text = { Text("Rename") },
                    onClick = { onShowActions(false); onRename() },
                )
                if (task.kind == "job" && task.nextRunAt != null) {
                    DropdownMenuItem(
                        text = { Text("Turn off schedule") },
                        onClick = { onShowActions(false); onCancelSchedule() },
                    )
                }
                DropdownMenuItem(
                    text = { Text("Delete permanently") },
                    onClick = { onShowActions(false); onDelete() },
                )
            }
        }
    }
}

@Composable
private fun ActivityIconButton(
    description: String,
    onClick: () -> Unit,
    content: @Composable () -> Unit,
) {
    IconButton(
        onClick = onClick,
        modifier = Modifier
            .size(40.dp)
            .semantics { contentDescription = description },
        content = content,
    )
}

@Composable
private fun JobDefinitionAccordion(
    task: AgentTask,
    onRetry: (String) -> Unit,
) {
    var expanded by rememberSaveable(task.id) { mutableStateOf(false) }
    Surface(
        modifier = Modifier.fillMaxWidth(),
        color = FoyerSurfaceRaised,
    ) {
        Column(modifier = Modifier.animateContentSize()) {
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .semantics {
                        role = Role.Button
                        contentDescription = if (expanded) "Collapse job definition" else "Expand job definition"
                    }
                    .clickable { expanded = !expanded }
                    .padding(horizontal = 20.dp, vertical = 14.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    text = task.title,
                    style = MaterialTheme.typography.labelMedium,
                    color = FoyerText,
                    modifier = Modifier.weight(1f),
                )
                Text(
                    text = "V${task.definitionVersion ?: 1}",
                    style = MaterialTheme.typography.labelSmall,
                    color = FoyerTextDim,
                )
                Spacer(Modifier.width(10.dp))
                ExpandGlyph(expanded = expanded)
            }
            if (expanded) {
                Column(
                    modifier = Modifier.padding(start = 20.dp, end = 20.dp, bottom = 16.dp),
                    verticalArrangement = Arrangement.spacedBy(6.dp),
                ) {
                    Text(task.jobObjective ?: task.title, style = MaterialTheme.typography.bodyMedium, color = FoyerText)
                    task.jobInstructions?.takeIf(String::isNotBlank)?.let {
                        Text(it, style = MaterialTheme.typography.bodySmall, color = FoyerTextDim)
                    }
                    task.expectedOutput?.takeIf(String::isNotBlank)?.let {
                        Text("Output: $it", style = MaterialTheme.typography.bodySmall, color = FoyerTextDim)
                    }
                    Text(task.subtitle.substringAfter(" · ", task.subtitle), style = MaterialTheme.typography.labelSmall, color = FoyerTextMuted)
                    task.latestFailedRunId?.let { runId ->
                        ActivityIconButton(description = "Retry failed run", onClick = { onRetry(runId) }) {
                            RetryGlyph(color = FoyerText)
                        }
                    }
                }
            }
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun ScheduleSheet(
    task: AgentTask,
    onDismiss: () -> Unit,
    onSave: (String, String, Int, String) -> Unit,
) {
    val timezone = remember { ZoneId.systemDefault() }
    val initialDateTime = remember(task.id, task.nextRunAt) {
        task.nextRunAt
            ?.let { runCatching { Instant.parse(it).atZone(timezone).toLocalDateTime() }.getOrNull() }
            ?: LocalDateTime.now(timezone).plusHours(1).withSecond(0).withNano(0)
    }
    val initialDateMillis = remember(initialDateTime) {
        initialDateTime.toLocalDate().atStartOfDay(ZoneOffset.UTC).toInstant().toEpochMilli()
    }
    val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    var selectedDateMillis by remember(task.id) { mutableStateOf(initialDateMillis) }
    var selectedHour by remember(task.id) { mutableStateOf(initialDateTime.hour) }
    var selectedMinute by remember(task.id) { mutableStateOf(initialDateTime.minute) }
    var showDatePicker by remember { mutableStateOf(false) }
    var showTimePicker by remember { mutableStateOf(false) }
    var frequency by remember(task.id) { mutableStateOf(task.scheduleFrequency ?: "once") }
    val dateLabel = remember(selectedDateMillis) {
        Instant.ofEpochMilli(selectedDateMillis)
            .atZone(ZoneOffset.UTC)
            .toLocalDate()
            .format(DateTimeFormatter.ofPattern("EEE, MMM d"))
    }
    val timeLabel = remember(selectedHour, selectedMinute) {
        LocalTime.of(selectedHour, selectedMinute).format(DateTimeFormatter.ofPattern("h:mm a"))
    }

    ModalBottomSheet(
        onDismissRequest = onDismiss,
        sheetState = sheetState,
        containerColor = FoyerBlack,
        contentColor = FoyerText,
    ) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .padding(start = 20.dp, end = 20.dp, bottom = 28.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Text(
                text = "Schedule",
                style = MaterialTheme.typography.titleMedium,
                color = FoyerText,
                modifier = Modifier.align(Alignment.Start),
            )
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.spacedBy(10.dp),
            ) {
                SchedulePickerField(
                    label = "Date",
                    value = dateLabel,
                    description = "Choose schedule date",
                    onClick = { showDatePicker = true },
                    modifier = Modifier.weight(1f),
                ) { ScheduleGlyph(color = FoyerText) }
                SchedulePickerField(
                    label = "Time",
                    value = timeLabel,
                    description = "Choose schedule time",
                    onClick = { showTimePicker = true },
                    modifier = Modifier.weight(1f),
                ) { TimeGlyph(color = FoyerText) }
            }
            Text(
                text = "Repeats",
                style = MaterialTheme.typography.labelSmall,
                color = FoyerTextDim,
                modifier = Modifier.align(Alignment.Start),
            )
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                listOf("once" to "Once", "daily" to "Daily", "weekly" to "Weekly").forEach { (value, label) ->
                    Text(
                        text = label,
                        style = MaterialTheme.typography.labelMedium,
                        color = if (frequency == value) FoyerBlack else FoyerText,
                        modifier = Modifier
                            .weight(1f)
                            .clip(RoundedCornerShape(18.dp))
                            .background(if (frequency == value) FoyerText else FoyerSurfaceRaised)
                            .clickable { frequency = value }
                            .padding(vertical = 9.dp),
                        textAlign = androidx.compose.ui.text.style.TextAlign.Center,
                    )
                }
            }
            Text(
                text = "Timezone: ${timezone.id}",
                style = MaterialTheme.typography.bodySmall,
                color = FoyerTextDim,
                modifier = Modifier.align(Alignment.Start),
            )
            HairlineDivider()
            Row(
                modifier = Modifier.fillMaxWidth().padding(top = 2.dp),
                horizontalArrangement = Arrangement.spacedBy(10.dp),
            ) {
                ScheduleSheetActionButton(
                    label = "Cancel",
                    filled = false,
                    onClick = onDismiss,
                    modifier = Modifier.weight(1f),
                )
                ScheduleSheetActionButton(
                    label = "Save",
                    filled = true,
                    onClick = {
                        val date = Instant.ofEpochMilli(selectedDateMillis)
                            .atZone(ZoneOffset.UTC)
                            .toLocalDate()
                        val runAt = date
                            .atTime(selectedHour, selectedMinute)
                            .atZone(timezone)
                            .toInstant()
                            .toString()
                        onSave(runAt, frequency, 1, timezone.id)
                    },
                    modifier = Modifier.weight(1f),
                )
            }
        }
    }

    if (showDatePicker) {
        val pickerState = rememberDatePickerState(initialSelectedDateMillis = selectedDateMillis)
        DatePickerDialog(
            onDismissRequest = { showDatePicker = false },
            confirmButton = {
                TextButton(onClick = {
                    pickerState.selectedDateMillis?.let { selectedDateMillis = it }
                    showDatePicker = false
                }) { Text("Done") }
            },
            dismissButton = {
                TextButton(onClick = { showDatePicker = false }) { Text("Cancel") }
            },
        ) {
            DatePicker(state = pickerState, showModeToggle = false)
        }
    }

    if (showTimePicker) {
        val pickerState = rememberTimePickerState(
            initialHour = selectedHour,
            initialMinute = selectedMinute,
            is24Hour = false,
        )
        AlertDialog(
            onDismissRequest = { showTimePicker = false },
            title = { Text("Choose a time") },
            text = { TimePicker(state = pickerState) },
            confirmButton = {
                TextButton(onClick = {
                    selectedHour = pickerState.hour
                    selectedMinute = pickerState.minute
                    showTimePicker = false
                }) { Text("Done") }
            },
            dismissButton = {
                TextButton(onClick = { showTimePicker = false }) { Text("Cancel") }
            },
        )
    }
}

@Composable
private fun SchedulePickerField(
    label: String,
    value: String,
    description: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    icon: @Composable () -> Unit,
) {
    Row(
        modifier = modifier
            .clip(RoundedCornerShape(14.dp))
            .border(1.dp, FoyerLine, RoundedCornerShape(14.dp))
            .semantics {
                role = Role.Button
                contentDescription = "$description, $value"
            }
            .clickable(onClick = onClick)
            .padding(horizontal = 12.dp, vertical = 11.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        icon()
        Column {
            Text(label, style = MaterialTheme.typography.labelSmall, color = FoyerTextDim)
            Text(value, style = MaterialTheme.typography.bodyMedium, color = FoyerText)
        }
    }
}

@Composable
private fun ScheduleSheetActionButton(
    label: String,
    filled: Boolean,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
) {
    Surface(
        modifier = modifier
            .height(50.dp)
            .clickable(onClick = onClick),
        shape = RoundedCornerShape(25.dp),
        color = if (filled) FoyerText else FoyerBlack,
        contentColor = if (filled) FoyerBlack else FoyerText,
        border = BorderStroke(1.dp, if (filled) FoyerText else FoyerLine),
    ) {
        Box(contentAlignment = Alignment.Center) {
            Text(label, style = MaterialTheme.typography.labelMedium)
        }
    }
}

@Composable
private fun ChatMessageBubble(
    message: ChatMessage,
    readAloudActive: Boolean,
    onReadAloud: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val fromUser = message.role == ChatRole.User
    val bubbleColor = if (fromUser) FoyerText else FoyerSurfaceRaised
    val messageColor = if (fromUser) FoyerBlack else FoyerText
    val alignment = if (fromUser) Alignment.End else Alignment.Start

    Column(
        modifier = modifier.fillMaxWidth(),
        horizontalAlignment = alignment,
    ) {
        Text(
            text = buildString {
                append(message.role.label.uppercase())
                append(" · ")
                append(message.timestamp)
                if (message.state == ChatMessageState.Sending) append(" · SENDING")
                if (message.state == ChatMessageState.Failed) append(" · FAILED")
            },
            style = MaterialTheme.typography.labelSmall,
            color = FoyerTextDim,
            modifier = Modifier.padding(horizontal = 4.dp, vertical = 5.dp),
        )
        Surface(
            modifier = Modifier.widthIn(max = 310.dp),
            shape = RoundedCornerShape(16.dp),
            color = bubbleColor,
            border = if (fromUser) null else BorderStroke(1.dp, FoyerLine),
        ) {
            if (fromUser) {
                Text(
                    text = message.content,
                    style = MaterialTheme.typography.bodyMedium,
                    color = messageColor,
                    modifier = Modifier.padding(horizontal = 15.dp, vertical = 12.dp),
                )
            } else {
                Column(modifier = Modifier.padding(horizontal = 15.dp, vertical = 8.dp)) {
                    RichAssistantMessage(text = message.content, color = messageColor)
                    if (message.state == ChatMessageState.Delivered) {
                        Row(modifier = Modifier.align(Alignment.End)) {
                            AssistantReadAloudButton(
                                active = readAloudActive,
                                onClick = onReadAloud,
                            )
                        }
                    }
                }
            }
        }
    }
}

private fun ReadAloudState.isActiveReadAloud(): Boolean =
    this is ReadAloudState.Preparing || this is ReadAloudState.Speaking

@Composable
private fun ChatComposer(
    value: String,
    onValueChange: (String) -> Unit,
    onSend: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val canSend = value.isNotBlank()

    Row(
        modifier = modifier
            .fillMaxWidth()
            .height(50.dp)
            .clip(RoundedCornerShape(25.dp))
            .border(1.dp, FoyerLine, RoundedCornerShape(25.dp))
            .background(FoyerBlack)
            .padding(start = 16.dp, end = 6.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        BasicTextField(
            value = value,
            onValueChange = onValueChange,
            modifier = Modifier.weight(1f),
            textStyle = MaterialTheme.typography.bodyMedium.copy(color = FoyerText),
            cursorBrush = SolidColor(FoyerText),
            singleLine = true,
            keyboardOptions = KeyboardOptions(imeAction = ImeAction.Send),
            keyboardActions = KeyboardActions(onSend = { onSend() }),
            decorationBox = { innerTextField ->
                Box(contentAlignment = Alignment.CenterStart) {
                    if (value.isEmpty()) {
                        Text(
                            text = "Message this activity",
                            style = MaterialTheme.typography.bodyMedium,
                            color = FoyerTextDim,
                        )
                    }
                    innerTextField()
                }
            },
        )
        Box(
            modifier = Modifier
                .height(38.dp)
                .clip(RoundedCornerShape(19.dp))
                .semantics { role = Role.Button }
                .clickable(enabled = canSend, onClick = onSend)
                .padding(horizontal = 12.dp),
            contentAlignment = Alignment.Center,
        ) {
            Text(
                text = "Send",
                style = MaterialTheme.typography.labelMedium,
                color = if (canSend) FoyerText else FoyerTextMuted.copy(alpha = 0.45f),
            )
        }
    }
}
