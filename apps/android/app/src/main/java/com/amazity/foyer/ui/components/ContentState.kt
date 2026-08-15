package com.amazity.foyer.ui.components

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import com.amazity.foyer.ui.theme.FoyerBlack
import com.amazity.foyer.ui.theme.FoyerLine
import com.amazity.foyer.ui.theme.FoyerText
import com.amazity.foyer.ui.theme.FoyerTextDim

@Composable
fun ContentStatePanel(
    title: String,
    message: String,
    actionLabel: String? = null,
    onAction: (() -> Unit)? = null,
    modifier: Modifier = Modifier,
) {
    Surface(
        modifier = modifier.fillMaxWidth(),
        shape = RoundedCornerShape(16.dp),
        color = FoyerBlack,
        border = BorderStroke(1.dp, FoyerLine),
    ) {
        Column(
            modifier = Modifier.padding(horizontal = 18.dp, vertical = 22.dp),
            verticalArrangement = Arrangement.spacedBy(7.dp),
            horizontalAlignment = Alignment.Start,
        ) {
            Text(title, style = MaterialTheme.typography.titleMedium, color = FoyerText)
            Text(message, style = MaterialTheme.typography.bodyMedium, color = FoyerTextDim)
            if (actionLabel != null && onAction != null) {
                Text(
                    text = actionLabel,
                    style = MaterialTheme.typography.labelMedium,
                    color = FoyerText,
                    modifier = Modifier
                        .clickable(onClick = onAction)
                        .padding(top = 5.dp, bottom = 2.dp),
                )
            }
        }
    }
}

@Composable
fun LoadingStatePanel(label: String, modifier: Modifier = Modifier) {
    ContentStatePanel(
        title = "Loading",
        message = label,
        modifier = modifier,
    )
}

@Composable
fun ErrorStatePanel(
    message: String,
    onRetry: () -> Unit,
    modifier: Modifier = Modifier,
) {
    ContentStatePanel(
        title = "Couldn’t load this",
        message = message,
        actionLabel = "Retry",
        onAction = onRetry,
        modifier = modifier,
    )
}
