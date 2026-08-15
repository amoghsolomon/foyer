package com.amazity.foyer.ui.screens

import androidx.compose.foundation.BorderStroke
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
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import com.amazity.foyer.model.NotesStatus
import com.amazity.foyer.model.VaultFolder
import com.amazity.foyer.model.VaultNote
import com.amazity.foyer.ui.components.FoyerScreen
import com.amazity.foyer.ui.components.HairlineDivider
import com.amazity.foyer.ui.components.NestedScreenHeader
import com.amazity.foyer.ui.components.SectionLabel
import com.amazity.foyer.ui.components.SpeakerGlyph
import com.amazity.foyer.ui.theme.FoyerBlack
import com.amazity.foyer.ui.theme.FoyerLine
import com.amazity.foyer.ui.theme.FoyerText
import com.amazity.foyer.ui.theme.FoyerTextDim
import com.amazity.foyer.ui.theme.FoyerTextMuted
import com.amazity.foyer.notes.SafeMarkdown
import com.amazity.foyer.voice.MoonshineKokoroReadAloud
import com.amazity.foyer.voice.ReadAloudState

@Composable
fun NoteDetailScreen(
    note: VaultNote,
    folder: VaultFolder?,
    status: NotesStatus = NotesStatus(loading = false),
    readAloud: MoonshineKokoroReadAloud,
    onBack: () -> Unit,
    onEdit: () -> Unit,
    onDelete: () -> Unit,
) {
    var confirmDelete by remember(note.id) { mutableStateOf(false) }
    var showingSource by remember(note.id) { mutableStateOf(false) }
    val readAloudState by readAloud.state.collectAsState()
    val metadata = buildString {
        append(note.updatedLabel)
        if (note.tags.isNotEmpty()) append(" · tags: ${note.tags.joinToString()}")
    }

    DisposableEffect(readAloud, note.id) {
        onDispose { readAloud.stop() }
    }

    FoyerScreen {
        Column(modifier = Modifier.fillMaxSize().padding(horizontal = 24.dp)) {
            NestedScreenHeader(title = note.title, onBack = onBack)
            HairlineDivider()
            Column(
                modifier = Modifier
                    .weight(1f)
                    .verticalScroll(rememberScrollState()),
            ) {
                Spacer(Modifier.height(18.dp))
                NotesStatusBanner(status)
                Text(note.title, style = MaterialTheme.typography.headlineMedium, color = FoyerText)
                Spacer(Modifier.height(7.dp))
                Text(metadata, style = MaterialTheme.typography.bodySmall, color = FoyerTextDim)
                Spacer(Modifier.height(14.dp))
                ReadAloudButton(
                    state = readAloudState,
                    onClick = {
                        when (readAloudState) {
                            ReadAloudState.Idle, is ReadAloudState.Error -> readAloud.read(readableNoteText(note))
                            is ReadAloudState.Preparing, ReadAloudState.Speaking -> readAloud.stop()
                        }
                    },
                )
                Spacer(Modifier.height(20.dp))
                HairlineDivider()
                Spacer(Modifier.height(16.dp))
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    EditorModeChip("Preview", selected = !showingSource, enabled = true) { showingSource = false }
                    EditorModeChip("Source", selected = showingSource, enabled = true) { showingSource = true }
                }
                Spacer(Modifier.height(16.dp))
                if (showingSource) {
                    Text(note.body, style = MaterialTheme.typography.bodyLarge, color = FoyerTextMuted)
                } else {
                    SafeMarkdown(note.body)
                }

                if (note.linkedFrom.isNotEmpty()) {
                    Spacer(Modifier.height(72.dp))
                    HairlineDivider()
                    Spacer(Modifier.height(16.dp))
                    SectionLabel("Linked from · ${note.linkedFrom.size}")
                    Spacer(Modifier.height(12.dp))
                    note.linkedFrom.forEach { backlink ->
                        Text(
                            text = backlink,
                            style = MaterialTheme.typography.bodyMedium,
                            color = FoyerText,
                            modifier = Modifier.fillMaxWidth().padding(vertical = 7.dp),
                        )
                    }
                }
                folder?.let {
                    Spacer(Modifier.height(32.dp))
                    HairlineDivider()
                    Spacer(Modifier.height(14.dp))
                    Text(
                        text = "Folder · ${it.name}",
                        style = MaterialTheme.typography.bodySmall,
                        color = FoyerTextDim,
                    )
                }
                Spacer(Modifier.height(24.dp))
            }
            HairlineDivider()
            Row(
                modifier = Modifier.fillMaxWidth().padding(vertical = 12.dp),
                horizontalArrangement = Arrangement.spacedBy(10.dp),
            ) {
                NoteActionButton("Delete", filled = false, onClick = { confirmDelete = true }, modifier = Modifier.weight(1f))
                NoteActionButton("Edit", filled = true, onClick = onEdit, modifier = Modifier.weight(1f))
            }
        }
    }

    if (confirmDelete) {
        AlertDialog(
            onDismissRequest = { confirmDelete = false },
            title = { Text("Delete note?", color = FoyerText) },
            text = { Text("“${note.title}” will be removed from your server vault.", color = FoyerTextMuted) },
            confirmButton = {
                Text(
                    "Delete",
                    style = MaterialTheme.typography.labelMedium,
                    color = FoyerText,
                    modifier = Modifier.clickable {
                        confirmDelete = false
                        onDelete()
                    }.padding(16.dp),
                )
            },
            dismissButton = {
                Text(
                    "Cancel",
                    style = MaterialTheme.typography.labelMedium,
                    color = FoyerTextMuted,
                    modifier = Modifier.clickable { confirmDelete = false }.padding(16.dp),
                )
            },
        )
    }
}

@Composable
private fun ReadAloudButton(
    state: ReadAloudState,
    onClick: () -> Unit,
) {
    val label = when (state) {
        ReadAloudState.Idle -> "Read aloud"
        is ReadAloudState.Preparing -> "Preparing read aloud · ${(state.progress * 100).toInt()}%"
        ReadAloudState.Speaking -> "Reading aloud"
        is ReadAloudState.Error -> state.message
    }
    val active = state is ReadAloudState.Preparing || state is ReadAloudState.Speaking
    Surface(
        modifier = Modifier.fillMaxWidth().height(46.dp).clickable(onClick = onClick),
        shape = RoundedCornerShape(23.dp),
        color = FoyerBlack,
        contentColor = FoyerText,
        border = BorderStroke(1.dp, if (active) FoyerTextMuted else FoyerLine),
    ) {
        Row(
            modifier = Modifier.fillMaxSize().padding(horizontal = 15.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            SpeakerGlyph(color = if (active) FoyerText else FoyerTextMuted)
            Spacer(Modifier.size(10.dp))
            Text(
                text = label,
                style = MaterialTheme.typography.labelMedium,
                color = if (state is ReadAloudState.Error) FoyerTextMuted else FoyerText,
                maxLines = 1,
                modifier = Modifier.weight(1f),
            )
            if (active) {
                Text("Stop", style = MaterialTheme.typography.labelMedium, color = FoyerText)
            }
        }
    }
}

@Composable
private fun NoteActionButton(
    label: String,
    filled: Boolean,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
) {
    Surface(
        modifier = modifier.height(50.dp).clickable(onClick = onClick),
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

private fun styledWikilinks(body: String): AnnotatedString = buildAnnotatedString {
    var cursor = 0
    wikilink.findAll(body).forEach { match ->
        append(body.substring(cursor, match.range.first))
        withStyle(SpanStyle(color = FoyerText, textDecoration = TextDecoration.Underline)) {
            append(match.groupValues[1])
        }
        cursor = match.range.last + 1
    }
    append(body.substring(cursor))
}

private val wikilink = Regex("\\[\\[([^]]+)]]")

internal fun readableNoteText(note: VaultNote): String {
    val readableBody = note.body
        .replace(wikilink, "$1")
        .replace(Regex("(?m)^\\s{0,3}[#>*+-]+\\s*"), "")
        .replace(Regex("[`*_~]"), "")
        .replace(Regex("\\s+"), " ")
        .trim()
    return listOf(note.title.trim(), readableBody).filter(String::isNotBlank).joinToString(". ")
}
