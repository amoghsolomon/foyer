package com.amazity.foyer.notifications

import android.Manifest
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import com.amazity.foyer.MainActivity
import com.amazity.foyer.R

object FoyerNotifications {
    fun ensureChannels(context: Context) {
        val manager = context.getSystemService(NotificationManager::class.java)
        manager.createNotificationChannel(
            NotificationChannel(
                context.getString(R.string.updates_channel_id),
                context.getString(R.string.updates_channel_name),
                NotificationManager.IMPORTANCE_DEFAULT,
            ),
        )
        manager.createNotificationChannel(
            NotificationChannel(
                context.getString(R.string.heartbeat_channel_id),
                context.getString(R.string.heartbeat_channel_name),
                NotificationManager.IMPORTANCE_DEFAULT,
            ),
        )
    }

    fun show(
        context: Context,
        title: String,
        body: String,
        messageId: String?,
        targetType: String? = null,
        targetId: String? = null,
        heartbeat: Boolean = false,
    ) {
        if (context.checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) !=
            PackageManager.PERMISSION_GRANTED
        ) {
            return
        }

        ensureChannels(context)
        val channelId = context.getString(
            if (heartbeat) R.string.heartbeat_channel_id else R.string.updates_channel_id,
        )
        val requestCode = listOf(messageId, targetType, targetId).joinToString(":").hashCode()
        val openApp = PendingIntent.getActivity(
            context,
            requestCode,
            Intent(context, MainActivity::class.java)
                .addFlags(Intent.FLAG_ACTIVITY_CLEAR_TOP)
                .setAction(ACTION_OPEN_TARGET)
                .putExtra("targetType", targetType)
                .putExtra("targetId", targetId),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
        val notification = Notification.Builder(context, channelId)
            .setSmallIcon(R.drawable.ic_notification)
            .setContentTitle(title)
            .setContentText(body)
            .setStyle(Notification.BigTextStyle().bigText(body))
            .setContentIntent(openApp)
            .setAutoCancel(true)
            .build()

        context.getSystemService(NotificationManager::class.java)
            .notify(messageId?.hashCode() ?: body.hashCode(), notification)
    }

    const val ACTION_OPEN_TARGET = "com.amazity.foyer.action.OPEN_TARGET"
}
