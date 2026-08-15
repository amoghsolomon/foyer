package com.amazity.foyer.launcher

import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import com.amazity.foyer.model.LauncherApp
import java.text.Collator

class LauncherAppsRepository(context: Context) {
    private val appContext = context.applicationContext
    private val packageManager = appContext.packageManager

    fun loadApps(): List<LauncherApp> {
        val intent = Intent(Intent.ACTION_MAIN).addCategory(Intent.CATEGORY_LAUNCHER)
        val collator = Collator.getInstance().apply {
            strength = Collator.PRIMARY
        }

        return queryLaunchableActivities(intent)
            .mapNotNull { resolveInfo ->
                val activityInfo = resolveInfo.activityInfo ?: return@mapNotNull null
                if (!activityInfo.enabled || !activityInfo.applicationInfo.enabled || !activityInfo.exported) {
                    return@mapNotNull null
                }

                val packageName = activityInfo.packageName
                val activityName = activityInfo.name
                val fallbackName = packageName.substringAfterLast('.')
                val label = resolveInfo.loadLabel(packageManager)
                    ?.toString()
                    ?.trim()
                    .orEmpty()
                    .ifBlank { fallbackName }

                LauncherApp(
                    name = label,
                    packageName = packageName,
                    activityName = activityName,
                    emphasized = packageName == appContext.packageName,
                )
            }
            .distinctBy(LauncherApp::stableKey)
            .sortedWith { left, right ->
                collator.compare(left.name, right.name).takeIf { it != 0 }
                    ?: left.stableKey.compareTo(right.stableKey)
            }
    }

    fun launch(app: LauncherApp): Result<Unit> = runCatching {
        require(app.packageName.isNotBlank() && app.activityName.isNotBlank()) {
            "Missing launch component for ${app.name}"
        }

        val intent = Intent(Intent.ACTION_MAIN)
            .addCategory(Intent.CATEGORY_LAUNCHER)
            .setComponent(ComponentName(app.packageName, app.activityName))
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_RESET_TASK_IF_NEEDED)
        appContext.startActivity(intent)
    }

    private fun queryLaunchableActivities(intent: Intent) =
        packageManager.queryIntentActivities(
            intent,
            PackageManager.ResolveInfoFlags.of(0),
        )
}
