package com.amazity.foyer.ui.screens

import android.content.Intent
import android.net.Uri
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.imePadding
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
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.amazity.foyer.bookmarks.parseTagInput
import com.amazity.foyer.bookmarks.validateBookmarkUrl
import com.amazity.foyer.model.BookmarkFolder
import com.amazity.foyer.model.BookmarkItem
import com.amazity.foyer.model.BookmarksCatalog
import com.amazity.foyer.model.BookmarksFilter
import com.amazity.foyer.model.BookmarksStatus
import com.amazity.foyer.model.BookmarksSyncBanner
import com.amazity.foyer.ui.components.ChevronGlyph
import com.amazity.foyer.ui.components.ContentStatePanel
import com.amazity.foyer.ui.components.ErrorStatePanel
import com.amazity.foyer.ui.components.FoyerScreen
import com.amazity.foyer.ui.components.HairlineDivider
import com.amazity.foyer.ui.components.LoadingStatePanel
import com.amazity.foyer.ui.components.NestedScreenHeader
import com.amazity.foyer.ui.components.PlusGlyph
import com.amazity.foyer.ui.components.SearchGlyph
import com.amazity.foyer.ui.components.SectionLabel
import com.amazity.foyer.ui.theme.FoyerBlack
import com.amazity.foyer.ui.theme.FoyerLine
import com.amazity.foyer.ui.theme.FoyerText
import com.amazity.foyer.ui.theme.FoyerTextDim
import com.amazity.foyer.ui.theme.FoyerTextMuted

@Composable
fun BookmarksPage(
    catalog: BookmarksCatalog,
    onOpenFolder: (String) -> Unit,
    onOpenBookmark: (String) -> Unit,
    onCreateBookmark: () -> Unit,
    onCreateFolder: (String) -> Unit = {},
    isLoading: Boolean = false,
    errorMessage: String? = null,
    onRetry: () -> Unit = {},
    modifier: Modifier = Modifier,
) {
    var query by rememberSaveable { mutableStateOf("") }
    var filter by rememberSaveable { mutableStateOf(BookmarksFilter.All) }
    var selectedTag by rememberSaveable { mutableStateOf<String?>(null) }
    var namingFolder by rememberSaveable { mutableStateOf(false) }
    val visible = catalog.visibleBookmarks(query, filter, selectedTag)
    val tags = catalog.allTags()
    val rootFolders = catalog.childFolders(null)

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
            SectionLabel("Bookmarks")
            Row {
                Box(modifier = Modifier.clickable { namingFolder = true }.padding(8.dp)) {
                    Text("Folder", style = MaterialTheme.typography.labelMedium, color = FoyerTextMuted)
                }
                Box(modifier = Modifier.clickable(onClick = onCreateBookmark).padding(8.dp)) {
                    PlusGlyph()
                }
            }
        }
        Spacer(Modifier.height(6.dp))
        BookmarksStatusBanner(catalog.status)
        if (catalog.status.developmentAuth) {
            Text(
                "Development session · local Foyer Server only",
                style = MaterialTheme.typography.bodySmall,
                color = FoyerTextDim,
            )
            Spacer(Modifier.height(8.dp))
        }
        BookmarksSearchField(query = query, onQueryChange = { query = it })
        Spacer(Modifier.height(10.dp))
        Row(
            modifier = Modifier.horizontalScroll(rememberScrollState()),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            BookmarksFilter.entries.forEach { option ->
                FilterChip(
                    label = option.label(),
                    selected = filter == option,
                    onClick = { filter = option },
                )
            }
        }
        if (tags.isNotEmpty()) {
            Spacer(Modifier.height(8.dp))
            Row(
                modifier = Modifier.horizontalScroll(rememberScrollState()),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                FilterChip("All tags", selectedTag == null) { selectedTag = null }
                tags.forEach { tag ->
                    FilterChip(tag, selectedTag == tag) {
                        selectedTag = if (selectedTag == tag) null else tag
                    }
                }
            }
        }
        Spacer(Modifier.height(18.dp))
        when {
            isLoading || catalog.status.loading -> {
                LoadingStatePanel("Loading your bookmarks")
                return@Column
            }
            (errorMessage != null || catalog.status.lastError != null) &&
                catalog.folders.isEmpty() && catalog.bookmarks.isEmpty() -> {
                ErrorStatePanel(errorMessage ?: catalog.status.lastError.orEmpty(), onRetry)
                return@Column
            }
        }
        if (query.isBlank() && filter == BookmarksFilter.All && selectedTag == null) {
            SectionLabel("Folders")
            Spacer(Modifier.height(6.dp))
            if (rootFolders.isEmpty()) {
                ContentStatePanel(
                    "No folders yet",
                    "Create a folder or save a URL to start your collection.",
                    "New bookmark",
                    onCreateBookmark,
                )
            } else {
                rootFolders.forEach { folder ->
                    BookmarkFolderRow(
                        folder = folder,
                        count = catalog.bookmarksIn(folder.id).size + catalog.childFolders(folder.id).size,
                        onClick = { onOpenFolder(folder.id) },
                    )
                    HairlineDivider()
                }
            }
            Spacer(Modifier.height(28.dp))
            SectionLabel("Recents")
            Spacer(Modifier.height(6.dp))
            val recents = catalog.recentBookmarks().filter { !it.archived }
            if (recents.isEmpty()) {
                ContentStatePanel("Nothing recent", "New bookmarks will appear here.")
            } else {
                recents.forEachIndexed { index, bookmark ->
                    BookmarkRow(bookmark = bookmark, onClick = { onOpenBookmark(bookmark.id) })
                    if (index != recents.lastIndex) HairlineDivider()
                }
            }
        } else {
            SectionLabel(
                when {
                    visible.isEmpty() -> "No matches"
                    else -> "${visible.size} bookmarks"
                },
            )
            Spacer(Modifier.height(6.dp))
            if (visible.isEmpty()) {
                ContentStatePanel("No bookmarks match", "Try another search, tag, or filter.")
            } else {
                visible.forEachIndexed { index, bookmark ->
                    BookmarkRow(bookmark = bookmark, onClick = { onOpenBookmark(bookmark.id) })
                    if (index != visible.lastIndex) HairlineDivider()
                }
            }
        }
    }
    if (namingFolder) {
        BookmarkFolderNameDialog(
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
fun BookmarkFolderScreen(
    catalog: BookmarksCatalog,
    folderId: String,
    onOpenBookmark: (String) -> Unit,
    onOpenFolder: (String) -> Unit = {},
    onCreateBookmark: () -> Unit = {},
    onCreateFolder: (String) -> Unit = {},
    onRenameFolder: (String) -> Unit = {},
    onMoveFolder: (String?) -> Unit = {},
    onDeleteFolder: () -> Unit = {},
    onBack: () -> Unit,
) {
    val folder = catalog.folder(folderId) ?: return
    val bookmarks = catalog.bookmarksIn(folderId)
    val children = catalog.childFolders(folderId)
    val path = catalog.folderPath(folderId)
    val empty = catalog.folderIsEmpty(folderId)
    val deleteBlocked = catalog.validateFolderDelete(folder)
    var namingChild by rememberSaveable { mutableStateOf(false) }
    var renaming by rememberSaveable { mutableStateOf(false) }
    var moving by rememberSaveable { mutableStateOf(false) }
    var confirmingDelete by rememberSaveable { mutableStateOf(false) }

    FoyerScreen {
        Column(modifier = Modifier.fillMaxSize().padding(horizontal = 24.dp)) {
            NestedScreenHeader(title = folder.name, onBack = onBack)
            HairlineDivider()
            Column(modifier = Modifier.weight(1f).verticalScroll(rememberScrollState())) {
                Spacer(Modifier.height(16.dp))
                if (path.size > 1) {
                    Text(
                        text = path.dropLast(1).joinToString(" / ", transform = BookmarkFolder::name),
                        style = MaterialTheme.typography.bodySmall,
                        color = FoyerTextDim,
                    )
                    Spacer(Modifier.height(10.dp))
                }
                BookmarksStatusBanner(catalog.status)
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
                        BookmarkFolderRow(
                            folder = child,
                            count = catalog.bookmarksIn(child.id).size + catalog.childFolders(child.id).size,
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
                    SectionLabel("${bookmarks.size} bookmarks")
                    Box(modifier = Modifier.clickable(onClick = onCreateBookmark).padding(8.dp)) {
                        PlusGlyph()
                    }
                }
                Spacer(Modifier.height(6.dp))
                if (bookmarks.isEmpty() && children.isEmpty()) {
                    ContentStatePanel("This folder is empty", "Create a nested folder or save a URL here.")
                } else if (bookmarks.isEmpty()) {
                    ContentStatePanel("No bookmarks here", "New bookmarks filed here will appear in this list.")
                } else {
                    bookmarks.forEachIndexed { index, bookmark ->
                        BookmarkRow(bookmark = bookmark, onClick = { onOpenBookmark(bookmark.id) })
                        if (index != bookmarks.lastIndex) HairlineDivider()
                    }
                }
                Spacer(Modifier.height(24.dp))
            }
        }
    }

    if (namingChild) {
        BookmarkFolderNameDialog(
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
        BookmarkFolderNameDialog(
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
        BookmarkFolderPickerDialog(
            title = "Move folder",
            rootLabel = "Bookmarks root",
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
                    deleteBlocked ?: "“${folder.name}” will be removed from your bookmarks.",
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
fun BookmarkDetailScreen(
    bookmark: BookmarkItem,
    folder: BookmarkFolder?,
    status: BookmarksStatus = BookmarksStatus(loading = false),
    onBack: () -> Unit,
    onEdit: () -> Unit,
    onToggleFavorite: () -> Unit,
    onToggleArchived: () -> Unit,
    onDelete: () -> Unit,
    onOpenUrl: ((String) -> Unit)? = null,
) {
    var confirmDelete by remember(bookmark.id) { mutableStateOf(false) }
    val context = LocalContext.current
    val openUrl = onOpenUrl ?: { url ->
        runCatching {
            context.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(url)))
        }
    }

    FoyerScreen {
        Column(modifier = Modifier.fillMaxSize().padding(horizontal = 24.dp)) {
            NestedScreenHeader(title = bookmark.title, onBack = onBack)
            HairlineDivider()
            Column(
                modifier = Modifier
                    .weight(1f)
                    .verticalScroll(rememberScrollState())
                    .padding(top = 16.dp, bottom = 24.dp),
                verticalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                BookmarksStatusBanner(status)
                Text(bookmark.host, style = MaterialTheme.typography.bodySmall, color = FoyerTextDim)
                Text(
                    bookmark.url,
                    style = MaterialTheme.typography.bodyMedium,
                    color = FoyerText,
                    modifier = Modifier.clickable { openUrl(bookmark.url) },
                )
                if (folder != null) {
                    Text(folder.name, style = MaterialTheme.typography.bodySmall, color = FoyerTextMuted)
                }
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    if (bookmark.favorite) StatusPill("Favorite")
                    if (bookmark.archived) StatusPill("Archived")
                }
                if (bookmark.tags.isNotEmpty()) {
                    Row(
                        modifier = Modifier.horizontalScroll(rememberScrollState()),
                        horizontalArrangement = Arrangement.spacedBy(8.dp),
                    ) {
                        bookmark.tags.forEach { StatusPill(it) }
                    }
                }
                if (bookmark.description.isNotEmpty()) {
                    Spacer(Modifier.height(4.dp))
                    SectionLabel("Description")
                    Text(
                        bookmark.description,
                        style = MaterialTheme.typography.bodyMedium,
                        color = FoyerText,
                    )
                }
                Text(bookmark.updatedLabel, style = MaterialTheme.typography.bodySmall, color = FoyerTextDim)
                Spacer(Modifier.height(8.dp))
                FolderActionRow(
                    labels = listOf(
                        "Open" to { openUrl(bookmark.url) },
                        "Edit" to onEdit,
                        if (bookmark.favorite) "Unfavorite" to onToggleFavorite else "Favorite" to onToggleFavorite,
                        if (bookmark.archived) "Restore" to onToggleArchived else "Archive" to onToggleArchived,
                        "Delete" to { confirmDelete = true },
                    ),
                )
            }
        }
    }
    if (confirmDelete) {
        AlertDialog(
            onDismissRequest = { confirmDelete = false },
            title = { Text("Delete bookmark?", color = FoyerText) },
            text = {
                Text(
                    "“${bookmark.title}” will be removed. This cannot be undone from this device.",
                    color = FoyerTextMuted,
                )
            },
            confirmButton = {
                TextButton(
                    onClick = {
                        confirmDelete = false
                        onDelete()
                    },
                ) { Text("Delete") }
            },
            dismissButton = {
                TextButton(onClick = { confirmDelete = false }) { Text("Cancel") }
            },
        )
    }
}

@Composable
fun BookmarkEditorScreen(
    bookmark: BookmarkItem?,
    folders: List<BookmarkFolder>,
    initialFolderId: String?,
    status: BookmarksStatus = BookmarksStatus(loading = false),
    saving: Boolean = false,
    saveError: String? = null,
    onCancel: () -> Unit,
    onSave: (url: String, title: String, description: String, tags: List<String>, folderId: String) -> Unit,
) {
    var url by rememberSaveable(bookmark?.id) { mutableStateOf(bookmark?.url.orEmpty()) }
    var title by rememberSaveable(bookmark?.id) { mutableStateOf(bookmark?.title.orEmpty()) }
    var description by rememberSaveable(bookmark?.id) { mutableStateOf(bookmark?.description.orEmpty()) }
    var tagInput by rememberSaveable(bookmark?.id) {
        mutableStateOf(bookmark?.tags?.joinToString(", ").orEmpty())
    }
    var folderId by rememberSaveable(bookmark?.id) {
        mutableStateOf(bookmark?.folderId ?: initialFolderId ?: folders.firstOrNull()?.id.orEmpty())
    }
    var pickingFolder by rememberSaveable { mutableStateOf(false) }
    val urlError = remember(url) {
        if (url.isBlank()) null else runCatching { validateBookmarkUrl(url); null }.exceptionOrNull()?.message
    }
    val parsedTags = runCatching { parseTagInput(tagInput) }.getOrElse { emptyList() }
    val tagError = runCatching { parseTagInput(tagInput); null }.exceptionOrNull()?.message
    val canSave = !saving && url.isNotBlank() && title.isNotBlank() && folderId.isNotBlank() &&
        urlError == null && tagError == null

    FoyerScreen {
        Column(
            modifier = Modifier
                .fillMaxSize()
                .imePadding()
                .padding(horizontal = 24.dp),
        ) {
            NestedScreenHeader(title = if (bookmark == null) "New bookmark" else "Edit bookmark", onBack = onCancel)
            HairlineDivider()
            Column(
                modifier = Modifier
                    .weight(1f)
                    .verticalScroll(rememberScrollState())
                    .padding(top = 16.dp, bottom = 24.dp),
                verticalArrangement = Arrangement.spacedBy(14.dp),
            ) {
                BookmarksStatusBanner(status)
                if (saveError != null) {
                    Text(saveError, style = MaterialTheme.typography.bodySmall, color = FoyerTextMuted)
                }
                EditorField("URL", url, "https://", singleLine = true) { url = it }
                urlError?.let { Text(it, style = MaterialTheme.typography.bodySmall, color = FoyerTextMuted) }
                EditorField("Title", title, "Title", singleLine = true) { title = it }
                EditorField("Description", description, "Optional notes, kept exactly", singleLine = false) {
                    description = it
                }
                EditorField("Tags", tagInput, "work, docs", singleLine = true) { tagInput = it }
                if (parsedTags.isNotEmpty()) {
                    Row(
                        modifier = Modifier.horizontalScroll(rememberScrollState()),
                        horizontalArrangement = Arrangement.spacedBy(8.dp),
                    ) {
                        parsedTags.forEach { StatusPill(it) }
                    }
                }
                tagError?.let { Text(it, style = MaterialTheme.typography.bodySmall, color = FoyerTextMuted) }
                SectionLabel("Folder")
                Text(
                    text = folders.firstOrNull { it.id == folderId }?.name ?: "Choose a folder",
                    style = MaterialTheme.typography.bodyMedium,
                    color = FoyerText,
                    modifier = Modifier
                        .fillMaxWidth()
                        .clickable { pickingFolder = true }
                        .padding(vertical = 8.dp),
                )
                Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                    TextButton(onClick = onCancel) { Text("Cancel") }
                    TextButton(
                        enabled = canSave,
                        onClick = {
                            onSave(url.trim(), title.trim(), description, parsedTags, folderId)
                        },
                    ) { Text(if (saving) "Saving" else "Save") }
                }
            }
        }
    }
    if (pickingFolder) {
        BookmarkFolderPickerDialog(
            title = "Choose folder",
            rootLabel = "",
            folders = folders,
            selectedId = folderId,
            allowRoot = false,
            pathLabel = { id -> folders.firstOrNull { it.id == id }?.name ?: id },
            confirmLabel = "Choose",
            onDismiss = { pickingFolder = false },
            onConfirm = { selected ->
                pickingFolder = false
                if (selected != null) folderId = selected
            },
        )
    }
}

@Composable
fun BookmarksStatusBanner(status: BookmarksStatus, modifier: Modifier = Modifier) {
    val banner = status.banner() ?: return
    val (title, message) = when (banner) {
        is BookmarksSyncBanner.Offline -> "Offline" to if (banner.pendingUploads == 0) {
            "Reading the local replica. New changes will upload when Foyer Server is reachable."
        } else {
            "${banner.pendingUploads} change(s) are queued and will upload when you are back online."
        }
        is BookmarksSyncBanner.Pending -> "Pending sync" to
            "${banner.pendingUploads} change(s) are waiting to upload to Foyer Server."
        is BookmarksSyncBanner.StaleRevision -> "Stale revision" to banner.message
        is BookmarksSyncBanner.Error -> "Couldn’t sync" to banner.message
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
private fun BookmarksSearchField(query: String, onQueryChange: (String) -> Unit) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .border(1.dp, FoyerLine, RoundedCornerShape(14.dp))
            .padding(horizontal = 12.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        SearchGlyph()
        BasicTextField(
            value = query,
            onValueChange = onQueryChange,
            textStyle = MaterialTheme.typography.bodyMedium.copy(color = FoyerText),
            cursorBrush = SolidColor(FoyerText),
            singleLine = true,
            modifier = Modifier.weight(1f),
            decorationBox = { inner ->
                if (query.isEmpty()) {
                    Text("Search titles, URLs, tags", style = MaterialTheme.typography.bodyMedium, color = FoyerTextDim)
                }
                inner()
            },
        )
    }
}

@Composable
private fun BookmarkFolderRow(folder: BookmarkFolder, count: Int, onClick: () -> Unit) {
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
        Text(count.toString(), style = MaterialTheme.typography.bodyMedium, color = FoyerTextMuted)
        Spacer(Modifier.padding(horizontal = 6.dp))
        ChevronGlyph()
    }
}

@Composable
private fun BookmarkRow(bookmark: BookmarkItem, onClick: () -> Unit) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .clickable(onClick = onClick)
            .padding(vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(modifier = Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
                Text(
                    text = bookmark.title,
                    style = MaterialTheme.typography.titleMedium,
                    color = FoyerText,
                    fontWeight = FontWeight.Normal,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.weight(1f, fill = false),
                )
                if (bookmark.favorite) {
                    Text("★", style = MaterialTheme.typography.labelSmall, color = FoyerTextMuted)
                }
            }
            Text(bookmark.host, style = MaterialTheme.typography.bodySmall, color = FoyerTextMuted, maxLines = 1)
            Text(bookmark.summary, style = MaterialTheme.typography.bodySmall, color = FoyerTextDim, maxLines = 2)
        }
        Spacer(Modifier.padding(horizontal = 6.dp))
        ChevronGlyph()
    }
}

@Composable
private fun FilterChip(label: String, selected: Boolean, onClick: () -> Unit) {
    Surface(
        modifier = Modifier.clickable(onClick = onClick),
        shape = RoundedCornerShape(16.dp),
        color = FoyerBlack,
        contentColor = FoyerText,
        border = BorderStroke(1.dp, if (selected) FoyerTextMuted else FoyerLine),
    ) {
        Text(
            label,
            style = MaterialTheme.typography.labelMedium,
            color = if (selected) FoyerText else FoyerTextMuted,
            modifier = Modifier.padding(horizontal = 11.dp, vertical = 7.dp),
        )
    }
}

@Composable
private fun StatusPill(label: String) {
    Surface(
        shape = RoundedCornerShape(16.dp),
        color = FoyerBlack,
        border = BorderStroke(1.dp, FoyerLine),
    ) {
        Text(
            label,
            style = MaterialTheme.typography.labelMedium,
            color = FoyerTextMuted,
            modifier = Modifier.padding(horizontal = 11.dp, vertical = 5.dp),
        )
    }
}

@Composable
private fun FolderActionRow(
    onCreateFolder: () -> Unit,
    onRename: () -> Unit,
    onMove: () -> Unit,
    onDelete: () -> Unit,
) {
    FolderActionRow(
        labels = listOf(
            "Folder" to onCreateFolder,
            "Rename" to onRename,
            "Move" to onMove,
            "Delete" to onDelete,
        ),
    )
}

@Composable
private fun FolderActionRow(labels: List<Pair<String, () -> Unit>>) {
    Row(
        modifier = Modifier.horizontalScroll(rememberScrollState()),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        labels.forEach { (label, onClick) ->
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
    }
}

@Composable
internal fun BookmarkFolderNameDialog(
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
            TextButton(enabled = draft.isNotBlank(), onClick = { onConfirm(draft.trim()) }) {
                Text(confirmLabel)
            }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Cancel") } },
    )
}

@Composable
internal fun BookmarkFolderPickerDialog(
    title: String,
    rootLabel: String,
    folders: List<BookmarkFolder>,
    selectedId: String?,
    allowRoot: Boolean,
    pathLabel: (String) -> String,
    confirmLabel: String = "Move",
    onDismiss: () -> Unit,
    onConfirm: (String?) -> Unit,
) {
    var selected by remember { mutableStateOf(selectedId) }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(title, color = FoyerText) },
        text = {
            Column(modifier = Modifier.verticalScroll(rememberScrollState()).heightIn(max = 360.dp)) {
                if (allowRoot) {
                    FolderPickRow(rootLabel, selected == null) { selected = null }
                }
                folders.forEach { folder ->
                    FolderPickRow(pathLabel(folder.id), selected == folder.id) { selected = folder.id }
                }
            }
        },
        confirmButton = { TextButton(onClick = { onConfirm(selected) }) { Text(confirmLabel) } },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Cancel") } },
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

@Composable
private fun EditorField(
    label: String,
    value: String,
    placeholder: String,
    singleLine: Boolean,
    onValueChange: (String) -> Unit,
) {
    SectionLabel(label)
    BasicTextField(
        value = value,
        onValueChange = onValueChange,
        textStyle = MaterialTheme.typography.bodyMedium.copy(color = FoyerText),
        cursorBrush = SolidColor(FoyerText),
        singleLine = singleLine,
        modifier = Modifier
            .fillMaxWidth()
            .heightIn(min = if (singleLine) 44.dp else 120.dp)
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

internal fun BookmarksFilter.label(): String = when (this) {
    BookmarksFilter.All -> "All"
    BookmarksFilter.Favorites -> "Favorites"
    BookmarksFilter.Archived -> "Archived"
}
