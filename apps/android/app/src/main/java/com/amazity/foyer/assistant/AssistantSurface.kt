package com.amazity.foyer.assistant

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
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import com.amazity.foyer.ui.components.MicrophoneGlyph
import com.amazity.foyer.ui.components.RichAssistantMessage
import com.amazity.foyer.ui.components.AssistantReadAloudButton
import com.amazity.foyer.ui.theme.FoyerBlack
import com.amazity.foyer.ui.theme.FoyerLine
import com.amazity.foyer.ui.theme.FoyerSurface
import com.amazity.foyer.ui.theme.FoyerText
import com.amazity.foyer.ui.theme.FoyerTextDim
import com.amazity.foyer.ui.theme.FoyerTextMuted
import com.amazity.foyer.voice.ReadAloudState

@Composable
fun AssistantOverlayHost(
    state: AssistantUiState,
    onInputChange: (String) -> Unit,
    onToggleListening: () -> Unit,
    onSubmit: () -> Unit,
    onConfirm: () -> Unit,
    onCancelAction: () -> Unit,
    onDismiss: () -> Unit,
    readAloudState: ReadAloudState,
    activeReadAloudMessageId: String?,
    onToggleReadAloud: (String, String) -> Unit,
) {
    if (!state.visible) return
    Box(
        modifier = Modifier
            .fillMaxSize()
            .background(FoyerBlack.copy(alpha = 0.56f))
            .padding(horizontal = 12.dp),
        contentAlignment = Alignment.BottomCenter,
    ) {
        AssistantSurface(
            state = state,
            onInputChange = onInputChange,
            onToggleListening = onToggleListening,
            onSubmit = onSubmit,
            onConfirm = onConfirm,
            onCancelAction = onCancelAction,
            onDismiss = onDismiss,
            readAloudState = readAloudState,
            activeReadAloudMessageId = activeReadAloudMessageId,
            onToggleReadAloud = onToggleReadAloud,
            modifier = Modifier.navigationBarsPadding(),
        )
    }
}

@Composable
fun AssistantSurface(
    state: AssistantUiState,
    onInputChange: (String) -> Unit,
    onToggleListening: () -> Unit,
    onSubmit: () -> Unit,
    onConfirm: () -> Unit,
    onCancelAction: () -> Unit,
    onDismiss: () -> Unit,
    readAloudState: ReadAloudState,
    activeReadAloudMessageId: String?,
    onToggleReadAloud: (String, String) -> Unit,
    modifier: Modifier = Modifier,
) {
    Surface(
        modifier = modifier
            .fillMaxWidth()
            .padding(bottom = 10.dp),
        color = FoyerSurface,
        contentColor = FoyerText,
        shape = RoundedCornerShape(28.dp),
        border = androidx.compose.foundation.BorderStroke(1.dp, FoyerLine),
        shadowElevation = 14.dp,
    ) {
        Column(modifier = Modifier.padding(18.dp)) {
            AssistantHeader(state = state, onDismiss = onDismiss)

            if (state.messages.isNotEmpty()) {
                Spacer(Modifier.height(14.dp))
                Column(
                    modifier = Modifier
                        .fillMaxWidth()
                        .heightIn(max = 220.dp)
                        .verticalScroll(rememberScrollState()),
                    verticalArrangement = Arrangement.spacedBy(10.dp),
                ) {
                    state.messages.takeLast(4).forEach { message ->
                        if (message.role == AssistantMessageRole.User) {
                            Text(
                                text = message.text,
                                style = MaterialTheme.typography.bodyMedium,
                                color = FoyerTextDim,
                                modifier = Modifier.padding(start = 24.dp),
                            )
                        } else {
                            val readAloudId = "assistant:${message.id}"
                            Column(modifier = Modifier.padding(end = 24.dp)) {
                                RichAssistantMessage(text = message.text, color = FoyerText)
                                if (state.phase != AssistantPhase.Sending && message.text.isNotBlank()) {
                                    Row(modifier = Modifier.align(Alignment.End)) {
                                        AssistantReadAloudButton(
                                            active = activeReadAloudMessageId == readAloudId &&
                                                readAloudState.isActiveReadAloud(),
                                            onClick = {
                                                onToggleReadAloud(readAloudId, message.text)
                                            },
                                        )
                                    }
                                }
                            }
                        }
                    }
                }
            }

            state.pendingAction?.let { action ->
                Spacer(Modifier.height(14.dp))
                ConfirmationCard(
                    summary = action.summary(),
                    onCancel = onCancelAction,
                    onConfirm = onConfirm,
                )
            }

            state.errorMessage?.let { error ->
                Spacer(Modifier.height(12.dp))
                Text(
                    text = error,
                    style = MaterialTheme.typography.bodySmall,
                    color = FoyerText,
                )
            }

            Spacer(Modifier.height(14.dp))
            Composer(
                state = state,
                onInputChange = onInputChange,
                onToggleListening = onToggleListening,
                onSubmit = onSubmit,
            )
        }
    }
}

private fun ReadAloudState.isActiveReadAloud(): Boolean =
    this is ReadAloudState.Preparing || this is ReadAloudState.Speaking

@Composable
private fun AssistantHeader(
    state: AssistantUiState,
    onDismiss: () -> Unit,
) {
    Row(
        modifier = Modifier.fillMaxWidth(),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(
            modifier = Modifier
                .size(8.dp)
                .background(
                    if (state.phase == AssistantPhase.Listening) FoyerText else FoyerTextMuted,
                    CircleShape,
                ),
        )
        Spacer(Modifier.width(9.dp))
        Text(
            text = statusText(state),
            style = MaterialTheme.typography.labelMedium,
            color = FoyerText,
            modifier = Modifier.weight(1f),
        )
        Box(
            modifier = Modifier
                .size(36.dp)
                .clip(CircleShape)
                .clickable(onClick = onDismiss)
                .semantics { contentDescription = "Close assistant" },
            contentAlignment = Alignment.Center,
        ) {
            Text("×", style = MaterialTheme.typography.titleMedium, color = FoyerTextDim)
        }
    }
}

@Composable
private fun Composer(
    state: AssistantUiState,
    onInputChange: (String) -> Unit,
    onToggleListening: () -> Unit,
    onSubmit: () -> Unit,
) {
    val busy = state.phase == AssistantPhase.Sending || state.phase == AssistantPhase.Executing
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(20.dp))
            .background(FoyerBlack)
            .border(1.dp, FoyerLine, RoundedCornerShape(20.dp))
            .padding(14.dp),
    ) {
        BasicTextField(
            value = state.input,
            onValueChange = onInputChange,
            enabled = !busy,
            textStyle = MaterialTheme.typography.bodyLarge.copy(color = FoyerText),
            cursorBrush = SolidColor(FoyerText),
            modifier = Modifier
                .fillMaxWidth()
                .heightIn(min = 56.dp, max = 150.dp),
            decorationBox = { inner ->
                Box {
                    if (state.input.isBlank()) {
                        Text(
                            text = "Ask Foyer anything",
                            style = MaterialTheme.typography.bodyLarge,
                            color = FoyerTextMuted,
                        )
                    }
                    inner()
                }
            },
        )

        if (state.phase == AssistantPhase.Listening && state.levels.isNotEmpty()) {
            VoiceLevels(state.levels)
            Spacer(Modifier.height(10.dp))
        }

        Row(
            modifier = Modifier.fillMaxWidth(),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            RoundActionButton(
                onClick = onToggleListening,
                enabled = !busy,
                description = if (state.phase == AssistantPhase.Listening) {
                    "Stop transcription"
                } else {
                    "Start transcription"
                },
            ) {
                if (state.phase == AssistantPhase.Listening || state.phase == AssistantPhase.Preparing) {
                    Box(
                        Modifier
                            .size(12.dp)
                            .background(FoyerText, RoundedCornerShape(2.dp)),
                    )
                } else {
                    MicrophoneGlyph()
                }
            }
            Spacer(Modifier.weight(1f))
            Text(
                text = when (state.phase) {
                    AssistantPhase.Listening -> elapsed(state.elapsedMillis)
                    AssistantPhase.Preparing -> "Loading ${
                        (state.preparationProgress * 100).toInt().coerceIn(0, 100)
                    }%"
                    AssistantPhase.Sending -> "Sending"
                    AssistantPhase.Executing -> "Acting"
                    else -> ""
                },
                style = MaterialTheme.typography.bodySmall,
                color = FoyerTextDim,
            )
            Spacer(Modifier.width(10.dp))
            RoundActionButton(
                onClick = onSubmit,
                enabled = state.input.isNotBlank() && !busy,
                filled = true,
                description = "Send request",
            ) {
                Text(
                    text = "↑",
                    style = MaterialTheme.typography.titleMedium,
                    fontWeight = FontWeight.Bold,
                    color = FoyerBlack,
                )
            }
        }
    }
}

@Composable
private fun VoiceLevels(levels: List<Float>) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .height(24.dp),
        horizontalArrangement = Arrangement.spacedBy(3.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        levels.takeLast(28).forEach { level ->
            Box(
                modifier = Modifier
                    .weight(1f)
                    .height((4 + 20 * level).dp)
                    .background(FoyerText.copy(alpha = 0.8f), RoundedCornerShape(2.dp)),
            )
        }
    }
}

@Composable
private fun ConfirmationCard(
    summary: String,
    onCancel: () -> Unit,
    onConfirm: () -> Unit,
) {
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(18.dp))
            .border(1.dp, FoyerLine, RoundedCornerShape(18.dp))
            .padding(14.dp),
    ) {
        Text(summary, style = MaterialTheme.typography.bodyMedium, color = FoyerText)
        Spacer(Modifier.height(12.dp))
        Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
            TextActionButton("Cancel", filled = false, onClick = onCancel, modifier = Modifier.weight(1f))
            TextActionButton("Confirm", filled = true, onClick = onConfirm, modifier = Modifier.weight(1f))
        }
    }
}

@Composable
private fun RoundActionButton(
    onClick: () -> Unit,
    enabled: Boolean,
    description: String,
    filled: Boolean = false,
    content: @Composable () -> Unit,
) {
    Surface(
        modifier = Modifier
            .size(42.dp)
            .clip(CircleShape)
            .clickable(enabled = enabled, onClick = onClick)
            .semantics { contentDescription = description },
        color = if (filled) FoyerText else FoyerBlack,
        contentColor = if (filled) FoyerBlack else FoyerText,
        shape = CircleShape,
        border = androidx.compose.foundation.BorderStroke(1.dp, FoyerLine),
    ) {
        Box(contentAlignment = Alignment.Center) { content() }
    }
}

@Composable
private fun TextActionButton(
    label: String,
    filled: Boolean,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
) {
    Surface(
        modifier = modifier
            .height(44.dp)
            .clickable(onClick = onClick),
        color = if (filled) FoyerText else FoyerBlack,
        contentColor = if (filled) FoyerBlack else FoyerText,
        shape = RoundedCornerShape(22.dp),
        border = androidx.compose.foundation.BorderStroke(1.dp, FoyerLine),
    ) {
        Box(contentAlignment = Alignment.Center) {
            Text(label, style = MaterialTheme.typography.labelMedium)
        }
    }
}

private fun statusText(state: AssistantUiState): String = when (state.phase) {
    AssistantPhase.Ready -> "Ready"
    AssistantPhase.Preparing -> "Preparing transcription"
    AssistantPhase.Listening -> "Listening"
    AssistantPhase.Sending -> "Thinking"
    AssistantPhase.AwaitingConfirmation -> "Confirm this action"
    AssistantPhase.Executing -> "Working"
    AssistantPhase.Complete -> "Anything else?"
    AssistantPhase.Error -> "Needs attention"
}

private fun elapsed(millis: Long): String {
    val seconds = millis.coerceAtLeast(0) / 1_000
    return "%d:%02d".format(seconds / 60, seconds % 60)
}
