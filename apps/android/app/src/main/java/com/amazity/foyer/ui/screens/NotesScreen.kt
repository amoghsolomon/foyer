package com.amazity.foyer.ui.screens

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
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
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import com.amazity.foyer.model.NotesCatalog
import com.amazity.foyer.model.NotesStatus
import com.amazity.foyer.model.NotesSyncBanner
import com.amazity.foyer.model.VaultFolder
import com.amazity.foyer.model.VaultNote
import com.amazity.foyer.ui.components.ChevronGlyph
import com.amazity.foyer.ui.components.ContentStatePanel
import com.amazity.foyer.ui.components.FoyerScreen
import com.amazity.foyer.ui.components.HairlineDivider
import com.amazity.foyer.ui.components.NestedScreenHeader
import com.amazity.foyer.ui.components.PlusGlyph
import com.amazity.foyer.ui.components.SectionLabel
import com.amazity.foyer.ui.theme.FoyerBlack
import com.amazity.foyer.ui.theme.FoyerLine
import com.amazity.foyer.ui.theme.FoyerText
import com.amazity.foyer.ui.theme.FoyerTextDim
import com.amazity.foyer.ui.theme.FoyerTextMuted

@Composable
fun NotesPage(
    catalog: NotesCatalog,
    onOpenFolder: (String) -> Unit,
    onOpenNote: (String) -> Unit,
    onCreateNote: () -> Unit,
    onCreateFolder: (String) -> Unit = {},
    isLoading: Boolean = false,
    errorMessage: String? = null,
    onRetry: () -> Unit = {},
    modifier: Modifier = Modifier,
) {
    var namingFolder by rememberSaveable { mutableStateOf(false) }
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
            SectionLabel("Folders")
            Row {
                androidx.compose.foundation.layout.Box(
                    modifier = Modifier.clickable { namingFolder = true }.padding(8.dp),
                ) {
                    Text("Folder", style = MaterialTheme.typography.labelMedium, color = FoyerTextMuted)
                }
                androidx.compose.foundation.layout.Box(
                    modifier = Modifier.clickable(onClick = onCreateNote).padding(8.dp),
                ) { PlusGlyph() }
            }
        }
        Spacer(Modifier.height(6.dp))
        NotesStatusBanner(catalog.status)
        if (catalog.status.developmentAuth) {
            Text(
                "Development session · local Foyer Server only",
                style = MaterialTheme.typography.bodySmall,
                color = FoyerTextDim,
            )
            Spacer(Modifier.height(8.dp))
        }
        val rootFolders = catalog.childFolders(null)
        when {
            isLoading || catalog.status.loading -> {
                com.amazity.foyer.ui.components.LoadingStatePanel("Loading your notes vault")
                return@Column
            }
            (errorMessage != null || catalog.status.lastError != null) &&
                catalog.folders.isEmpty() && catalog.notes.isEmpty() -> {
                com.amazity.foyer.ui.components.ErrorStatePanel(
                    errorMessage ?: catalog.status.lastError.orEmpty(),
                    onRetry,
                )
                return@Column
            }
            rootFolders.isEmpty() -> {
                ContentStatePanel("No folders yet", "Create a folder or note to start your vault.", "New note", onCreateNote)
                return@Column
            }
        }
        rootFolders.forEach { folder ->
            FolderRow(
                folder = folder,
                noteCount = catalog.notesIn(folder.id).size + catalog.childFolders(folder.id).size,
                onClick = { onOpenFolder(folder.id) },
            )
            HairlineDivider()
        }

        Spacer(Modifier.height(28.dp))
        SectionLabel("Recents")
        Spacer(Modifier.height(6.dp))
        catalog.recentNotes().forEachIndexed { index, note ->
            NoteRow(note = note, onClick = { onOpenNote(note.id) })
            if (index != catalog.recentNotes().lastIndex) HairlineDivider()
        }
    }
    if (namingFolder) {
        FolderNameDialog(
            title = "New folder",
            initial = "",
            confirmLabel = "Create",
            onDismiss = { namingFolder = false },
            onConfirm = { name ->
                namingFolder = false
                onCreateFolder(name)
            },
        )
    }
}

@Composable
fun FolderNotesScreen(
    catalog: NotesCatalog,
    folderId: String,
    onOpenNote: (String) -> Unit,
    onOpenFolder: (String) -> Unit = {},
    onCreateNote: () -> Unit = {},
    onCreateFolder: (String) -> Unit = {},
    onRenameFolder: (String) -> Unit = {},
    onMoveFolder: (String?) -> Unit = {},
    onDeleteFolder: () -> Unit = {},
    onBack: () -> Unit,
) {
    val folder = catalog.folder(folderId) ?: return
    val notes = catalog.notesIn(folderId)
    val children = catalog.childFolders(folderId)
    val path = catalog.folderPath(folderId)
    val empty = catalog.folderIsEmpty(folderId)
    val deleteBlocked = catalog.validateFolderDelete(folder)
    var namingChild by rememberSaveable { mutableStateOf(false) }
    var renaming by rememberSaveable { mutableStateOf(false) }
    var moving by rememberSaveable { mutableStateOf(false) }
    var confirmingDelete by rememberSaveable { mutableStateOf(false) }

    FoyerScreen {
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(horizontal = 24.dp),
        ) {
            NestedScreenHeader(title = folder.name, onBack = onBack)
            HairlineDivider()
            Column(modifier = Modifier.weight(1f).verticalScroll(rememberScrollState())) {
                Spacer(Modifier.height(16.dp))
                if (path.size > 1) {
                    Text(
                        text = path.dropLast(1).joinToString(" / ", transform = VaultFolder::name),
                        style = MaterialTheme.typography.bodySmall,
                        color = FoyerTextDim,
                    )
                    Spacer(Modifier.height(10.dp))
                }
                NotesStatusBanner(catalog.status)
                FolderActionRow(
                    onCreateFolder = { namingChild = true },
                    onRename = { renaming = true },
                    onMove = { moving = true },
                    onDelete = { confirmingDelete = true },
                )
                Spacer(Modifier.height(20.dp))
                if (children.isNotEmpty()) {
                    SectionLabel("${children.size} folders")
                    Spacer(Modifier.height(6.dp))
                    children.forEach { child ->
                        FolderRow(
                            folder = child,
                            noteCount = catalog.notesIn(child.id).size + catalog.childFolders(child.id).size,
                            onClick = { onOpenFolder(child.id) },
                        )
                        HairlineDivider()
                    }
                    Spacer(Modifier.height(20.dp))
                }
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.SpaceBetween,
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    SectionLabel("${notes.size} notes")
                    androidx.compose.foundation.layout.Box(
                        modifier = Modifier.clickable(onClick = onCreateNote).padding(8.dp),
                    ) { PlusGlyph() }
                }
                Spacer(Modifier.height(6.dp))
                if (notes.isEmpty() && children.isEmpty()) {
                    ContentStatePanel("This folder is empty", "Create a nested folder or note here.")
                } else if (notes.isEmpty()) {
                    ContentStatePanel("No notes here", "New notes filed here will appear in this list.")
                } else {
                    notes.forEachIndexed { index, note ->
                        NoteRow(note = note, onClick = { onOpenNote(note.id) })
                        if (index != notes.lastIndex) HairlineDivider()
                    }
                }
                Spacer(Modifier.height(24.dp))
            }
        }
    }

    if (namingChild) {
        FolderNameDialog(
            title = "New folder",
            initial = "",
            confirmLabel = "Create",
            onDismiss = { namingChild = false },
            onConfirm = { name ->
                namingChild = false
                onCreateFolder(name)
            },
        )
    }
    if (renaming) {
        FolderNameDialog(
            title = "Rename folder",
            initial = folder.name,
            confirmLabel = "Save",
            onDismiss = { renaming = false },
            onConfirm = { name ->
                renaming = false
                onRenameFolder(name)
            },
        )
    }
    if (moving) {
        FolderPickerDialog(
            title = "Move folder",
            rootLabel = "Vault root",
            folders = catalog.validFolderMoveTargets(folder),
            selectedId = folder.parentId,
            allowRoot = true,
            pathLabel = catalog::folderPathLabel,
            onDismiss = { moving = false },
            onConfirm = { parentId ->
                moving = false
                onMoveFolder(parentId)
            },
        )
    }
    if (confirmingDelete) {
        AlertDialog(
            onDismissRequest = { confirmingDelete = false },
            title = { Text("Delete folder?", color = FoyerText) },
            text = {
                Text(
                    deleteBlocked ?: "“${folder.name}” will be removed from your vault.",
                    color = FoyerTextMuted,
                )
            },
            confirmButton = {
                TextButton(
                    enabled = empty,
                    onClick = {
                        confirmingDelete = false
                        onDeleteFolder()
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
private fun FolderRow(
    folder: VaultFolder,
    noteCount: Int,
    onClick: () -> Unit,
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .height(54.dp)
            .clickable(onClick = onClick),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(
            text = folder.name,
            style = MaterialTheme.typography.titleMedium,
            color = FoyerText,
            modifier = Modifier.weight(1f),
        )
        Text(
            text = noteCount.toString(),
            style = MaterialTheme.typography.bodyMedium,
            color = FoyerTextMuted,
        )
        Spacer(Modifier.padding(horizontal = 6.dp))
        ChevronGlyph()
    }
}

@Composable
private fun NoteRow(
    note: VaultNote,
    onClick: () -> Unit,
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .clickable(onClick = onClick)
            .padding(vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(
            modifier = Modifier.weight(1f),
            verticalArrangement = Arrangement.spacedBy(2.dp),
        ) {
            Text(
                text = note.title,
                style = MaterialTheme.typography.titleMedium,
                color = FoyerText,
                fontWeight = FontWeight.Normal,
            )
            Text(
                text = note.summary,
                style = MaterialTheme.typography.bodySmall,
                color = FoyerTextMuted,
                maxLines = 2,
            )
            Text(
                text = note.updatedLabel,
                style = MaterialTheme.typography.bodySmall,
                color = FoyerTextDim,
            )
        }
        Spacer(Modifier.padding(horizontal = 6.dp))
        ChevronGlyph()
    }
}

@Composable
fun NotesStatusBanner(status: NotesStatus, modifier: Modifier = Modifier) {
    val banner = status.banner() ?: return
    val (title, message) = when (banner) {
        is NotesSyncBanner.Offline -> "Offline" to if (banner.pendingUploads == 0) {
            "Reading the local replica. New changes will upload when Foyer Server is reachable."
        } else {
            "${banner.pendingUploads} change(s) are queued and will upload when you are back online."
        }
        is NotesSyncBanner.Pending -> "Pending sync" to
            "${banner.pendingUploads} change(s) are waiting to upload to Foyer Server."
        is NotesSyncBanner.StaleRevision -> "Stale revision" to banner.message
        is NotesSyncBanner.Error -> "Couldn’t sync" to banner.message
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
private fun FolderActionRow(
    onCreateFolder: () -> Unit,
    onRename: () -> Unit,
    onMove: () -> Unit,
    onDelete: () -> Unit,
) {
    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        FolderChip("Folder", onCreateFolder)
        FolderChip("Rename", onRename)
        FolderChip("Move", onMove)
        FolderChip("Delete", onDelete)
    }
}

@Composable
private fun FolderChip(label: String, onClick: () -> Unit) {
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
internal fun FolderNameDialog(
    title: String,
    initial: String,
    confirmLabel: String,
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
                        Text("Folder name", style = MaterialTheme.typography.bodyMedium, color = FoyerTextDim)
                    }
                    inner()
                },
            )
        },
        confirmButton = {
            TextButton(
                enabled = draft.isNotBlank(),
                onClick = { onConfirm(draft.trim()) },
            ) { Text(confirmLabel) }
        },
        dismissButton = {
            TextButton(onClick = onDismiss) { Text("Cancel") }
        },
    )
}

@Composable
internal fun FolderPickerDialog(
    title: String,
    rootLabel: String,
    folders: List<VaultFolder>,
    selectedId: String?,
    allowRoot: Boolean,
    pathLabel: (String) -> String,
    onDismiss: () -> Unit,
    onConfirm: (String?) -> Unit,
) {
    var selected by remember { mutableStateOf(selectedId) }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(title, color = FoyerText) },
        text = {
            Column(modifier = Modifier.verticalScroll(rememberScrollState())) {
                if (allowRoot) {
                    FolderPickRow(rootLabel, selected == null) { selected = null }
                }
                folders.forEach { folder ->
                    FolderPickRow(pathLabel(folder.id), selected == folder.id) { selected = folder.id }
                }
            }
        },
        confirmButton = {
            TextButton(onClick = { onConfirm(selected) }) { Text("Move") }
        },
        dismissButton = {
            TextButton(onClick = onDismiss) { Text("Cancel") }
        },
    )
}

@Composable
private fun FolderPickRow(label: String, selected: Boolean, onClick: () -> Unit) {
    Text(
        text = if (selected) "• $label" else label,
        style = MaterialTheme.typography.bodyMedium,
        color = if (selected) FoyerText else FoyerTextMuted,
        modifier = Modifier
            .fillMaxWidth()
            .clickable(onClick = onClick)
            .padding(vertical = 8.dp),
    )
}
