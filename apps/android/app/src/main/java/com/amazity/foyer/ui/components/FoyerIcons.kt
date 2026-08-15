package com.amazity.foyer.ui.components

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import com.amazity.foyer.ui.theme.FoyerTextMuted

@Composable
fun SearchGlyph(
    modifier: Modifier = Modifier,
    color: Color = FoyerTextMuted,
) {
    Canvas(
        modifier = modifier
            .size(18.dp)
            .semantics { contentDescription = "Search" },
    ) {
        val stroke = 1.7.dp.toPx()
        drawCircle(
            color = color,
            radius = size.minDimension * 0.28f,
            center = Offset(size.width * 0.43f, size.height * 0.43f),
            style = Stroke(width = stroke),
        )
        drawLine(
            color = color,
            start = Offset(size.width * 0.63f, size.height * 0.63f),
            end = Offset(size.width * 0.86f, size.height * 0.86f),
            strokeWidth = stroke,
            cap = StrokeCap.Round,
        )
    }
}

@Composable
fun MicrophoneGlyph(
    modifier: Modifier = Modifier,
    color: Color = FoyerTextMuted,
) {
    Canvas(
        modifier = modifier
            .size(20.dp)
            .semantics { contentDescription = "Record voice" },
    ) {
        val stroke = 1.6.dp.toPx()
        drawRoundRect(
            color = color,
            topLeft = Offset(size.width * 0.37f, size.height * 0.08f),
            size = Size(size.width * 0.26f, size.height * 0.52f),
            cornerRadius = CornerRadius(size.width * 0.14f),
            style = Stroke(width = stroke),
        )
        drawArc(
            color = color,
            startAngle = 0f,
            sweepAngle = 180f,
            useCenter = false,
            topLeft = Offset(size.width * 0.23f, size.height * 0.27f),
            size = Size(size.width * 0.54f, size.height * 0.48f),
            style = Stroke(width = stroke, cap = StrokeCap.Round),
        )
        drawLine(
            color = color,
            start = Offset(size.width * 0.5f, size.height * 0.75f),
            end = Offset(size.width * 0.5f, size.height * 0.91f),
            strokeWidth = stroke,
            cap = StrokeCap.Round,
        )
        drawLine(
            color = color,
            start = Offset(size.width * 0.34f, size.height * 0.91f),
            end = Offset(size.width * 0.66f, size.height * 0.91f),
            strokeWidth = stroke,
            cap = StrokeCap.Round,
        )
    }
}

@Composable
fun SpeakerGlyph(
    modifier: Modifier = Modifier,
    color: Color = FoyerTextMuted,
) {
    Canvas(
        modifier = modifier
            .size(20.dp)
            .semantics { contentDescription = "Read aloud" },
    ) {
        val stroke = 1.6.dp.toPx()
        val speaker = Path().apply {
            moveTo(size.width * 0.12f, size.height * 0.40f)
            lineTo(size.width * 0.32f, size.height * 0.40f)
            lineTo(size.width * 0.54f, size.height * 0.20f)
            lineTo(size.width * 0.54f, size.height * 0.80f)
            lineTo(size.width * 0.32f, size.height * 0.60f)
            lineTo(size.width * 0.12f, size.height * 0.60f)
            close()
        }
        drawPath(speaker, color = color, style = Stroke(width = stroke, join = androidx.compose.ui.graphics.StrokeJoin.Round))
        drawArc(
            color = color,
            startAngle = -55f,
            sweepAngle = 110f,
            useCenter = false,
            topLeft = Offset(size.width * 0.44f, size.height * 0.29f),
            size = Size(size.width * 0.32f, size.height * 0.42f),
            style = Stroke(width = stroke, cap = StrokeCap.Round),
        )
        drawArc(
            color = color,
            startAngle = -48f,
            sweepAngle = 96f,
            useCenter = false,
            topLeft = Offset(size.width * 0.42f, size.height * 0.16f),
            size = Size(size.width * 0.50f, size.height * 0.68f),
            style = Stroke(width = stroke, cap = StrokeCap.Round),
        )
    }
}

@Composable
fun ChevronGlyph(
    modifier: Modifier = Modifier,
    color: Color = FoyerTextMuted,
) {
    Canvas(
        modifier = modifier
            .size(14.dp)
            .semantics { contentDescription = "Open" },
    ) {
        val stroke = 1.4.dp.toPx()
        drawLine(
            color = color,
            start = Offset(size.width * 0.38f, size.height * 0.25f),
            end = Offset(size.width * 0.62f, size.height * 0.5f),
            strokeWidth = stroke,
            cap = StrokeCap.Round,
        )
        drawLine(
            color = color,
            start = Offset(size.width * 0.62f, size.height * 0.5f),
            end = Offset(size.width * 0.38f, size.height * 0.75f),
            strokeWidth = stroke,
            cap = StrokeCap.Round,
        )
    }
}

@Composable
fun BackGlyph(
    modifier: Modifier = Modifier,
    color: Color = FoyerTextMuted,
) {
    Canvas(
        modifier = modifier
            .size(18.dp)
            .semantics { contentDescription = "Back" },
    ) {
        val stroke = 1.6.dp.toPx()
        drawLine(
            color = color,
            start = Offset(size.width * 0.68f, size.height * 0.2f),
            end = Offset(size.width * 0.34f, size.height * 0.5f),
            strokeWidth = stroke,
            cap = StrokeCap.Round,
        )
        drawLine(
            color = color,
            start = Offset(size.width * 0.34f, size.height * 0.5f),
            end = Offset(size.width * 0.68f, size.height * 0.8f),
            strokeWidth = stroke,
            cap = StrokeCap.Round,
        )
    }
}

@Composable
fun PlusGlyph(
    modifier: Modifier = Modifier,
    color: Color = FoyerTextMuted,
) {
    Canvas(
        modifier = modifier
            .size(20.dp)
            .semantics { contentDescription = "Add note" },
    ) {
        val stroke = 1.5.dp.toPx()
        drawLine(
            color = color,
            start = Offset(size.width * 0.5f, size.height * 0.18f),
            end = Offset(size.width * 0.5f, size.height * 0.82f),
            strokeWidth = stroke,
            cap = StrokeCap.Round,
        )
        drawLine(
            color = color,
            start = Offset(size.width * 0.18f, size.height * 0.5f),
            end = Offset(size.width * 0.82f, size.height * 0.5f),
            strokeWidth = stroke,
            cap = StrokeCap.Round,
        )
    }
}

@Composable
fun RunNowGlyph(
    modifier: Modifier = Modifier,
    color: Color = FoyerTextMuted,
) {
    Canvas(modifier = modifier.size(20.dp)) {
        val play = Path().apply {
            moveTo(size.width * 0.34f, size.height * 0.23f)
            lineTo(size.width * 0.78f, size.height * 0.5f)
            lineTo(size.width * 0.34f, size.height * 0.77f)
            close()
        }
        drawPath(play, color = color)
    }
}

@Composable
fun ScheduleGlyph(
    modifier: Modifier = Modifier,
    color: Color = FoyerTextMuted,
) {
    Canvas(modifier = modifier.size(20.dp)) {
        val stroke = 1.6.dp.toPx()
        drawRoundRect(
            color = color,
            topLeft = Offset(size.width * 0.14f, size.height * 0.2f),
            size = Size(size.width * 0.72f, size.height * 0.66f),
            cornerRadius = CornerRadius(size.width * 0.1f),
            style = Stroke(width = stroke),
        )
        drawLine(
            color = color,
            start = Offset(size.width * 0.14f, size.height * 0.39f),
            end = Offset(size.width * 0.86f, size.height * 0.39f),
            strokeWidth = stroke,
        )
        for (x in listOf(0.35f, 0.65f)) {
            drawLine(
                color = color,
                start = Offset(size.width * x, size.height * 0.1f),
                end = Offset(size.width * x, size.height * 0.29f),
                strokeWidth = stroke,
                cap = StrokeCap.Round,
            )
        }
        drawCircle(
            color = color,
            radius = size.minDimension * 0.055f,
            center = Offset(size.width * 0.5f, size.height * 0.62f),
        )
    }
}

@Composable
fun TimeGlyph(
    modifier: Modifier = Modifier,
    color: Color = FoyerTextMuted,
) {
    Canvas(modifier = modifier.size(20.dp)) {
        val stroke = 1.6.dp.toPx()
        drawCircle(
            color = color,
            radius = size.minDimension * 0.36f,
            center = Offset(size.width * 0.5f, size.height * 0.5f),
            style = Stroke(width = stroke),
        )
        drawLine(
            color = color,
            start = Offset(size.width * 0.5f, size.height * 0.5f),
            end = Offset(size.width * 0.5f, size.height * 0.29f),
            strokeWidth = stroke,
            cap = StrokeCap.Round,
        )
        drawLine(
            color = color,
            start = Offset(size.width * 0.5f, size.height * 0.5f),
            end = Offset(size.width * 0.68f, size.height * 0.61f),
            strokeWidth = stroke,
            cap = StrokeCap.Round,
        )
    }
}

@Composable
fun MoreGlyph(
    modifier: Modifier = Modifier,
    color: Color = FoyerTextMuted,
) {
    Canvas(modifier = modifier.size(20.dp)) {
        val radius = size.minDimension * 0.075f
        listOf(0.24f, 0.5f, 0.76f).forEach { x ->
            drawCircle(color = color, radius = radius, center = Offset(size.width * x, size.height * 0.5f))
        }
    }
}

@Composable
fun RetryGlyph(
    modifier: Modifier = Modifier,
    color: Color = FoyerTextMuted,
) {
    Canvas(modifier = modifier.size(20.dp)) {
        val stroke = 1.6.dp.toPx()
        drawArc(
            color = color,
            startAngle = -65f,
            sweepAngle = 285f,
            useCenter = false,
            topLeft = Offset(size.width * 0.16f, size.height * 0.16f),
            size = Size(size.width * 0.68f, size.height * 0.68f),
            style = Stroke(width = stroke, cap = StrokeCap.Round),
        )
        val arrow = Path().apply {
            moveTo(size.width * 0.68f, size.height * 0.08f)
            lineTo(size.width * 0.88f, size.height * 0.16f)
            lineTo(size.width * 0.72f, size.height * 0.31f)
            close()
        }
        drawPath(arrow, color = color)
    }
}

@Composable
fun ExpandGlyph(
    expanded: Boolean,
    modifier: Modifier = Modifier,
    color: Color = FoyerTextMuted,
) {
    Canvas(modifier = modifier.size(16.dp)) {
        val stroke = 1.5.dp.toPx()
        if (expanded) {
            drawLine(
                color = color,
                start = Offset(size.width * 0.22f, size.height * 0.38f),
                end = Offset(size.width * 0.5f, size.height * 0.66f),
                strokeWidth = stroke,
                cap = StrokeCap.Round,
            )
            drawLine(
                color = color,
                start = Offset(size.width * 0.5f, size.height * 0.66f),
                end = Offset(size.width * 0.78f, size.height * 0.38f),
                strokeWidth = stroke,
                cap = StrokeCap.Round,
            )
        } else {
            drawLine(
                color = color,
                start = Offset(size.width * 0.38f, size.height * 0.22f),
                end = Offset(size.width * 0.66f, size.height * 0.5f),
                strokeWidth = stroke,
                cap = StrokeCap.Round,
            )
            drawLine(
                color = color,
                start = Offset(size.width * 0.66f, size.height * 0.5f),
                end = Offset(size.width * 0.38f, size.height * 0.78f),
                strokeWidth = stroke,
                cap = StrokeCap.Round,
            )
        }
    }
}
