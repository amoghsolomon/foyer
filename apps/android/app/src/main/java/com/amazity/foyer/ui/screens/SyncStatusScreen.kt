package com.amazity.foyer.ui.screens

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import com.amazity.foyer.data.SyncStatusSnapshot
import com.amazity.foyer.ui.components.FoyerScreen
import com.amazity.foyer.ui.components.HairlineDivider
import com.amazity.foyer.ui.components.NestedScreenHeader
import com.amazity.foyer.ui.components.SectionLabel
import com.amazity.foyer.ui.theme.FoyerBlack
import com.amazity.foyer.ui.theme.FoyerLine
import com.amazity.foyer.ui.theme.FoyerText
import com.amazity.foyer.ui.theme.FoyerTextDim
import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter

@Composable
fun SyncStatusScreen(
    status: SyncStatusSnapshot,
    onSyncNow: () -> Unit,
    onBack: () -> Unit,
) {
    val healthy = status.lastError == null
    val headline = when {
        !healthy -> "The last sync needs attention"
        status.pendingMutations > 0 -> "${status.pendingMutations} local changes waiting"
        else -> "Everything is up to date"
    }
    val sourceDetail = when {
        !healthy -> status.lastError.orEmpty()
        status.pendingMutations > 0 -> "Waiting to upload ${status.pendingMutations} local changes"
        status.lastSuccessfulAt != null -> "Device cache matches the Foyer server"
        else -> "No successful sync recorded yet"
    }

    FoyerScreen {
        Column(
            modifier = Modifier.fillMaxSize().verticalScroll(rememberScrollState())
                .padding(horizontal = 24.dp),
        ) {
            NestedScreenHeader(title = "Sync status", onBack = onBack)
            HairlineDivider()
            Spacer(Modifier.height(18.dp))
            Text(headline, style = MaterialTheme.typography.bodyMedium, color = FoyerTextDim)
            Spacer(Modifier.height(28.dp))
            SectionLabel("Source")
            Spacer(Modifier.height(6.dp))
            Row(
                modifier = Modifier.fillMaxWidth().padding(vertical = 14.dp),
                verticalAlignment = Alignment.Top,
                horizontalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                Text(if (healthy && status.pendingMutations == 0) "✓" else "○", color = FoyerText)
                Column(Modifier.weight(1f)) {
                    Text("Foyer server", style = MaterialTheme.typography.bodyLarge, color = FoyerText)
                    Text(sourceDetail, style = MaterialTheme.typography.bodySmall, color = FoyerTextDim)
                }
            }
            HairlineDivider()
            Spacer(Modifier.height(28.dp))
            SectionLabel("Storage")
            Spacer(Modifier.height(8.dp))
            Text(
                status.lastSuccessfulAt?.let { "Last successful sync · ${formatSyncTime(it)}" }
                    ?: "Last successful sync · not yet",
                style = MaterialTheme.typography.bodyMedium,
                color = FoyerTextDim,
            )
            Spacer(Modifier.height(18.dp))
            Surface(
                modifier = Modifier.fillMaxWidth().height(48.dp).clickable(onClick = onSyncNow),
                shape = RoundedCornerShape(24.dp),
                color = FoyerBlack,
                border = BorderStroke(1.dp, FoyerLine),
            ) {
                Column(
                    horizontalAlignment = Alignment.CenterHorizontally,
                    verticalArrangement = Arrangement.Center,
                ) {
                    Text("Sync now", style = MaterialTheme.typography.labelMedium, color = FoyerText)
                }
            }
            Spacer(Modifier.height(24.dp))
        }
    }
}

private fun formatSyncTime(value: String): String = runCatching {
    DateTimeFormatter.ofPattern("d MMM, h:mm a")
        .format(Instant.parse(value).atZone(ZoneId.systemDefault()))
}.getOrDefault(value)
