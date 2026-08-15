package com.amazity.foyer.ui.screens

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.clickable
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.Switch
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.Alignment
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.unit.dp
import android.widget.Toast
import com.amazity.foyer.R
import com.amazity.foyer.auth.FoyerAccountCoordinator
import com.amazity.foyer.data.UserSettingsStore
import com.amazity.foyer.ui.components.FoyerScreen
import com.amazity.foyer.ui.components.HairlineDivider
import com.amazity.foyer.ui.components.NestedScreenHeader
import com.amazity.foyer.ui.components.SectionLabel
import com.amazity.foyer.ui.components.TimezoneInput
import com.amazity.foyer.ui.theme.FoyerText
import com.amazity.foyer.ui.theme.FoyerTextDim

@Composable
fun SettingsScreen(
    onBack: () -> Unit,
    serverUrl: String,
    signedIn: Boolean,
    grokConnectionStatus: String?,
    grokConnectionEnabled: Boolean,
    assistantConfigured: Boolean,
    timezone: String,
    onTimezoneChange: (String) -> Unit,
    onConnectGrok: () -> Unit,
    onSignOut: () -> Unit,
    onOpenSyncStatus: () -> Unit,
    onOpenMemoryProfile: () -> Unit,
    notificationContextEnabled: Boolean,
    notificationAccessGranted: Boolean,
    notificationWhitelistCount: Int,
    onNotificationContextToggle: (Boolean) -> Unit,
    onOpenNotificationWhitelist: () -> Unit,
    onConfigureAssistant: () -> Unit,
    onOpenOnboarding: () -> Unit,
) {
    var showTimezoneDialog by rememberSaveable { mutableStateOf(false) }
    var timezoneDraft by rememberSaveable(timezone) { mutableStateOf(timezone) }
    val context = LocalContext.current
    val enrollment = rememberSaveable { FoyerAccountCoordinator(context).enrollment().fingerprint }
    FoyerScreen {
        Column(
            modifier = Modifier
                .fillMaxSize()
                .verticalScroll(rememberScrollState())
                .padding(horizontal = 24.dp),
        ) {
            NestedScreenHeader(title = "Foyer settings", onBack = onBack)
            HairlineDivider()
            Spacer(Modifier.height(24.dp))
            SectionLabel("Connections")
            SettingsRow("Agent server", serverUrl)
            HairlineDivider()
            SettingsRow(
                title = stringResource(R.string.settings_timezone_title),
                subtitle = timezone,
                enabled = true,
                onClick = {
                    timezoneDraft = timezone
                    showTimezoneDialog = true
                },
            )
            HairlineDivider()
            SettingsRow(
                title = "Foyer account",
                subtitle = if (signedIn) {
                    "Device $enrollment · tap to sign out"
                } else {
                    "This device must be enrolled by the operator"
                },
                enabled = signedIn,
                onClick = onSignOut,
            )
            HairlineDivider()
            SettingsRow(
                title = "Device enrollment",
                subtitle = "Copy the public key the operator must add",
                enabled = true,
                onClick = {
                    FoyerAccountCoordinator(context).copyEnrollment()
                    Toast.makeText(context, "Copied public enrollment", Toast.LENGTH_SHORT).show()
                },
            )
            HairlineDivider()
            SettingsRow(
                title = "SuperGrok model access",
                subtitle = grokConnectionStatus ?: when {
                    !signedIn -> "Sign in before connecting the shared subscription"
                    else -> "Tap to authorize with a device code"
                },
                enabled = signedIn && grokConnectionEnabled,
                onClick = onConnectGrok,
            )
            HairlineDivider()
            SettingsRow("Vault", "Server-authoritative private workspace")
            HairlineDivider()
            SettingsRow(
                title = "Sync status",
                subtitle = "2 sources need attention",
                enabled = true,
                onClick = onOpenSyncStatus,
            )

            Spacer(Modifier.height(28.dp))
            SectionLabel("Device")
            SettingsRow(
                title = "Voice and assistant",
                subtitle = if (assistantConfigured) {
                    "Foyer is the default assistant"
                } else {
                    "Tap to make Foyer the default assistant"
                },
                enabled = true,
                onClick = onConfigureAssistant,
            )
            HairlineDivider()
            SettingsRow("Launcher", "System managed")
            HairlineDivider()
            SettingsRow(
                title = "Run setup again",
                subtitle = "Review launcher, microphone, and connection setup",
                enabled = true,
                onClick = onOpenOnboarding,
            )

            Spacer(Modifier.height(28.dp))
            SectionLabel(stringResource(R.string.notification_context_heading))
            NotificationContextToggleRow(
                enabled = notificationContextEnabled,
                accessGranted = notificationAccessGranted,
                onToggle = onNotificationContextToggle,
            )
            if (notificationContextEnabled) {
                HairlineDivider()
                SettingsRow(
                    title = stringResource(R.string.notification_context_manage_title),
                    subtitle = pluralStringResource(
                        R.plurals.notification_context_manage_subtitle,
                        notificationWhitelistCount,
                        notificationWhitelistCount,
                    ),
                    enabled = true,
                    onClick = onOpenNotificationWhitelist,
                )
            }

            Spacer(Modifier.height(28.dp))
            SectionLabel("Privacy")
            SettingsRow(
                title = stringResource(R.string.memory_settings_title),
                subtitle = stringResource(R.string.memory_settings_subtitle),
                enabled = true,
                onClick = onOpenMemoryProfile,
            )
            HairlineDivider()
            SettingsRow("Local data", "Review stored captures and cache")
            Spacer(Modifier.height(24.dp))
        }
    }

    if (showTimezoneDialog) {
        AlertDialog(
            onDismissRequest = { showTimezoneDialog = false },
            title = { Text(stringResource(R.string.settings_timezone_dialog_title)) },
            text = {
                TimezoneInput(
                    value = timezoneDraft,
                    onValueChange = { timezoneDraft = it },
                )
            },
            confirmButton = {
                TextButton(
                    enabled = UserSettingsStore.isValidTimezone(timezoneDraft),
                    onClick = {
                        onTimezoneChange(timezoneDraft)
                        showTimezoneDialog = false
                    },
                ) { Text(stringResource(R.string.settings_timezone_save)) }
            },
            dismissButton = {
                TextButton(onClick = { showTimezoneDialog = false }) {
                    Text(stringResource(R.string.settings_timezone_cancel))
                }
            },
        )
    }
}

@Composable
private fun NotificationContextToggleRow(
    enabled: Boolean,
    accessGranted: Boolean,
    onToggle: (Boolean) -> Unit,
) {
    Row(
        modifier = Modifier.fillMaxWidth().padding(vertical = 13.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(modifier = Modifier.weight(1f)) {
            Text(
                text = stringResource(R.string.notification_context_master_title),
                style = MaterialTheme.typography.bodySmall,
                color = FoyerText,
            )
            Text(
                text = stringResource(
                    when {
                        !enabled -> R.string.notification_context_off
                        accessGranted -> R.string.notification_context_on
                        else -> R.string.notification_context_needs_access
                    },
                ),
                style = MaterialTheme.typography.bodySmall,
                color = FoyerTextDim,
                modifier = Modifier.padding(top = 2.dp),
            )
        }
        Switch(checked = enabled, onCheckedChange = onToggle)
    }
}

@Composable
private fun SettingsRow(
    title: String,
    subtitle: String,
    enabled: Boolean = false,
    onClick: () -> Unit = {},
) {
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .then(if (enabled) Modifier.clickable(onClick = onClick) else Modifier)
            .padding(vertical = 13.dp),
    ) {
        Text(
            text = title,
            style = MaterialTheme.typography.bodySmall,
            color = FoyerText,
        )
        Text(
            text = subtitle,
            style = MaterialTheme.typography.bodySmall,
            color = FoyerTextDim,
            modifier = Modifier.padding(top = 2.dp),
        )
    }
}
