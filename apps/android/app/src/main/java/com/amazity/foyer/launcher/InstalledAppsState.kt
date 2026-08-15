package com.amazity.foyer.launcher

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import com.amazity.foyer.model.LauncherApp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

data class InstalledAppsState(
    val apps: List<LauncherApp> = emptyList(),
    val loading: Boolean = true,
    val errorMessage: String? = null,
)

@Composable
fun rememberInstalledApps(
    context: Context,
    repository: LauncherAppsRepository,
): InstalledAppsState {
    var refreshVersion by remember { mutableIntStateOf(0) }
    var state by remember { mutableStateOf(InstalledAppsState()) }

    LaunchedEffect(repository, refreshVersion) {
        state = state.copy(loading = true, errorMessage = null)
        val result = withContext(Dispatchers.IO) {
            runCatching(repository::loadApps)
        }
        state = result.fold(
            onSuccess = { apps -> InstalledAppsState(apps = apps, loading = false) },
            onFailure = { error ->
                InstalledAppsState(
                    apps = state.apps,
                    loading = false,
                    errorMessage = error.message ?: "Unable to load installed apps",
                )
            },
        )
    }

    DisposableEffect(context) {
        val receiver = object : BroadcastReceiver() {
            override fun onReceive(context: Context?, intent: Intent?) {
                refreshVersion += 1
            }
        }
        val filter = IntentFilter().apply {
            addAction(Intent.ACTION_PACKAGE_ADDED)
            addAction(Intent.ACTION_PACKAGE_CHANGED)
            addAction(Intent.ACTION_PACKAGE_REMOVED)
            addAction(Intent.ACTION_PACKAGE_REPLACED)
            addDataScheme("package")
        }
        val registered = runCatching {
            registerPackageReceiver(context, receiver, filter)
        }.isSuccess

        onDispose {
            if (registered) {
                runCatching { context.unregisterReceiver(receiver) }
            }
        }
    }

    return state
}

private fun registerPackageReceiver(
    context: Context,
    receiver: BroadcastReceiver,
    filter: IntentFilter,
) {
    context.registerReceiver(receiver, filter, Context.RECEIVER_EXPORTED)
}
