package com.amazity.foyer.ui.screens

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.Checkbox
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import com.amazity.foyer.R
import com.amazity.foyer.model.LauncherApp
import com.amazity.foyer.ui.components.FoyerScreen
import com.amazity.foyer.ui.components.HairlineDivider
import com.amazity.foyer.ui.components.NestedScreenHeader
import com.amazity.foyer.ui.theme.FoyerText
import com.amazity.foyer.ui.theme.FoyerTextDim

@Composable
fun NotificationContextScreen(
    apps: List<LauncherApp>,
    whitelist: Set<String>,
    onWhitelistChange: (Set<String>) -> Unit,
    onBack: () -> Unit,
) {
    val packages = remember(apps) {
        apps.filter { it.packageName.isNotBlank() }
            .distinctBy(LauncherApp::packageName)
            .sortedBy { it.name.lowercase() }
    }
    FoyerScreen {
        Column(modifier = Modifier.fillMaxSize().padding(horizontal = 24.dp)) {
            NestedScreenHeader(
                title = stringResource(R.string.notification_context_apps_title),
                onBack = onBack,
            )
            HairlineDivider()
            Text(
                text = stringResource(R.string.notification_context_apps_body),
                style = MaterialTheme.typography.bodyMedium,
                color = FoyerTextDim,
                modifier = Modifier.padding(vertical = 18.dp),
            )
            LazyColumn(modifier = Modifier.fillMaxSize()) {
                items(packages, key = LauncherApp::packageName) { app ->
                    val selected = app.packageName in whitelist
                    Row(
                        modifier = Modifier
                            .fillMaxWidth()
                            .clickable {
                                onWhitelistChange(
                                    if (selected) whitelist - app.packageName
                                    else whitelist + app.packageName,
                                )
                            }
                            .padding(vertical = 10.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Column(modifier = Modifier.weight(1f)) {
                            Text(app.name, style = MaterialTheme.typography.bodyMedium, color = FoyerText)
                            Spacer(Modifier.height(2.dp))
                            Text(
                                app.packageName,
                                style = MaterialTheme.typography.bodySmall,
                                color = FoyerTextDim,
                            )
                        }
                        Checkbox(
                            checked = selected,
                            onCheckedChange = { checked ->
                                onWhitelistChange(
                                    if (checked) whitelist + app.packageName
                                    else whitelist - app.packageName,
                                )
                            },
                        )
                    }
                    HairlineDivider()
                }
            }
        }
    }
}
