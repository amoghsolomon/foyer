package com.amazity.foyer.data

import android.content.Context
import java.time.ZoneId

class UserSettingsStore(context: Context) {
    private val preferences = context.applicationContext.getSharedPreferences(
        PREFERENCES_NAME,
        Context.MODE_PRIVATE,
    )

    fun timezone(): String = preferences.getString(TIMEZONE, null)
        ?.takeIf(::isValidTimezone)
        ?: ZoneId.systemDefault().id

    fun pendingTimezone(): String? = preferences.getString(PENDING_TIMEZONE, null)
        ?.takeIf(::isValidTimezone)

    fun queueTimezone(timezone: String) {
        require(isValidTimezone(timezone)) { "Invalid IANA timezone" }
        preferences.edit()
            .putString(TIMEZONE, timezone)
            .putString(PENDING_TIMEZONE, timezone)
            .apply()
    }

    fun cacheTimezone(timezone: String) {
        if (isValidTimezone(timezone)) preferences.edit().putString(TIMEZONE, timezone).apply()
    }

    fun markTimezoneSynced(timezone: String) {
        if (pendingTimezone() == timezone) preferences.edit().remove(PENDING_TIMEZONE).apply()
    }

    fun notificationContextEnabled(): Boolean = preferences.getBoolean(
        NOTIFICATION_CONTEXT_ENABLED,
        false,
    )

    fun setNotificationContextEnabled(enabled: Boolean) {
        preferences.edit().putBoolean(NOTIFICATION_CONTEXT_ENABLED, enabled).apply()
    }

    fun notificationWhitelist(): Set<String> = preferences.getStringSet(
        NOTIFICATION_WHITELIST,
        emptySet(),
    )?.toSet().orEmpty()

    fun whatsappSeedCompleted(): Boolean = preferences.getBoolean(WHATSAPP_SEED_COMPLETED, false)

    fun markWhatsappSeedCompleted() {
        preferences.edit().putBoolean(WHATSAPP_SEED_COMPLETED, true).apply()
    }

    fun pendingNotificationWhitelist(): Set<String>? = if (
        preferences.getBoolean(PENDING_NOTIFICATION_WHITELIST, false)
    ) {
        notificationWhitelist()
    } else {
        null
    }

    fun queueNotificationWhitelist(packages: Set<String>) {
        preferences.edit()
            .putStringSet(NOTIFICATION_WHITELIST, packages.toSet())
            .putBoolean(PENDING_NOTIFICATION_WHITELIST, true)
            .apply()
    }

    fun cacheNotificationWhitelist(packages: Set<String>) {
        if (pendingNotificationWhitelist() == null) {
            preferences.edit().putStringSet(NOTIFICATION_WHITELIST, packages.toSet()).apply()
        }
    }

    fun markNotificationWhitelistSynced(packages: Set<String>) {
        if (pendingNotificationWhitelist() == packages) {
            preferences.edit().remove(PENDING_NOTIFICATION_WHITELIST).apply()
        }
    }

    companion object {
        private const val PREFERENCES_NAME = "foyer_user_settings"
        private const val TIMEZONE = "timezone"
        private const val PENDING_TIMEZONE = "pending_timezone"
        private const val NOTIFICATION_CONTEXT_ENABLED = "notification_context_enabled"
        private const val NOTIFICATION_WHITELIST = "notification_whitelist"
        private const val PENDING_NOTIFICATION_WHITELIST = "pending_notification_whitelist"
        private const val WHATSAPP_SEED_COMPLETED = "whatsapp_seed_completed"

        fun isValidTimezone(value: String): Boolean = runCatching { ZoneId.of(value) }.isSuccess
    }
}
