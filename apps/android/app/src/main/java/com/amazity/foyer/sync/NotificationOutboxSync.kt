package com.amazity.foyer.sync

import android.content.Context
import androidx.work.BackoffPolicy
import androidx.work.Constraints
import androidx.work.CoroutineWorker
import androidx.work.ExistingPeriodicWorkPolicy
import androidx.work.ExistingWorkPolicy
import androidx.work.NetworkType
import androidx.work.OneTimeWorkRequestBuilder
import androidx.work.PeriodicWorkRequestBuilder
import androidx.work.WorkManager
import androidx.work.WorkerParameters
import com.amazity.foyer.data.FoyerDatabase
import com.amazity.foyer.auth.foyerApiClient
import com.amazity.foyer.auth.foyerAuthSession
import com.amazity.foyer.data.UserSettingsStore
import java.util.concurrent.TimeUnit

class NotificationOutboxWorker(
    appContext: Context,
    params: WorkerParameters,
) : CoroutineWorker(appContext, params) {
    override suspend fun doWork(): Result {
        val settings = UserSettingsStore(applicationContext)
        if (!settings.notificationContextEnabled()) return Result.success()
        val dao = FoyerDatabase.get(applicationContext).foyerDao()
        val queued = dao.pendingNotifications(50)
        val whitelist = settings.notificationWhitelist()
        val disallowed = queued.filterNot { it.appPackage in whitelist }
        if (disallowed.isNotEmpty()) dao.deleteNotifications(disallowed.map { it.id })
        val batch = queued.filter { it.appPackage in whitelist }
        if (batch.isEmpty()) return Result.success()
        if (!foyerAuthSession(applicationContext).hasPreviousAccess()) return Result.retry()
        return runCatching {
            foyerApiClient(applicationContext).sendNotifications(batch)
            dao.deleteNotifications(batch.map { it.id })
        }.fold(
            onSuccess = {
                if (dao.pendingNotificationCount() > 0) {
                    NotificationOutboxScheduler.requestNow(applicationContext)
                }
                Result.success()
            },
            onFailure = {
                dao.incrementNotificationAttempts(batch.map { it.id })
                if (runAttemptCount >= 5) Result.failure() else Result.retry()
            },
        )
    }
}

object NotificationOutboxScheduler {
    private const val PERIODIC_WORK = "foyer-notification-outbox-periodic"
    private const val IMMEDIATE_WORK = "foyer-notification-outbox-immediate"

    fun ensureScheduled(context: Context) {
        val request = PeriodicWorkRequestBuilder<NotificationOutboxWorker>(15, TimeUnit.MINUTES)
            .setConstraints(networkConstraints())
            .setBackoffCriteria(BackoffPolicy.EXPONENTIAL, 30, TimeUnit.SECONDS)
            .build()
        WorkManager.getInstance(context).enqueueUniquePeriodicWork(
            PERIODIC_WORK,
            ExistingPeriodicWorkPolicy.UPDATE,
            request,
        )
    }

    fun requestNow(context: Context) {
        val request = OneTimeWorkRequestBuilder<NotificationOutboxWorker>()
            .setConstraints(networkConstraints())
            .build()
        WorkManager.getInstance(context).enqueueUniqueWork(
            IMMEDIATE_WORK,
            ExistingWorkPolicy.KEEP,
            request,
        )
    }

    fun disable(context: Context) {
        WorkManager.getInstance(context).cancelUniqueWork(PERIODIC_WORK)
        WorkManager.getInstance(context).cancelUniqueWork(IMMEDIATE_WORK)
    }

    private fun networkConstraints(): Constraints = Constraints.Builder()
        .setRequiredNetworkType(NetworkType.CONNECTED)
        .build()
}
