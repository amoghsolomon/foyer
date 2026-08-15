package com.amazity.foyer.sync

import android.content.Context
import androidx.work.BackoffPolicy
import androidx.work.Constraints
import androidx.work.CoroutineWorker
import androidx.work.ExistingPeriodicWorkPolicy
import androidx.work.NetworkType
import androidx.work.OneTimeWorkRequestBuilder
import androidx.work.PeriodicWorkRequestBuilder
import androidx.work.WorkManager
import androidx.work.WorkerParameters
import com.amazity.foyer.data.CachedMemory
import com.amazity.foyer.data.FoyerDatabase
import com.amazity.foyer.data.upsertActivitiesFrom
import com.amazity.foyer.data.upsertActivityFrom
import com.amazity.foyer.auth.foyerApiClient
import com.amazity.foyer.auth.foyerAuthSession
import com.amazity.foyer.data.SyncState
import java.util.concurrent.TimeUnit
import java.time.Instant
import org.json.JSONObject

class FoyerSyncWorker(
    appContext: Context,
    params: WorkerParameters,
) : CoroutineWorker(appContext, params) {
    override suspend fun doWork(): Result {
        if (!foyerAuthSession(applicationContext).hasPreviousAccess()) return Result.retry()
        val dao = FoyerDatabase.get(applicationContext).foyerDao()
        val api = foyerApiClient(applicationContext)
        val outcome = runCatching {
            val pending = dao.pendingMutations().filter { mutation ->
                mutation.entityType != "calendar" && mutation.entityType != "task"
            }
            if (pending.isNotEmpty()) {
                val accepted = api.sendMutations(pending)
                    .optJSONArray("results")
                    ?.let { array ->
                        buildList {
                            for (index in 0 until array.length()) {
                                array.optJSONObject(index)?.optString("mutationId")
                                    ?.takeIf(String::isNotBlank)
                                    ?.let(::add)
                            }
                        }
                    }
                    .orEmpty()
                if (accepted.isNotEmpty()) dao.deleteMutations(accepted)
                val remaining = pending.map { it.mutationId } - accepted.toSet()
                if (remaining.isNotEmpty()) dao.incrementMutationAttempts(remaining)
            }

            val leftoverAgendaMutations = dao.pendingMutations()
                .filter { it.entityType == "calendar" || it.entityType == "task" }
                .map { it.mutationId }
            if (leftoverAgendaMutations.isNotEmpty()) {
                dao.deleteMutations(leftoverAgendaMutations)
            }

            val cursor = dao.syncState(CURSOR_KEY)?.toLongOrNull() ?: 0L
            runCatching {
                val page = api.changes(cursor)
                page.optJSONArray("changes")?.let { changes ->
                    for (index in 0 until changes.length()) {
                        changes.optJSONObject(index)?.let { applyChange(dao, it) }
                    }
                }
                dao.putSyncState(SyncState(CURSOR_KEY, page.optLong("nextCursor", cursor).toString()))
            }
            dao.upsertActivitiesFrom(api.activities())
            api.homeBriefing(java.time.ZoneId.systemDefault().id)
                .optJSONObject("briefing")
                ?.let { briefing ->
                    val insight = briefing.optJSONObject("insight")
                    val target = insight?.optJSONObject("target")
                    dao.upsertHomeBriefing(
                        com.amazity.foyer.data.CachedHomeBriefing(
                            dailyMessage = briefing.optString("dailyMessage"),
                            insightMessage = insight?.optString("message")?.takeIf(String::isNotBlank),
                            targetType = target?.optString("type")?.takeIf(String::isNotBlank),
                            targetId = target?.optString("id")?.takeIf(String::isNotBlank),
                            targetLabel = target?.optString("label")?.takeIf(String::isNotBlank),
                            generatedAt = briefing.optString("generatedAt"),
                            expiresAt = briefing.optString("expiresAt"),
                        ),
                    )
                }
        }
        return outcome.fold(
            onSuccess = {
                dao.putSyncState(SyncState(SyncMetadata.LAST_SUCCESS, Instant.now().toString()))
                dao.putSyncState(SyncState(SyncMetadata.LAST_ERROR, ""))
                Result.success()
            },
            onFailure = { error ->
                dao.putSyncState(
                    SyncState(
                        SyncMetadata.LAST_ERROR,
                        (error.message ?: error.javaClass.simpleName).take(500),
                    ),
                )
                if (runAttemptCount >= 5) Result.failure() else Result.retry()
            },
        )
    }

    private suspend fun applyChange(dao: com.amazity.foyer.data.FoyerDao, change: JSONObject) {
        val type = change.optString("entityType")
        val id = change.optString("entityId")
        if (change.optString("operation") == "delete") {
            when (type) {
                "memory" -> dao.deleteMemory(id)
                "activity" -> dao.deleteActivityCache(id)
            }
            return
        }
        val payload = change.optJSONObject("payload") ?: return
        when (type) {
            "memory" -> dao.upsertMemory(
                CachedMemory(
                    id = id,
                    kind = payload.optString("kind", "fact"),
                    content = payload.optString("content"),
                    importance = payload.optDouble("importance", 0.5),
                    confidence = payload.optDouble("confidence", 0.8),
                    updatedAt = payload.optString("updated_at", change.optString("changedAt")),
                ),
            )
            "activity" -> dao.upsertActivityFrom(payload)
        }
    }

    private companion object {
        const val CURSOR_KEY = "server_change_cursor"
    }
}
object SyncMetadata {
    const val LAST_SUCCESS = "last_successful_sync"
    const val LAST_ERROR = "last_sync_error"
}

object SyncScheduler {
    private const val PERIODIC_SYNC = "foyer-periodic-sync"

    fun ensureScheduled(context: Context) {
        val request = PeriodicWorkRequestBuilder<FoyerSyncWorker>(15, TimeUnit.MINUTES)
            .setConstraints(Constraints.Builder().setRequiredNetworkType(NetworkType.CONNECTED).build())
            .setBackoffCriteria(BackoffPolicy.EXPONENTIAL, 30, TimeUnit.SECONDS)
            .build()
        WorkManager.getInstance(context).enqueueUniquePeriodicWork(
            PERIODIC_SYNC,
            ExistingPeriodicWorkPolicy.UPDATE,
            request,
        )
    }

    fun requestNow(context: Context) {
        val request = OneTimeWorkRequestBuilder<FoyerSyncWorker>()
            .setConstraints(Constraints.Builder().setRequiredNetworkType(NetworkType.CONNECTED).build())
            .build()
        WorkManager.getInstance(context).enqueue(request)
    }
}
