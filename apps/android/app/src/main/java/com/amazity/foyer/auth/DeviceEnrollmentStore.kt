package com.amazity.foyer.auth

import android.content.Context
import java.io.File
import java.time.Instant

/**
 * Operator-readable public enrollment state. This store never holds the device
 * private key or a short-lived access token.
 */
class DeviceEnrollmentStore(context: Context) : AuthSessionPersistence {
    private val appContext = context.applicationContext
    private val preferences = appContext.getSharedPreferences(PREFERENCES_NAME, Context.MODE_PRIVATE)
    private val enrollmentFile = File(appContext.filesDir, ENROLLMENT_FILE)

    override fun lastAuthenticatedAt(): Instant? =
        preferences.getString(LAST_AUTHENTICATED_AT, null)?.let { runCatching { Instant.parse(it) }.getOrNull() }

    override fun markAuthenticated(at: Instant) {
        preferences.edit().putString(LAST_AUTHENTICATED_AT, at.toString()).apply()
    }

    override fun usedDevelopmentAuth(): Boolean = preferences.getBoolean(USED_DEVELOPMENT_AUTH, false)

    override fun markDevelopmentAuth(used: Boolean) {
        preferences.edit().putBoolean(USED_DEVELOPMENT_AUTH, used).apply()
    }

    override fun clearSessionState() {
        preferences.edit()
            .remove(LAST_AUTHENTICATED_AT)
            .putBoolean(USED_DEVELOPMENT_AUTH, false)
            .apply()
    }

    override fun writePublicMaterial(material: DevicePublicMaterial) {
        val payload = material.enrollmentJson()
        val tmp = File(appContext.filesDir, "$ENROLLMENT_FILE.tmp")
        tmp.writeText(payload)
        if (!tmp.renameTo(enrollmentFile)) {
            enrollmentFile.writeText(payload)
            tmp.delete()
        }
    }

    fun enrollmentFile(): File = enrollmentFile

    companion object {
        const val ENROLLMENT_FILE = "foyer-device-enrollment.json"
        private const val PREFERENCES_NAME = "foyer_device_auth"
        private const val LAST_AUTHENTICATED_AT = "last_authenticated_at"
        private const val USED_DEVELOPMENT_AUTH = "used_development_auth"
    }
}
