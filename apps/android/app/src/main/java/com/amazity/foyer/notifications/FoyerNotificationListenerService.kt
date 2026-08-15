package com.amazity.foyer.notifications

import android.app.Notification
import android.service.notification.NotificationListenerService
import android.service.notification.StatusBarNotification
import com.amazity.foyer.data.FoyerDatabase
import com.amazity.foyer.data.PendingNotification
import com.amazity.foyer.data.UserSettingsStore
import com.amazity.foyer.sync.NotificationOutboxScheduler
import java.nio.charset.StandardCharsets
import java.time.Instant
import java.util.UUID
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.launch

class FoyerNotificationListenerService : NotificationListenerService() {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)

    override fun onNotificationPosted(sbn: StatusBarNotification) {
        val store = UserSettingsStore(this)
        if (!store.notificationContextEnabled()) return
        if (!NotificationCapturePolicy.accepts(sbn.packageName, store.notificationWhitelist(), sbn.notification.flags)) {
            return
        }

        val body = sbn.notification.extras.getCharSequence(Notification.EXTRA_BIG_TEXT)
            ?: sbn.notification.extras.getCharSequence(Notification.EXTRA_TEXT)
            ?: return
        val redactedBody = NotificationSanitizer.redact(body.toString())
        if (redactedBody.text.isBlank()) return
        val rawTitle = sbn.notification.extras.getCharSequence(Notification.EXTRA_TITLE)?.toString()
        val redactedTitle = rawTitle?.let(NotificationSanitizer::redact)
        val item = PendingNotification(
            id = stableId(sbn),
            appPackage = sbn.packageName,
            title = redactedTitle?.text?.takeIf(String::isNotBlank),
            body = redactedBody.text,
            postedAt = Instant.ofEpochMilli(sbn.postTime).toString(),
            redacted = redactedBody.redacted || redactedTitle?.redacted == true,
        )
        scope.launch {
            val dao = FoyerDatabase.get(applicationContext).foyerDao()
            dao.enqueueNotification(item)
            if (dao.pendingNotificationCount() >= 50) {
                NotificationOutboxScheduler.requestNow(applicationContext)
            }
        }
    }

    override fun onDestroy() {
        scope.cancel()
        super.onDestroy()
    }

    private fun stableId(sbn: StatusBarNotification): String {
        val source = "${sbn.packageName}|${sbn.key}|${sbn.postTime}"
        return "android_notification_${UUID.nameUUIDFromBytes(source.toByteArray(StandardCharsets.UTF_8))}"
    }
}
