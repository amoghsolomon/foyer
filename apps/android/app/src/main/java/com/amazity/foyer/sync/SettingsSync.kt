package com.amazity.foyer.sync

import android.content.Context
import androidx.work.BackoffPolicy
import androidx.work.Constraints
import androidx.work.CoroutineWorker
import androidx.work.ExistingWorkPolicy
import androidx.work.NetworkType
import androidx.work.OneTimeWorkRequestBuilder
import androidx.work.WorkManager
import androidx.work.WorkerParameters
import com.amazity.foyer.auth.foyerApiClient
import com.amazity.foyer.auth.foyerAuthSession
import com.amazity.foyer.data.UserSettingsStore
import java.util.concurrent.TimeUnit

class SettingsSyncWorker(
    appContext: Context,
    params: WorkerParameters,
) : CoroutineWorker(appContext, params) {
    override suspend fun doWork(): Result {
        val store = UserSettingsStore(applicationContext)
        val timezone = store.pendingTimezone()
        val whitelist = store.pendingNotificationWhitelist()
        if (timezone == null && whitelist == null) return Result.success()
        if (!foyerAuthSession(applicationContext).hasPreviousAccess()) return Result.retry()
        return runCatching {
            foyerApiClient(applicationContext).updateSettings(
                timezone = timezone ?: store.timezone().takeIf { whitelist != null },
                notificationWhitelist = whitelist?.toList()?.sorted(),
            )
            timezone?.let(store::markTimezoneSynced)
            whitelist?.let(store::markNotificationWhitelistSynced)
        }.fold(
            onSuccess = { Result.success() },
            onFailure = { if (runAttemptCount >= 5) Result.failure() else Result.retry() },
        )
    }
}

object SettingsSyncScheduler {
    private const val UNIQUE_WORK = "foyer-settings-sync"

    fun requestNow(context: Context) {
        val request = OneTimeWorkRequestBuilder<SettingsSyncWorker>()
            .setConstraints(Constraints.Builder().setRequiredNetworkType(NetworkType.CONNECTED).build())
            .setBackoffCriteria(BackoffPolicy.EXPONENTIAL, 30, TimeUnit.SECONDS)
            .build()
        WorkManager.getInstance(context).enqueueUniqueWork(
            UNIQUE_WORK,
            ExistingWorkPolicy.REPLACE,
            request,
        )
    }
}
