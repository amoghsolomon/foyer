package com.amazity.foyer.notifications

import android.app.NotificationManager
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.provider.Settings
import com.amazity.foyer.data.FoyerDatabase
import com.amazity.foyer.data.UserSettingsStore
import com.amazity.foyer.sync.NotificationOutboxScheduler
import com.amazity.foyer.sync.SettingsSyncScheduler
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.launch

class NotificationContextManager(context: Context) {
    private val appContext = context.applicationContext
    private val store = UserSettingsStore(appContext)
    private val component = ComponentName(appContext, FoyerNotificationListenerService::class.java)

    fun enabled(): Boolean = store.notificationContextEnabled()

    fun whitelist(): Set<String> = store.notificationWhitelist()

    fun accessGranted(): Boolean = appContext.getSystemService(NotificationManager::class.java)
        .isNotificationListenerAccessGranted(component)

    fun setEnabled(enabled: Boolean) {
        store.setNotificationContextEnabled(enabled)
        appContext.packageManager.setComponentEnabledSetting(
            component,
            if (enabled) PackageManager.COMPONENT_ENABLED_STATE_ENABLED
            else PackageManager.COMPONENT_ENABLED_STATE_DISABLED,
            PackageManager.DONT_KILL_APP,
        )
        if (enabled) {
            NotificationOutboxScheduler.ensureScheduled(appContext)
        } else {
            NotificationOutboxScheduler.disable(appContext)
            scope.launch {
                FoyerDatabase.get(appContext).foyerDao().clearPendingNotifications()
            }
        }
    }

    fun openAccessSettings() {
        appContext.startActivity(
            Intent(Settings.ACTION_NOTIFICATION_LISTENER_SETTINGS)
                .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK),
        )
    }

    fun initialize(installedPackages: Set<String> = emptySet()): Set<String> {
        val enabled = store.notificationContextEnabled()
        appContext.packageManager.setComponentEnabledSetting(
            component,
            if (enabled) PackageManager.COMPONENT_ENABLED_STATE_ENABLED
            else PackageManager.COMPONENT_ENABLED_STATE_DISABLED,
            PackageManager.DONT_KILL_APP,
        )
        if (enabled) NotificationOutboxScheduler.ensureScheduled(appContext)
        if (!store.whatsappSeedCompleted() && installedPackages.isNotEmpty()) {
            if (WHATSAPP_PACKAGE in installedPackages && store.notificationWhitelist().isEmpty()) {
                store.queueNotificationWhitelist(setOf(WHATSAPP_PACKAGE))
                SettingsSyncScheduler.requestNow(appContext)
            }
            store.markWhatsappSeedCompleted()
        }
        return store.notificationWhitelist()
    }

    companion object {
        const val WHATSAPP_PACKAGE = "com.whatsapp"
        private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    }
}
