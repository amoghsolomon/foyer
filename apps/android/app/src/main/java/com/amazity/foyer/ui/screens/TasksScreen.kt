package com.amazity.foyer.ui.screens

import androidx.compose.foundation.BorderStroke
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
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.unit.dp
import com.amazity.foyer.model.TaskDue
import com.amazity.foyer.model.TasksCatalog
import com.amazity.foyer.model.TasksStatus
import com.amazity.foyer.model.TasksSyncBanner
import com.amazity.foyer.model.VaultTask
import com.amazity.foyer.model.VaultTaskList
import com.amazity.foyer.notes.SafeMarkdown
import com.amazity.foyer.ui.components.ChevronGlyph
import com.amazity.foyer.ui.components.ContentStatePanel
import com.amazity.foyer.ui.components.ErrorStatePanel
import com.amazity.foyer.ui.components.FoyerScreen
import com.amazity.foyer.ui.components.HairlineDivider
import com.amazity.foyer.ui.components.LoadingStatePanel
import com.amazity.foyer.ui.components.NestedScreenHeader
import com.amazity.foyer.ui.components.PlusGlyph
import com.amazity.foyer.ui.components.SectionLabel
import com.amazity.foyer.ui.components.TimezoneInput
import com.amazity.foyer.ui.theme.FoyerBlack
import com.amazity.foyer.ui.theme.FoyerLine
import com.amazity.foyer.ui.theme.FoyerText
import com.amazity.foyer.ui.theme.FoyerTextDim
import com.amazity.foyer.ui.theme.FoyerTextMuted

@Composable
fun TasksPage(
    catalog: TasksCatalog,
    onOpenList: (String) -> Unit,
    onOpenTask: (String) -> Unit,
    onCreateTask: () -> Unit,
    onCreateList: (String) -> Unit = {},
    isLoading: Boolean = false,
    errorMessage: String? = null,
    onRetry: () -> Unit = {},
    modifier: Modifier = Modifier,
) {
    var namingList by rememberSaveable { mutableStateOf(false) }
    Column(
        modifier = modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(top = 14.dp, bottom = 88.dp),
    ) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            SectionLabel("Lists")
            Row {
                Box(modifier = Modifier.clickable { namingList = true }.padding(8.dp)) {
                    Text("List", style = MaterialTheme.typography.labelMedium, color = FoyerTextMuted)
                }
                Box(modifier = Modifier.clickable(onClick = onCreateTask).padding(8.dp)) {
                    PlusGlyph()
                }
            }
        }
        Spacer(Modifier.height(6.dp))
        TasksStatusBanner(catalog.status)
        if (catalog.status.developmentAuth) {
            Text(
                if (catalog.status.sharingReplica) {
                    "Development session · sharing the Notes PowerSync replica"
                } else {
                    "Development session · local Foyer Server only"
                },
                style = MaterialTheme.typography.bodySmall,
                color = FoyerTextDim,
            )
            Spacer(Modifier.height(8.dp))
        }
        when {
            isLoading || catalog.status.loading -> {
                LoadingStatePanel("Loading your tasks")
                return@Column
            }
            (errorMessage != null || catalog.status.lastError != null) &&
                catalog.lists.isEmpty() && catalog.tasks.isEmpty() -> {
                ErrorStatePanel(errorMessage ?: catalog.status.lastError.orEmpty(), onRetry)
                return@Column
            }
            catalog.lists.isEmpty() -> {
                ContentStatePanel(
                    "No task lists yet",
                    "Create a list, then add a task with a due date and Markdown notes.",
                    "New task",
                    onCreateTask,
                )
                return@Column
            }
        }
        catalog.lists.forEach { list ->
            TaskListRow(
                list = list,
                openCount = catalog.openTasksIn(list.id).size,
                onClick = { onOpenList(list.id) },
            )
            HairlineDivider()
        }

        Spacer(Modifier.height(28.dp))
        SectionLabel("Open")
        Spacer(Modifier.height(6.dp))
        val open = catalog.recentOpenTasks()
        if (open.isEmpty()) {
            ContentStatePanel("Nothing open", "New tasks will appear here until you complete them.")
        } else {
            open.forEachIndexed { index, task ->
                TaskRow(task = task, listName = catalog.list(task.listId)?.name, onClick = { onOpenTask(task.id) })
                if (index != open.lastIndex) HairlineDivider()
            }
        }
    }
    if (namingList) {
        TaskNameDialog(
            title = "New list",
            initial = "",
            confirmLabel = "Create",
            placeholder = "List name",
            onDismiss = { namingList = false },
            onConfirm = { name ->
                namingList = false
                onCreateList(name)
            },
        )
    }
}

@Composable
fun TaskListScreen(
    catalog: TasksCatalog,
    listId: String,
    onOpenTask: (String) -> Unit,
    onCreateTask: () -> Unit = {},
    onRenameList: (String) -> Unit = {},
    onDeleteList: () -> Unit = {},
    onBack: () -> Unit,
) {
    val list = catalog.list(listId) ?: return
    val open = catalog.openTasksIn(listId)
    val done = catalog.completedTasksIn(listId)
    val deleteBlocked = catalog.validateListDelete(list)
    var renaming by rememberSaveable { mutableStateOf(false) }
    var confirmingDelete by rememberSaveable { mutableStateOf(false) }

    FoyerScreen {
        Column(modifier = Modifier.fillMaxSize().padding(horizontal = 24.dp)) {
            NestedScreenHeader(title = list.name, onBack = onBack)
            HairlineDivider()
            Column(modifier = Modifier.weight(1f).verticalScroll(rememberScrollState())) {
                Spacer(Modifier.height(16.dp))
                TasksStatusBanner(catalog.status)
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    ActionChip("Rename") { renaming = true }
                    ActionChip("Delete") { confirmingDelete = true }
                    ActionChip("Task", onCreateTask)
                }
                Spacer(Modifier.height(20.dp))
                SectionLabel("${open.size} open")
                Spacer(Modifier.height(6.dp))
                if (open.isEmpty() && done.isEmpty()) {
                    ContentStatePanel("This list is empty", "Add a task to keep work in one place.")
                } else if (open.isEmpty()) {
                    ContentStatePanel("All caught up", "Completed tasks stay below until you delete them.")
                } else {
                    open.forEachIndexed { index, task ->
                        TaskRow(task = task, onClick = { onOpenTask(task.id) })
                        if (index != open.lastIndex) HairlineDivider()
                    }
                }
                if (done.isNotEmpty()) {
                    Spacer(Modifier.height(24.dp))
                    SectionLabel("${done.size} done")
                    Spacer(Modifier.height(6.dp))
                    done.forEachIndexed { index, task ->
                        TaskRow(task = task, onClick = { onOpenTask(task.id) })
                        if (index != done.lastIndex) HairlineDivider()
                    }
                }
                Spacer(Modifier.height(24.dp))
            }
        }
    }
    if (renaming) {
        TaskNameDialog(
            title = "Rename list",
            initial = list.name,
            confirmLabel = "Save",
            placeholder = "List name",
            onDismiss = { renaming = false },
            onConfirm = { name ->
                renaming = false
                onRenameList(name)
            },
        )
    }
    if (confirmingDelete) {
        AlertDialog(
            onDismissRequest = { confirmingDelete = false },
            title = { Text("Delete list?", color = FoyerText) },
            text = {
                Text(
                    deleteBlocked ?: "“${list.name}” will be removed.",
                    color = FoyerTextMuted,
                )
            },
            confirmButton = {
                TextButton(
                    enabled = deleteBlocked == null,
                    onClick = {
                        confirmingDelete = false
                        onDeleteList()
                    },
                ) { Text("Delete") }
            },
            dismissButton = {
                TextButton(onClick = { confirmingDelete = false }) { Text("Cancel") }
            },
        )
    }
}

@Composable
fun TaskDetailScreen(
    catalog: TasksCatalog,
    taskId: String,
    onBack: () -> Unit,
    onEdit: () -> Unit,
    onComplete: () -> Unit,
    onReopen: () -> Unit,
    onDelete: () -> Unit,
) {
    val task = catalog.task(taskId) ?: return
    val list = catalog.list(task.listId)
    var confirmingDelete by rememberSaveable(task.id) { mutableStateOf(false) }
    var showingSource by rememberSaveable(task.id) { mutableStateOf(false) }

    FoyerScreen {
        Column(modifier = Modifier.fillMaxSize().padding(horizontal = 24.dp)) {
            NestedScreenHeader(title = task.title, onBack = onBack)
            HairlineDivider()
            Column(modifier = Modifier.weight(1f).verticalScroll(rememberScrollState())) {
                Spacer(Modifier.height(16.dp))
                TasksStatusBanner(catalog.status)
                Text(
                    buildString {
                        append(list?.name ?: "Task")
                        task.due?.let { append(" · ${it.displayLabel()}") }
                        if (task.priority > 0) append(" · ${task.priorityLabel}")
                        if (task.completed) append(" · Done")
                    },
                    style = MaterialTheme.typography.bodySmall,
                    color = FoyerTextDim,
                )
                Spacer(Modifier.height(16.dp))
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    ActionChip(if (task.completed) "Reopen" else "Complete") {
                        if (task.completed) onReopen() else onComplete()
                    }
                    ActionChip("Edit", onEdit)
                    ActionChip("Delete") { confirmingDelete = true }
                    ActionChip(if (showingSource) "Preview" else "Source") {
                        showingSource = !showingSource
                    }
                }
                Spacer(Modifier.height(20.dp))
                if (task.description.isBlank()) {
                    Text("No description", style = MaterialTheme.typography.bodyMedium, color = FoyerTextDim)
                } else if (showingSource) {
                    Text(task.description, style = MaterialTheme.typography.bodyMedium, color = FoyerTextMuted)
                } else {
                    SafeMarkdown(task.description)
                }
                Spacer(Modifier.height(24.dp))
            }
        }
    }
    if (confirmingDelete) {
        AlertDialog(
            onDismissRequest = { confirmingDelete = false },
            title = { Text("Delete task?", color = FoyerText) },
            text = { Text("“${task.title}” will be removed from this list.", color = FoyerTextMuted) },
            confirmButton = {
                TextButton(onClick = {
                    confirmingDelete = false
                    onDelete()
                }) { Text("Delete") }
            },
            dismissButton = {
                TextButton(onClick = { confirmingDelete = false }) { Text("Cancel") }
            },
        )
    }
}

@Composable
fun TaskEditorScreen(
    task: VaultTask?,
    lists: List<VaultTaskList>,
    initialListId: String?,
    status: TasksStatus = TasksStatus(loading = false),
    saving: Boolean = false,
    saveError: String? = null,
    onCancel: () -> Unit,
    onSave: (title: String, description: String, listId: String, due: TaskDue?, priority: Int) -> Unit,
) {
    var title by rememberSaveable(task?.id) { mutableStateOf(task?.title.orEmpty()) }
    var description by rememberSaveable(task?.id) { mutableStateOf(task?.description.orEmpty()) }
    var listId by rememberSaveable(task?.id) {
        mutableStateOf(task?.listId ?: initialListId ?: lists.firstOrNull()?.id.orEmpty())
    }
    var dueLocal by rememberSaveable(task?.id) {
        mutableStateOf(task?.due?.local.orEmpty().let { if (it.contains('T')) it else if (it.isBlank()) "" else "${it}T18:00:00" })
    }
    var dueDate by rememberSaveable(task?.id) {
        mutableStateOf(task?.due?.local?.take(10).orEmpty())
    }
    var allDay by rememberSaveable(task?.id) { mutableStateOf(task?.due?.allDay == true) }
    var timeZone by rememberSaveable(task?.id) { mutableStateOf(task?.due?.timeZone.orEmpty()) }
    var hasDue by rememberSaveable(task?.id) { mutableStateOf(task?.due != null) }
    var priority by rememberSaveable(task?.id) { mutableStateOf(task?.priority ?: 0) }
    var previewing by rememberSaveable(task?.id) { mutableStateOf(false) }
    val due = if (!hasDue) {
        null
    } else if (allDay) {
        TaskDue.parse(dueDate, timeZone, true)
    } else {
        val local = if (dueLocal.contains('T')) dueLocal else dueDate.takeIf(String::isNotBlank)?.let { "${it}T18:00:00" }.orEmpty()
        TaskDue.parse(local, timeZone, false)
    }
    val canSave = !saving && title.isNotBlank() && listId.isNotBlank() && (!hasDue || due != null)

    FoyerScreen {
        Column(modifier = Modifier.fillMaxSize().padding(horizontal = 24.dp)) {
            NestedScreenHeader(title = if (task == null) "New task" else "Edit task", onBack = onCancel)
            HairlineDivider()
            Column(modifier = Modifier.weight(1f).verticalScroll(rememberScrollState()).padding(bottom = 28.dp)) {
                Spacer(Modifier.height(16.dp))
                TasksStatusBanner(status)
                FieldLabel("Title")
                DraftField(value = title, onValueChange = { title = it }, placeholder = "What needs doing", singleLine = true)
                Spacer(Modifier.height(16.dp))
                FieldLabel("List")
                lists.forEach { list ->
                    Text(
                        text = if (list.id == listId) "• ${list.name}" else list.name,
                        style = MaterialTheme.typography.bodyMedium,
                        color = if (list.id == listId) FoyerText else FoyerTextMuted,
                        modifier = Modifier
                            .fillMaxWidth()
                            .clickable { listId = list.id }
                            .padding(vertical = 8.dp),
                    )
                }
                Spacer(Modifier.height(16.dp))
                FieldLabel("Due")
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    ActionChip(if (hasDue) "Clear date" else "Add date") { hasDue = !hasDue }
                    if (hasDue) {
                        ActionChip(if (allDay) "All day" else "Date & time") { allDay = !allDay }
                    }
                }
                if (hasDue) {
                    Spacer(Modifier.height(10.dp))
                    DraftField(
                        value = if (allDay) dueDate else dueLocal.ifBlank { dueDate },
                        onValueChange = {
                            if (allDay) dueDate = it else {
                                dueLocal = it
                                dueDate = it.take(10)
                            }
                        },
                        placeholder = if (allDay) "YYYY-MM-DD" else "YYYY-MM-DDTHH:MM:SS",
                        singleLine = true,
                    )
                    if (!allDay) {
                        Spacer(Modifier.height(10.dp))
                        TimezoneInput(value = timeZone, onValueChange = { timeZone = it })
                    }
                    if (due == null) {
                        Text("Enter a valid date.", style = MaterialTheme.typography.bodySmall, color = FoyerTextDim)
                    }
                }
                Spacer(Modifier.height(16.dp))
                FieldLabel("Priority")
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    listOf(0 to "None", 1 to "High", 5 to "Medium", 9 to "Low").forEach { (value, label) ->
                        ActionChip(if (priority == value) "• $label" else label) { priority = value }
                    }
                }
                Spacer(Modifier.height(16.dp))
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.SpaceBetween,
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    FieldLabel("Description")
                    Text(
                        if (previewing) "Edit" else "Preview",
                        style = MaterialTheme.typography.labelMedium,
                        color = FoyerTextMuted,
                        modifier = Modifier.clickable { previewing = !previewing }.padding(8.dp),
                    )
                }
                if (previewing) {
                    if (description.isBlank()) {
                        Text("Nothing to preview", style = MaterialTheme.typography.bodyMedium, color = FoyerTextDim)
                    } else {
                        SafeMarkdown(description)
                    }
                } else {
                    DraftField(
                        value = description,
                        onValueChange = { description = it },
                        placeholder = "Lossless Markdown",
                        singleLine = false,
                        minHeight = 160,
                    )
                }
                saveError?.let {
                    Spacer(Modifier.height(12.dp))
                    Text(it, style = MaterialTheme.typography.bodySmall, color = FoyerTextMuted)
                }
                Spacer(Modifier.height(20.dp))
                TextButton(enabled = canSave, onClick = {
                    onSave(title.trim(), description, listId, due, priority)
                }) {
                    Text(if (saving) "Saving" else "Save")
                }
            }
        }
    }
}

@Composable
fun TasksStatusBanner(status: TasksStatus, modifier: Modifier = Modifier) {
    val banner = status.banner() ?: return
    val (title, message) = when (banner) {
        is TasksSyncBanner.Offline -> "Offline" to if (banner.pendingUploads == 0) {
            "Reading the local replica. New changes will upload when Foyer Server is reachable."
        } else {
            "${banner.pendingUploads} change(s) are queued and will upload when you are back online."
        }
        is TasksSyncBanner.Pending -> "Pending sync" to
            "${banner.pendingUploads} change(s) are waiting to upload to Foyer Server."
        is TasksSyncBanner.StaleRevision -> "Stale revision" to banner.message
        is TasksSyncBanner.Error -> "Couldn’t sync" to banner.message
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

@Composable
private fun TaskListRow(list: VaultTaskList, openCount: Int, onClick: () -> Unit) {
    Row(
        modifier = Modifier.fillMaxWidth().height(54.dp).clickable(onClick = onClick),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(list.name, style = MaterialTheme.typography.titleMedium, color = FoyerText, modifier = Modifier.weight(1f))
        Text(openCount.toString(), style = MaterialTheme.typography.bodyMedium, color = FoyerTextMuted)
        Spacer(Modifier.padding(horizontal = 6.dp))
        ChevronGlyph()
    }
}

@Composable
private fun TaskRow(task: VaultTask, listName: String? = null, onClick: () -> Unit) {
    Row(
        modifier = Modifier.fillMaxWidth().clickable(onClick = onClick).padding(vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(modifier = Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Text(
                text = task.title,
                style = MaterialTheme.typography.titleMedium,
                color = FoyerText,
                fontWeight = FontWeight.Normal,
                textDecoration = if (task.completed) TextDecoration.LineThrough else TextDecoration.None,
            )
            val detail = buildList {
                listName?.let(::add)
                task.due?.displayLabel()?.let(::add)
                if (task.priority > 0) add(task.priorityLabel)
                task.summary.takeIf(String::isNotBlank)?.let(::add)
            }.joinToString(" · ")
            if (detail.isNotBlank()) {
                Text(detail, style = MaterialTheme.typography.bodySmall, color = FoyerTextMuted, maxLines = 2)
            }
        }
        Spacer(Modifier.padding(horizontal = 6.dp))
        ChevronGlyph()
    }
}

@Composable
private fun ActionChip(label: String, onClick: () -> Unit) {
    Surface(
        modifier = Modifier.clickable(onClick = onClick),
        shape = RoundedCornerShape(16.dp),
        color = FoyerBlack,
        contentColor = FoyerText,
        border = BorderStroke(1.dp, FoyerLine),
    ) {
        Text(
            label,
            style = MaterialTheme.typography.labelMedium,
            modifier = Modifier.padding(horizontal = 11.dp, vertical = 7.dp),
        )
    }
}

@Composable
private fun FieldLabel(text: String) {
    Text(text.uppercase(), style = MaterialTheme.typography.labelSmall, color = FoyerTextDim)
    Spacer(Modifier.height(6.dp))
}

@Composable
private fun DraftField(
    value: String,
    onValueChange: (String) -> Unit,
    placeholder: String,
    singleLine: Boolean,
    minHeight: Int = 0,
) {
    BasicTextField(
        value = value,
        onValueChange = onValueChange,
        singleLine = singleLine,
        textStyle = MaterialTheme.typography.bodyMedium.copy(color = FoyerText),
        cursorBrush = SolidColor(FoyerText),
        modifier = Modifier
            .fillMaxWidth()
            .then(if (minHeight > 0) Modifier.height(minHeight.dp) else Modifier)
            .border(1.dp, FoyerLine, RoundedCornerShape(12.dp))
            .padding(12.dp),
        decorationBox = { inner ->
            if (value.isEmpty()) {
                Text(placeholder, style = MaterialTheme.typography.bodyMedium, color = FoyerTextDim)
            }
            inner()
        },
    )
}

@Composable
internal fun TaskNameDialog(
    title: String,
    initial: String,
    confirmLabel: String,
    placeholder: String,
    onDismiss: () -> Unit,
    onConfirm: (String) -> Unit,
) {
    var draft by rememberSaveable { mutableStateOf(initial) }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(title, color = FoyerText) },
        text = {
            BasicTextField(
                value = draft,
                onValueChange = { draft = it },
                textStyle = MaterialTheme.typography.bodyMedium.copy(color = FoyerText),
                cursorBrush = SolidColor(FoyerText),
                modifier = Modifier
                    .fillMaxWidth()
                    .border(1.dp, FoyerLine, RoundedCornerShape(12.dp))
                    .padding(12.dp),
                decorationBox = { inner ->
                    if (draft.isEmpty()) {
                        Text(placeholder, style = MaterialTheme.typography.bodyMedium, color = FoyerTextDim)
                    }
                    inner()
                },
            )
        },
        confirmButton = {
            TextButton(enabled = draft.isNotBlank(), onClick = { onConfirm(draft.trim()) }) {
                Text(confirmLabel)
            }
        },
        dismissButton = {
            TextButton(onClick = onDismiss) { Text("Cancel") }
        },
    )
}
