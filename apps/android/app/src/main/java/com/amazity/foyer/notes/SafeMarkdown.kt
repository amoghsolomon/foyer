package com.amazity.foyer.notes

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import com.amazity.foyer.ui.theme.FoyerText
import com.amazity.foyer.ui.theme.FoyerTextMuted

/**
 * Renders Markdown as Compose text. HTML tags are shown as literal characters and never executed.
 */
@Composable
fun SafeMarkdown(
    source: String,
    modifier: Modifier = Modifier,
) {
    Column(modifier = modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(10.dp)) {
        markdownBlocks(source).forEach { block ->
            when (block) {
                is MarkdownBlock.Heading -> Text(
                    text = inlineMarkdown(block.text),
                    style = when (block.level) {
                        1 -> MaterialTheme.typography.headlineMedium
                        2 -> MaterialTheme.typography.titleLarge
                        else -> MaterialTheme.typography.titleMedium
                    },
                    color = FoyerText,
                )
                is MarkdownBlock.ListItem -> Text(
                    text = buildAnnotatedString {
                        append("•  ")
                        append(inlineMarkdown(block.text))
                    },
                    style = MaterialTheme.typography.bodyLarge,
                    color = FoyerTextMuted,
                    modifier = Modifier.padding(start = 8.dp),
                )
                is MarkdownBlock.Code -> Text(
                    text = block.text,
                    style = MaterialTheme.typography.bodyMedium.copy(fontFamily = FontFamily.Monospace),
                    color = FoyerText,
                )
                is MarkdownBlock.Paragraph -> Text(
                    text = inlineMarkdown(block.text),
                    style = MaterialTheme.typography.bodyLarge,
                    color = FoyerTextMuted,
                )
            }
        }
    }
}

internal sealed class MarkdownBlock {
    data class Heading(val level: Int, val text: String) : MarkdownBlock()
    data class ListItem(val text: String) : MarkdownBlock()
    data class Code(val text: String) : MarkdownBlock()
    data class Paragraph(val text: String) : MarkdownBlock()
}

internal fun markdownBlocks(source: String): List<MarkdownBlock> {
    val blocks = mutableListOf<MarkdownBlock>()
    val paragraph = StringBuilder()
    val code = StringBuilder()
    var inFence = false
    fun flushParagraph() {
        val text = paragraph.toString().trim()
        if (text.isNotEmpty()) blocks += MarkdownBlock.Paragraph(text)
        paragraph.clear()
    }
    source.replace("\r\n", "\n").lines().forEach { line ->
        if (line.trimStart().startsWith("```")) {
            if (inFence) {
                blocks += MarkdownBlock.Code(code.toString().trimEnd())
                code.clear()
                inFence = false
            } else {
                flushParagraph()
                inFence = true
            }
            return@forEach
        }
        if (inFence) {
            if (code.isNotEmpty()) code.append('\n')
            code.append(line)
            return@forEach
        }
        val heading = Regex("^(#{1,6})\\s+(.*)$").matchEntire(line)
        val list = Regex("^\\s*[-*]\\s+(.*)$").matchEntire(line)
        when {
            heading != null -> {
                flushParagraph()
                blocks += MarkdownBlock.Heading(heading.groupValues[1].length, heading.groupValues[2])
            }
            list != null -> {
                flushParagraph()
                blocks += MarkdownBlock.ListItem(list.groupValues[1])
            }
            line.isBlank() -> flushParagraph()
            else -> {
                if (paragraph.isNotEmpty()) paragraph.append('\n')
                paragraph.append(line)
            }
        }
    }
    if (inFence) blocks += MarkdownBlock.Code(code.toString().trimEnd())
    flushParagraph()
    return blocks.ifEmpty { listOf(MarkdownBlock.Paragraph("")) }
}

internal fun inlineMarkdown(source: String) = buildAnnotatedString {
    val pattern = Regex("(`[^`]+`|\\*\\*[^*]+\\*\\*|\\*[^*]+\\*|\\[[^]]+]\\([^)]+\\))")
    var cursor = 0
    pattern.findAll(source).forEach { match ->
        append(source.substring(cursor, match.range.first))
        val token = match.value
        when {
            token.startsWith("`") -> withStyle(SpanStyle(fontFamily = FontFamily.Monospace)) {
                append(token.removeSurrounding("`"))
            }
            token.startsWith("**") -> withStyle(SpanStyle(fontWeight = FontWeight.SemiBold, color = FoyerText)) {
                append(token.removeSurrounding("**"))
            }
            token.startsWith("*") -> withStyle(SpanStyle(fontStyle = FontStyle.Italic)) {
                append(token.removeSurrounding("*"))
            }
            else -> {
                val link = Regex("\\[([^]]+)]\\(([^)]+)\\)").matchEntire(token)
                withStyle(SpanStyle(color = FoyerText, textDecoration = TextDecoration.Underline)) {
                    append(link?.groupValues?.get(1) ?: token)
                }
            }
        }
        cursor = match.range.last + 1
    }
    append(source.substring(cursor))
}

internal fun stripHtmlToLiteral(source: String): String = source
