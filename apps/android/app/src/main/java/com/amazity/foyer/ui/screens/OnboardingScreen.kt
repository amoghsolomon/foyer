package com.amazity.foyer.ui.screens

import android.Manifest
import android.content.pm.PackageManager
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.core.content.ContextCompat
import androidx.annotation.StringRes
import com.amazity.foyer.R
import com.amazity.foyer.data.UserSettingsStore
import com.amazity.foyer.ui.components.FoyerScreen
import com.amazity.foyer.ui.components.HairlineDivider
import com.amazity.foyer.ui.components.NestedScreenHeader
import com.amazity.foyer.ui.components.TimezoneInput
import com.amazity.foyer.ui.theme.FoyerBlack
import com.amazity.foyer.ui.theme.FoyerLine
import com.amazity.foyer.ui.theme.FoyerText
import com.amazity.foyer.ui.theme.FoyerTextDim

private data class SetupStep(
    @StringRes val eyebrow: Int,
    @StringRes val title: Int,
    @StringRes val body: Int,
    val checks: List<Int>,
    val timezone: Boolean = false,
)

private val setupSteps = listOf(
    SetupStep(R.string.onboarding_welcome_eyebrow, R.string.onboarding_welcome_title, R.string.onboarding_welcome_body, listOf(R.string.onboarding_fast_launcher, R.string.onboarding_offline_capture, R.string.onboarding_private_workspace)),
    SetupStep(R.string.onboarding_private_eyebrow, R.string.onboarding_private_title, R.string.onboarding_private_body, listOf(R.string.onboarding_server_agenda, R.string.onboarding_agent_access, R.string.onboarding_notes_vault)),
    SetupStep(R.string.onboarding_timezone_eyebrow, R.string.onboarding_timezone_title, R.string.onboarding_timezone_body, emptyList(), timezone = true),
    SetupStep(R.string.onboarding_launcher_eyebrow, R.string.onboarding_launcher_title, R.string.onboarding_launcher_body, listOf(R.string.onboarding_default_home, R.string.onboarding_microphone, R.string.onboarding_notifications)),
)

@Composable
fun OnboardingScreen(
    initialTimezone: String,
    onSaveTimezone: (String) -> Unit,
    onFinish: () -> Unit,
    onBack: (() -> Unit)? = null,
) {
    val context = LocalContext.current
    var page by rememberSaveable { mutableIntStateOf(0) }
    var timezone by rememberSaveable(initialTimezone) { androidx.compose.runtime.mutableStateOf(initialTimezone) }
    val step = setupSteps[page]
    val notificationPermission = rememberLauncherForActivityResult(
        ActivityResultContracts.RequestPermission(),
    ) { onFinish() }
    val finishSetup = {
        val chosenTimezone = timezone.takeIf(UserSettingsStore::isValidTimezone)
            ?: java.time.ZoneId.systemDefault().id
        onSaveTimezone(chosenTimezone)
        if (
            ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) ==
            PackageManager.PERMISSION_GRANTED
        ) {
            onFinish()
        } else {
            notificationPermission.launch(Manifest.permission.POST_NOTIFICATIONS)
        }
    }

    FoyerScreen {
        Column(modifier = Modifier.fillMaxSize().padding(horizontal = 28.dp)) {
            if (onBack != null) {
                NestedScreenHeader(title = "Setup", onBack = onBack)
                HairlineDivider()
                Spacer(Modifier.height(22.dp))
            } else {
                Spacer(Modifier.height(40.dp))
            }
            Text(stringResource(step.eyebrow), style = MaterialTheme.typography.labelSmall, color = FoyerTextDim)
            Spacer(Modifier.height(12.dp))
            Text(stringResource(step.title), style = MaterialTheme.typography.displaySmall, color = FoyerText)
            Spacer(Modifier.height(14.dp))
            Text(stringResource(step.body), style = MaterialTheme.typography.bodyLarge, color = FoyerTextDim)
            Spacer(Modifier.height(34.dp))
            if (step.timezone) {
                TimezoneInput(
                    value = timezone,
                    onValueChange = { timezone = it },
                    modifier = Modifier.fillMaxWidth(),
                )
            }
            step.checks.forEachIndexed { index, check ->
                Row(modifier = Modifier.fillMaxWidth().padding(vertical = 13.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(14.dp)) {
                    Text("0${index + 1}", style = MaterialTheme.typography.labelSmall, color = FoyerTextDim)
                    Text(stringResource(check), style = MaterialTheme.typography.bodyLarge, color = FoyerText)
                }
                HairlineDivider()
            }
            Spacer(Modifier.weight(1f))
            Text("${page + 1} / ${setupSteps.size}", style = MaterialTheme.typography.bodySmall, color = FoyerTextDim)
            Spacer(Modifier.height(12.dp))
            Surface(
                modifier = Modifier.fillMaxWidth().height(52.dp).clickable(
                    enabled = !step.timezone || UserSettingsStore.isValidTimezone(timezone),
                ) {
                    if (page == setupSteps.lastIndex) finishSetup() else page += 1
                },
                shape = RoundedCornerShape(26.dp),
                color = FoyerText,
                contentColor = FoyerBlack,
                border = BorderStroke(1.dp, FoyerLine),
            ) { Box(contentAlignment = Alignment.Center) { Text(stringResource(if (page == setupSteps.lastIndex) R.string.onboarding_start else R.string.onboarding_continue), style = MaterialTheme.typography.labelMedium) } }
            if (page < setupSteps.lastIndex) {
                Text(stringResource(R.string.onboarding_skip), style = MaterialTheme.typography.labelMedium, color = FoyerTextDim, modifier = Modifier.align(Alignment.CenterHorizontally).clickable(onClick = finishSetup).padding(16.dp))
            } else {
                Spacer(Modifier.height(48.dp))
            }
        }
    }
}
