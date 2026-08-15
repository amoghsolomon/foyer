package com.amazity.foyer.auth

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent

class FoyerAccountCoordinator(context: Context) {
    private val appContext = context.applicationContext
    private val auth = foyerAuthSession(appContext)

    fun hasPreviousAccess(): Boolean = auth.hasPreviousAccess()

    fun enrollment(): DeviceEnrollmentPresentation = auth.enrollment()

    suspend fun restoreSession(): Boolean = auth.restoreSession()

    suspend fun retryEnrollment(): Boolean {
        return auth.authenticate()
    }

    fun developmentAuthAvailable(): Boolean = auth.developmentAuthAvailable()

    suspend fun useDevelopmentSession() {
        auth.useDevelopmentSession()
    }

    fun copyEnrollment() {
        val clipboard = appContext.getSystemService(ClipboardManager::class.java) ?: return
        clipboard.setPrimaryClip(
            ClipData.newPlainText("Foyer public device enrollment", enrollment().enrollmentJson),
        )
    }

    fun shareEnrollmentIntent(): Intent = Intent(Intent.ACTION_SEND).apply {
        type = "text/plain"
        putExtra(Intent.EXTRA_TEXT, enrollment().enrollmentJson)
    }

    suspend fun signOut() {
        auth.signOut()
    }
}
