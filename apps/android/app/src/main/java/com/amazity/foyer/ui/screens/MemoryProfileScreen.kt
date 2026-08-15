package com.amazity.foyer.ui.screens

import androidx.compose.foundation.BorderStroke
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
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.amazity.foyer.R
import com.amazity.foyer.model.ConsolidatedProfile
import com.amazity.foyer.model.MemoryPage
import com.amazity.foyer.model.MemoryRecord
import com.amazity.foyer.ui.components.FoyerScreen
import com.amazity.foyer.ui.components.HairlineDivider
import com.amazity.foyer.ui.components.MoreGlyph
import com.amazity.foyer.ui.components.NestedScreenHeader
import com.amazity.foyer.ui.components.SectionLabel
import com.amazity.foyer.ui.theme.FoyerLine
import com.amazity.foyer.ui.theme.FoyerSurfaceRaised
import com.amazity.foyer.ui.theme.FoyerText
import com.amazity.foyer.ui.theme.FoyerTextDim
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.launch

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun MemoryProfileScreen(
    onBack: () -> Unit,
    loadProfile: suspend () -> ConsolidatedProfile?,
    loadMemories: suspend (String?) -> MemoryPage,
    deleteMemory: suspend (MemoryRecord) -> Unit,
) {
    var profile by remember { mutableStateOf<ConsolidatedProfile?>(null) }
    var memories by remember { mutableStateOf(emptyList<MemoryRecord>()) }
    var nextCursor by remember { mutableStateOf<String?>(null) }
    var refreshing by remember { mutableStateOf(false) }
    var loadingMore by remember { mutableStateOf(false) }
    var memoryToDelete by remember { mutableStateOf<MemoryRecord?>(null) }
    val listState = rememberLazyListState()
    val snackbar = remember { SnackbarHostState() }
    val scope = rememberCoroutineScope()
    val loadError = stringResource(R.string.memory_load_error)
    val deleteError = stringResource(R.string.memory_delete_error)

    val refresh: () -> Unit = {
        if (!refreshing) scope.launch {
            refreshing = true
            val profileResult = runCatching { loadProfile() }
            val pageResult = runCatching { loadMemories(null) }
            profileResult.onSuccess { profile = it }
            pageResult.onSuccess { page ->
                memories = page.items
                nextCursor = page.nextCursor
            }
            if (profileResult.isFailure || pageResult.isFailure) snackbar.showSnackbar(loadError)
            refreshing = false
        }
    }

    LaunchedEffect(Unit) { refresh() }
    LaunchedEffect(listState, memories.size, nextCursor) {
        snapshotFlow { listState.layoutInfo.visibleItemsInfo.lastOrNull()?.index ?: 0 }
            .distinctUntilChanged()
            .collect { lastVisible ->
                val cursor = nextCursor
                if (cursor != null && !loadingMore && lastVisible >= memories.lastIndex - 3) {
                    loadingMore = true
                    runCatching { loadMemories(cursor) }
                        .onSuccess { page ->
                            val knownIds = memories.mapTo(mutableSetOf(), MemoryRecord::id)
                            memories = memories + page.items.filter { knownIds.add(it.id) }
                            nextCursor = page.nextCursor
                        }
                        .onFailure { snackbar.showSnackbar(loadError) }
                    loadingMore = false
                }
            }
    }

    FoyerScreen {
        Column(modifier = Modifier.fillMaxSize().padding(horizontal = 24.dp)) {
            NestedScreenHeader(
                title = stringResource(R.string.memory_screen_title),
                onBack = onBack,
            )
            HairlineDivider()
            PullToRefreshBox(
                isRefreshing = refreshing,
                onRefresh = refresh,
                modifier = Modifier.fillMaxSize(),
            ) {
                LazyColumn(
                    state = listState,
                    contentPadding = PaddingValues(vertical = 22.dp),
                    verticalArrangement = Arrangement.spacedBy(12.dp),
                ) {
                    item(key = "profile") {
                        ProfileSection(profile)
                        Spacer(Modifier.height(22.dp))
                        SectionLabel(stringResource(R.string.memory_list_heading))
                    }
                    if (memories.isEmpty() && !refreshing) {
                        item(key = "empty") {
                            Text(
                                text = stringResource(R.string.memory_empty),
                                style = MaterialTheme.typography.bodyMedium,
                                color = FoyerTextDim,
                                modifier = Modifier.padding(vertical = 16.dp),
                            )
                        }
                    }
                    itemsIndexed(memories, key = { _, item -> item.id }) { _, memory ->
                        MemoryRow(memory = memory, onDelete = { memoryToDelete = memory })
                    }
                    if (loadingMore) {
                        item(key = "loading-more") {
                            Text(
                                text = stringResource(R.string.memory_loading_more),
                                style = MaterialTheme.typography.bodySmall,
                                color = FoyerTextDim,
                                modifier = Modifier.padding(vertical = 10.dp),
                            )
                        }
                    }
                }
            }
        }
        Box(modifier = Modifier.fillMaxSize(), contentAlignment = Alignment.BottomCenter) {
            SnackbarHost(hostState = snackbar)
        }
    }

    memoryToDelete?.let { memory ->
        AlertDialog(
            onDismissRequest = { memoryToDelete = null },
            title = { Text(stringResource(R.string.memory_delete_title)) },
            text = { Text(stringResource(R.string.memory_delete_body)) },
            confirmButton = {
                TextButton(onClick = {
                    memoryToDelete = null
                    val index = memories.indexOfFirst { it.id == memory.id }
                    memories = memories.filterNot { it.id == memory.id }
                    scope.launch {
                        runCatching { deleteMemory(memory) }.onFailure {
                            if (memories.none { it.id == memory.id }) {
                                memories = memories.toMutableList().apply {
                                    add(index.coerceIn(0, size), memory)
                                }
                            }
                            snackbar.showSnackbar(deleteError)
                        }
                    }
                }) { Text(stringResource(R.string.memory_delete_confirm)) }
            },
            dismissButton = {
                TextButton(onClick = { memoryToDelete = null }) {
                    Text(stringResource(R.string.memory_delete_cancel))
                }
            },
        )
    }
}

@Composable
private fun ProfileSection(profile: ConsolidatedProfile?) {
    SectionLabel(stringResource(R.string.memory_profile_heading))
    Spacer(Modifier.height(12.dp))
    if (profile == null) {
        Text(
            text = stringResource(R.string.memory_profile_empty),
            style = MaterialTheme.typography.bodyMedium,
            color = FoyerTextDim,
        )
    } else {
        profile.text.split(Regex("\\n\\s*\\n")).filter(String::isNotBlank).forEach { paragraph ->
            Text(
                text = paragraph.trim(),
                style = MaterialTheme.typography.bodyMedium,
                color = FoyerText,
                modifier = Modifier.padding(bottom = 10.dp),
            )
        }
        Text(
            text = stringResource(R.string.memory_profile_updated, profile.updatedAt),
            style = MaterialTheme.typography.bodySmall,
            color = FoyerTextDim,
        )
    }
}

@Composable
private fun MemoryRow(memory: MemoryRecord, onDelete: () -> Unit) {
    var menuExpanded by remember(memory.id) { mutableStateOf(false) }
    Surface(
        shape = RoundedCornerShape(16.dp),
        color = FoyerSurfaceRaised,
        border = BorderStroke(1.dp, FoyerLine),
    ) {
        Column(modifier = Modifier.padding(14.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Surface(
                    shape = RoundedCornerShape(50),
                    color = FoyerLine,
                ) {
                    Text(
                        text = memory.kind.uppercase(),
                        style = MaterialTheme.typography.labelSmall,
                        color = FoyerText,
                        modifier = Modifier.padding(horizontal = 9.dp, vertical = 5.dp),
                    )
                }
                Spacer(Modifier.weight(1f))
                Box {
                    Box(modifier = Modifier.clickable { menuExpanded = true }.padding(8.dp)) {
                        MoreGlyph()
                    }
                    DropdownMenu(
                        expanded = menuExpanded,
                        onDismissRequest = { menuExpanded = false },
                    ) {
                        DropdownMenuItem(
                            text = { Text(stringResource(R.string.memory_delete_action)) },
                            onClick = { menuExpanded = false; onDelete() },
                        )
                    }
                }
            }
            Text(
                text = memory.content,
                style = MaterialTheme.typography.bodyMedium,
                color = FoyerText,
                modifier = Modifier.padding(top = 10.dp),
            )
            Text(
                text = memory.createdAt,
                style = MaterialTheme.typography.bodySmall,
                color = FoyerTextDim,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.padding(top = 8.dp),
            )
        }
    }
}
