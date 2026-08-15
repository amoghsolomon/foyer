package com.amazity.foyer.ui.components

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.WindowInsetsSides
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.only
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import com.amazity.foyer.model.AgentTask
import com.amazity.foyer.model.TaskState
import com.amazity.foyer.ui.theme.FoyerBlack
import com.amazity.foyer.ui.theme.FoyerLineSubtle
import com.amazity.foyer.ui.theme.FoyerText
import com.amazity.foyer.ui.theme.FoyerTextDim
import com.amazity.foyer.ui.theme.FoyerTextMuted

@Composable
fun FoyerScreen(
    modifier: Modifier = Modifier,
    content: @Composable BoxScope.() -> Unit,
) {
    Box(
        modifier = modifier
            .fillMaxSize()
            .background(FoyerBlack)
            .windowInsetsPadding(WindowInsets.safeDrawing.only(WindowInsetsSides.Vertical)),
        content = content,
    )
}

@Composable
fun SectionLabel(
    text: String,
    modifier: Modifier = Modifier,
) {
    Text(
        text = text.uppercase(),
        style = MaterialTheme.typography.labelSmall,
        color = FoyerTextDim,
        modifier = modifier,
    )
}

@Composable
fun HairlineDivider(modifier: Modifier = Modifier) {
    Spacer(
        modifier = modifier
            .fillMaxWidth()
            .height(1.dp)
            .background(FoyerLineSubtle),
    )
}

@Composable
fun NestedScreenHeader(
    title: String,
    onBack: () -> Unit,
    modifier: Modifier = Modifier,
) {
    Row(
        modifier = modifier
            .fillMaxWidth()
            .height(64.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(
            modifier = Modifier
                .size(40.dp)
                .clickable(onClick = onBack),
            contentAlignment = Alignment.CenterStart,
        ) {
            BackGlyph()
        }
        Text(
            text = title,
            style = MaterialTheme.typography.titleMedium,
            color = FoyerText,
            modifier = Modifier.weight(1f),
        )
        Spacer(Modifier.width(40.dp))
    }
}

@Composable
fun StatusMarker(
    state: TaskState,
    modifier: Modifier = Modifier,
) {
    when (state) {
        TaskState.Done -> Text(
            text = "✓",
            color = FoyerText,
            style = MaterialTheme.typography.labelMedium,
            modifier = modifier.width(14.dp),
        )

        TaskState.Failed -> Text(
            text = "!",
            color = FoyerTextDim,
            style = MaterialTheme.typography.labelMedium,
            modifier = modifier.width(14.dp),
        )

        TaskState.Scheduled -> Box(
            modifier = modifier
                .size(7.dp)
                .drawBehind {
                    drawCircle(
                        color = FoyerTextDim,
                        style = Stroke(width = 1.dp.toPx()),
                    )
                },
        )

        TaskState.Running,
        TaskState.Queued,
        -> Box(
            modifier = modifier
                .size(7.dp)
                .background(
                    color = if (state == TaskState.Running) FoyerText else FoyerTextMuted,
                    shape = androidx.compose.foundation.shape.CircleShape,
                ),
        )
    }
}

@Composable
fun TaskRow(
    task: AgentTask,
    modifier: Modifier = Modifier,
    showChevron: Boolean = false,
    onClick: (() -> Unit)? = null,
) {
    val rowModifier = if (onClick != null) modifier.clickable(onClick = onClick) else modifier

    Row(
        modifier = rowModifier
            .fillMaxWidth()
            .padding(vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(
            modifier = Modifier.width(18.dp),
            contentAlignment = Alignment.CenterStart,
        ) {
            StatusMarker(state = task.state)
        }
        Column(
            modifier = Modifier.weight(1f),
            verticalArrangement = Arrangement.spacedBy(1.dp),
        ) {
            Text(
                text = task.title,
                style = MaterialTheme.typography.bodyMedium,
                color = FoyerText,
                fontWeight = FontWeight.Normal,
            )
            Text(
                text = task.subtitle,
                style = MaterialTheme.typography.bodySmall,
                color = FoyerTextDim,
            )
        }
        if (showChevron) {
            Spacer(Modifier.width(8.dp))
            ChevronGlyph()
        }
    }
}
