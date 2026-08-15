package com.amazity.foyer.ui.components

import android.content.ActivityNotFoundException
import android.content.Context
import android.content.Intent
import android.graphics.BitmapFactory
import android.net.Uri
import android.util.Log
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.Image
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.ClickableText
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import com.amazity.foyer.R
import com.amazity.foyer.data.CachedLinkPreview
import com.amazity.foyer.network.LinkPreviewRepository
import com.amazity.foyer.ui.theme.FoyerLine
import com.amazity.foyer.ui.theme.FoyerSurfaceRaised
import com.amazity.foyer.ui.theme.FoyerText
import com.amazity.foyer.ui.theme.FoyerTextDim

data class AssistantLink(val uri: String, val start: Int, val end: Int)

fun assistantLinks(text: String): List<AssistantLink> = LINK_PATTERN.findAll(text).mapNotNull { match ->
    val uri = match.value.trimEnd('.', ',', ';', '!', '?', ')', ']', '}')
    uri.takeIf(String::isNotEmpty)?.let {
        AssistantLink(it, match.range.first, match.range.first + it.length)
    }
}.toList()

@Composable
fun RichAssistantMessage(
    text: String,
    color: Color,
    modifier: Modifier = Modifier,
    style: TextStyle = MaterialTheme.typography.bodyMedium,
) {
    val links = remember(text) { assistantLinks(text) }
    Column(modifier = modifier, verticalArrangement = Arrangement.spacedBy(10.dp)) {
        LinkifiedText(text = text, links = links, color = color, style = style)
        links.asSequence()
            .map(AssistantLink::uri)
            .filter { it.startsWith("https://", ignoreCase = true) }
            .distinct()
            .take(2)
            .forEach { url ->
                LinkPreview(url)
            }
    }
}

@Composable
private fun LinkifiedText(
    text: String,
    links: List<AssistantLink>,
    color: Color,
    style: TextStyle,
) {
    val context = LocalContext.current
    val annotated = remember(text, links, color) {
        buildAnnotatedString {
            var cursor = 0
            links.forEach { link ->
                if (link.start > cursor) append(text.substring(cursor, link.start))
                pushStringAnnotation(LINK_TAG, link.uri)
                withStyle(SpanStyle(color = color, textDecoration = TextDecoration.Underline)) {
                    append(text.substring(link.start, link.end))
                }
                pop()
                cursor = link.end
            }
            if (cursor < text.length) append(text.substring(cursor))
        }
    }
    ClickableText(
        text = annotated,
        style = style.copy(color = color),
        onClick = { offset ->
            annotated.getStringAnnotations(LINK_TAG, offset, offset)
                .firstOrNull()
                ?.item
                ?.let { openUri(context, it) }
        },
    )
}

@Composable
private fun LinkPreview(url: String) {
    val context = LocalContext.current
    val openDescription = stringResource(R.string.link_preview_open)
    val repository = remember(context) { LinkPreviewRepository.get(context) }
    val preview by produceState<CachedLinkPreview?>(null, repository, url) {
        value = repository.load(url)
    }
    preview?.let { metadata ->
        val bitmap = remember(metadata.imageBytes) {
            metadata.imageBytes?.let { BitmapFactory.decodeByteArray(it, 0, it.size) }
        }
        Surface(
            modifier = Modifier
                .fillMaxWidth()
                .clickable { openUri(context, url) }
                .semantics { contentDescription = openDescription },
            shape = RoundedCornerShape(14.dp),
            color = FoyerSurfaceRaised,
            border = BorderStroke(1.dp, FoyerLine),
        ) {
            Column {
                bitmap?.let {
                    Image(
                        bitmap = it.asImageBitmap(),
                        contentDescription = null,
                        modifier = Modifier.fillMaxWidth().height(118.dp),
                        contentScale = ContentScale.Crop,
                    )
                }
                Column(modifier = Modifier.padding(12.dp)) {
                    metadata.title?.let {
                        Text(it, style = MaterialTheme.typography.labelMedium, color = FoyerText)
                    }
                    metadata.description?.let {
                        if (metadata.title != null) Spacer(Modifier.height(4.dp))
                        Text(
                            text = it,
                            style = MaterialTheme.typography.bodySmall,
                            color = FoyerTextDim,
                            maxLines = 3,
                        )
                    }
                }
            }
        }
    }
}

private fun openUri(context: Context, value: String) {
    try {
        context.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(value)))
    } catch (error: ActivityNotFoundException) {
        Log.w(TAG, "No activity can open assistant link scheme")
    }
}

private val LINK_PATTERN = Regex(
    "(?i)(https://[^\\s<]+|geo:[^\\s<]+|tel:[+0-9][0-9+(). #*-]{1,}[0-9#*])",
)
private const val LINK_TAG = "assistant_uri"
private const val TAG = "AssistantLinks"
