package com.amazity.foyer.ui.screens

import android.Manifest
import android.content.pm.PackageManager
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.clickable
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
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.core.content.ContextCompat
import com.amazity.foyer.model.NotesStatus
import com.amazity.foyer.model.VaultFolder
import com.amazity.foyer.model.VaultNote
import com.amazity.foyer.notes.SafeMarkdown
import com.amazity.foyer.ui.components.FoyerScreen
import com.amazity.foyer.ui.components.HairlineDivider
import com.amazity.foyer.ui.components.MicrophoneGlyph
import com.amazity.foyer.ui.components.NestedScreenHeader
import com.amazity.foyer.ui.theme.FoyerBlack
import com.amazity.foyer.ui.theme.FoyerLine
import com.amazity.foyer.ui.theme.FoyerText
import com.amazity.foyer.ui.theme.FoyerTextDim
import com.amazity.foyer.ui.theme.FoyerTextMuted
import com.amazity.foyer.voice.MoonshineVoiceInput
import com.amazity.foyer.voice.VoiceInputState
import kotlinx.coroutines.flow.distinctUntilChanged

@Composable
fun NoteEditorScreen(
    note: VaultNote?,
    folders: List<VaultFolder>,
    initialFolderId: String?,
    status: NotesStatus = NotesStatus(loading = false),
    voiceInput: MoonshineVoiceInput,
    saving: Boolean = false,
    saveError: String? = null,
    onCancel: () -> Unit,
    onSave: (title: String, body: String, folderId: String) -> Unit,
) {
    var title by rememberSaveable(note?.id) { mutableStateOf(note?.title.orEmpty()) }
    var body by rememberSaveable(note?.id) { mutableStateOf(note?.body.orEmpty()) }
    var folderId by rememberSaveable(note?.id) {
        mutableStateOf(note?.folderId ?: initialFolderId ?: folders.firstOrNull()?.id.orEmpty())
    }
    var dictationBase by rememberSaveable(note?.id) { mutableStateOf("") }
    var permissionError by rememberSaveable(note?.id) { mutableStateOf<String?>(null) }
    var previewing by rememberSaveable(note?.id) { mutableStateOf(false) }
    val voiceState by voiceInput.state.collectAsState()
    val voiceActive = voiceState is VoiceInputState.Preparing || voiceState is VoiceInputState.Listening
    val canSave = !saving && !voiceActive && title.isNotBlank() && folderId.isNotBlank()
    val context = LocalContext.current
    val focusManager = LocalFocusManager.current
    val noteScrollState = rememberScrollState()

    fun beginDictation() {
        permissionError = null
        dictationBase = body.trimEnd()
        focusManager.clearFocus(force = true)
        voiceInput.start { transcript ->
            body = mergeDictation(dictationBase, transcript)
        }
    }

    val permissionLauncher = rememberLauncherForActivityResult(
        ActivityResultContracts.RequestPermission(),
    ) { granted ->
        if (granted) beginDictation() else permissionError = "Allow microphone access to dictate"
    }
    val onMicrophoneClick = {
        if (ContextCompat.checkSelfPermission(context, Manifest.permission.RECORD_AUDIO) == PackageManager.PERMISSION_GRANTED) {
            beginDictation()
        } else {
            permissionLauncher.launch(Manifest.permission.RECORD_AUDIO)
        }
    }

    DisposableEffect(voiceInput) {
        onDispose { voiceInput.stop() }
    }
    LaunchedEffect(voiceState is VoiceInputState.Listening) {
        if (voiceState is VoiceInputState.Listening) {
            snapshotFlow { noteScrollState.maxValue }
                .distinctUntilChanged()
                .collect { maximum -> noteScrollState.animateScrollTo(maximum) }
        }
    }

    FoyerScreen {
        Column(
            modifier = Modifier
                .fillMaxSize()
                .imePadding()
                .padding(horizontal = 24.dp),
        ) {
            NestedScreenHeader(
                title = if (note == null) "New note" else "Edit note",
                onBack = onCancel,
            )
            HairlineDivider()
            Column(
                modifier = Modifier
                    .weight(1f)
                    .verticalScroll(noteScrollState),
            ) {
                Spacer(Modifier.height(22.dp))
                NotesStatusBanner(status)
                BasicTextField(
                    value = title,
                    onValueChange = { title = it },
                    readOnly = voiceActive,
                    textStyle = MaterialTheme.typography.headlineMedium.copy(color = FoyerText),
                    cursorBrush = SolidColor(FoyerText),
                    modifier = Modifier.fillMaxWidth(),
                    decorationBox = { inner ->
                        Box {
                            if (title.isEmpty()) Text("Title", style = MaterialTheme.typography.headlineMedium, color = FoyerTextDim)
                            inner()
                        }
                    },
                )
                Spacer(Modifier.height(18.dp))
                Text("FOLDER", style = MaterialTheme.typography.labelSmall, color = FoyerTextDim)
                Spacer(Modifier.height(8.dp))
                Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    folders.forEach { folder ->
                        val selected = folder.id == folderId
                        val label = folderPathLabel(folders, folder.id)
                        Surface(
                            modifier = Modifier.clickable(enabled = !voiceActive) { folderId = folder.id },
                            shape = RoundedCornerShape(16.dp),
                            color = if (selected) FoyerText else FoyerBlack,
                            contentColor = if (selected) FoyerBlack else FoyerText,
                            border = BorderStroke(1.dp, if (selected) FoyerText else FoyerLine),
                        ) {
                            Text(label, style = MaterialTheme.typography.labelMedium, modifier = Modifier.padding(horizontal = 11.dp, vertical = 7.dp))
                        }
                    }
                }
                Spacer(Modifier.height(22.dp))
                HairlineDivider()
                Spacer(Modifier.height(16.dp))
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    EditorModeChip("Source", selected = !previewing, enabled = true) { previewing = false }
                    EditorModeChip("Preview", selected = previewing, enabled = true) { previewing = true }
                }
                Spacer(Modifier.height(16.dp))
                if (previewing) {
                    if (body.isBlank()) {
                        Text("Nothing to preview yet.", style = MaterialTheme.typography.bodyLarge, color = FoyerTextDim)
                    } else {
                        SafeMarkdown(body)
                    }
                } else {
                    BasicTextField(
                        value = body,
                        onValueChange = { body = it },
                        readOnly = voiceActive,
                        textStyle = MaterialTheme.typography.bodyLarge.copy(color = FoyerText),
                        cursorBrush = SolidColor(FoyerText),
                        modifier = Modifier.fillMaxWidth().heightIn(min = 360.dp),
                        decorationBox = { inner ->
                            Box {
                                if (body.isEmpty()) Text("Write in Markdown…", style = MaterialTheme.typography.bodyLarge, color = FoyerTextDim)
                                inner()
                            }
                        },
                    )
                }
            }
            NoteEditorOmniBar(
                voiceState = voiceState,
                errorMessage = saveError ?: permissionError,
                saving = saving,
                canSave = canSave,
                onCancel = onCancel,
                onMicrophoneClick = onMicrophoneClick,
                onStop = voiceInput::stop,
                onSave = { onSave(title.trim(), body, folderId) },
                modifier = Modifier.padding(vertical = 12.dp),
            )
        }
    }
}

@Composable
private fun NoteEditorOmniBar(
    voiceState: VoiceInputState,
    errorMessage: String?,
    saving: Boolean,
    canSave: Boolean,
    onCancel: () -> Unit,
    onMicrophoneClick: () -> Unit,
    onStop: () -> Unit,
    onSave: () -> Unit,
    modifier: Modifier = Modifier,
) {
    Surface(
        modifier = modifier.fillMaxWidth().height(58.dp),
        shape = RoundedCornerShape(29.dp),
        color = FoyerBlack,
        contentColor = FoyerText,
        border = BorderStroke(1.dp, FoyerLine),
    ) {
        Row(
            modifier = Modifier.fillMaxSize().padding(horizontal = 6.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            when (voiceState) {
                is VoiceInputState.Preparing -> {
                    OmniTextAction("Cancel", onCancel)
                    Text(
                        text = "Preparing voice · ${(voiceState.progress * 100).toInt()}%",
                        style = MaterialTheme.typography.labelMedium,
                        color = FoyerTextMuted,
                        modifier = Modifier.weight(1f),
                    )
                    CircularVoiceButton(onClick = onStop, contentDescription = "Cancel voice model preparation") {
                        StopGlyph()
                    }
                }

                is VoiceInputState.Listening -> {
                    Text(
                        text = formatDuration(voiceState.elapsedMillis),
                        style = MaterialTheme.typography.labelMedium,
                        color = FoyerTextMuted,
                        modifier = Modifier.padding(start = 10.dp),
                    )
                    LiveWaveform(
                        levels = voiceState.levels,
                        modifier = Modifier.weight(1f).height(32.dp).padding(horizontal = 13.dp),
                    )
                    CircularVoiceButton(onClick = onStop, contentDescription = "Stop dictation") {
                        StopGlyph()
                    }
                }

                VoiceInputState.Idle, is VoiceInputState.Error -> {
                    OmniTextAction("Cancel", onCancel)
                    Text(
                        text = errorMessage
                            ?: (voiceState as? VoiceInputState.Error)?.message
                            ?: if (saving) "Saving to server…" else null
                            ?: "Write or dictate",
                        style = MaterialTheme.typography.labelMedium,
                        color = if (errorMessage != null || voiceState is VoiceInputState.Error) FoyerTextMuted else FoyerTextDim,
                        maxLines = 1,
                        modifier = Modifier.weight(1f).padding(horizontal = 6.dp),
                    )
                    CircularVoiceButton(onClick = onMicrophoneClick, contentDescription = "Start dictation") {
                        MicrophoneGlyph()
                    }
                    Spacer(Modifier.width(6.dp))
                    Surface(
                        modifier = Modifier.height(44.dp).width(68.dp).clickable(enabled = canSave, onClick = onSave),
                        shape = RoundedCornerShape(22.dp),
                        color = if (canSave) FoyerText else FoyerLine,
                        contentColor = if (canSave) FoyerBlack else FoyerTextDim,
                    ) {
                        Box(contentAlignment = Alignment.Center) {
                            Text(if (saving) "Saving" else "Save", style = MaterialTheme.typography.labelMedium)
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun OmniTextAction(label: String, onClick: () -> Unit) {
    Box(
        modifier = Modifier.height(44.dp).clickable(onClick = onClick).padding(horizontal = 10.dp),
        contentAlignment = Alignment.Center,
    ) {
        Text(label, style = MaterialTheme.typography.labelMedium, color = FoyerTextMuted)
    }
}

@Composable
private fun CircularVoiceButton(
    onClick: () -> Unit,
    contentDescription: String,
    content: @Composable () -> Unit,
) {
    Surface(
        modifier = Modifier
            .size(44.dp)
            .semantics { this.contentDescription = contentDescription }
            .clickable(onClick = onClick),
        shape = RoundedCornerShape(22.dp),
        color = FoyerText,
        contentColor = FoyerBlack,
    ) {
        Box(contentAlignment = Alignment.Center, content = { content() })
    }
}

@Composable
private fun LiveWaveform(levels: List<Float>, modifier: Modifier = Modifier) {
    Canvas(modifier = modifier.semantics { contentDescription = "Live microphone waveform" }) {
        val barCount = 28
        val gap = size.width / barCount
        val visible = List(barCount) { index ->
            levels.getOrNull(levels.size - barCount + index) ?: 0.035f
        }
        visible.forEachIndexed { index, level ->
            val barHeight = (size.height * (0.13f + level * 0.87f)).coerceAtLeast(3.dp.toPx())
            val x = gap * index + gap / 2f
            drawLine(
                color = FoyerText.copy(alpha = 0.45f + level * 0.55f),
                start = Offset(x, (size.height - barHeight) / 2f),
                end = Offset(x, (size.height + barHeight) / 2f),
                strokeWidth = minOf(2.4.dp.toPx(), gap * 0.55f),
                cap = StrokeCap.Round,
            )
        }
    }
}

@Composable
private fun StopGlyph() {
    Canvas(Modifier.size(16.dp)) {
        drawRoundRect(
            color = FoyerBlack,
            topLeft = Offset(3.dp.toPx(), 3.dp.toPx()),
            size = androidx.compose.ui.geometry.Size(10.dp.toPx(), 10.dp.toPx()),
            cornerRadius = androidx.compose.ui.geometry.CornerRadius(2.dp.toPx()),
        )
    }
}

@Composable
internal fun EditorModeChip(
    label: String,
    selected: Boolean,
    enabled: Boolean,
    onClick: () -> Unit,
) {
    Surface(
        modifier = Modifier.clickable(enabled = enabled, onClick = onClick),
        shape = RoundedCornerShape(16.dp),
        color = if (selected) FoyerText else FoyerBlack,
        contentColor = if (selected) FoyerBlack else FoyerText,
        border = BorderStroke(1.dp, if (selected) FoyerText else FoyerLine),
    ) {
        Text(label, style = MaterialTheme.typography.labelMedium, modifier = Modifier.padding(horizontal = 11.dp, vertical = 7.dp))
    }
}

internal fun folderPathLabel(folders: List<VaultFolder>, folderId: String): String {
    val byId = folders.associateBy(VaultFolder::id)
    val path = ArrayList<String>()
    val seen = HashSet<String>()
    var current = byId[folderId]
    while (current != null && seen.add(current.id)) {
        path.add(0, current.name)
        current = current.parentId?.let(byId::get)
    }
    return path.joinToString(" / ").ifBlank { folderId }
}

internal fun mergeDictation(base: String, transcript: String): String = when {
    base.isBlank() -> transcript
    transcript.isBlank() -> base
    else -> "${base.trimEnd()}\n\n${transcript.trimStart()}"
}

private fun formatDuration(elapsedMillis: Long): String {
    val totalSeconds = elapsedMillis / 1_000
    return "%d:%02d".format(totalSeconds / 60, totalSeconds % 60)
}
