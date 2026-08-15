package com.amazity.foyer.ui.theme

import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable

private val FoyerColorScheme = darkColorScheme(
    primary = FoyerText,
    onPrimary = FoyerBlack,
    background = FoyerBlack,
    onBackground = FoyerText,
    surface = FoyerSurface,
    onSurface = FoyerText,
    surfaceVariant = FoyerSurfaceRaised,
    onSurfaceVariant = FoyerTextMuted,
    outline = FoyerLine,
)

@Composable
fun FoyerTheme(content: @Composable () -> Unit) {
    MaterialTheme(
        colorScheme = FoyerColorScheme,
        typography = Typography,
        content = content,
    )
}

