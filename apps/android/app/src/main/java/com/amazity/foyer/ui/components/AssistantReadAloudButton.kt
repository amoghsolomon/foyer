package com.amazity.foyer.ui.components

import androidx.compose.material3.IconButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import com.amazity.foyer.R
import com.amazity.foyer.ui.theme.FoyerText
import com.amazity.foyer.ui.theme.FoyerTextMuted

@Composable
fun AssistantReadAloudButton(
    active: Boolean,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val description = stringResource(
        if (active) R.string.assistant_read_aloud_stop else R.string.assistant_read_aloud_start,
    )
    IconButton(
        onClick = onClick,
        modifier = modifier.semantics { contentDescription = description },
    ) {
        SpeakerGlyph(color = if (active) FoyerText else FoyerTextMuted)
    }
}
