package com.amazity.foyer.data

import com.amazity.foyer.model.AgentTask
import com.amazity.foyer.model.ChatMessage
import com.amazity.foyer.model.ChatMessageState
import com.amazity.foyer.model.ChatRole
import com.amazity.foyer.model.TaskState
import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import org.json.JSONArray
import org.json.JSONObject

internal suspend fun FoyerDao.upsertActivitiesFrom(response: JSONObject) {
    val activities = response.optJSONArray("activities").activityObjects().map(::cachedActivity)
    replaceActivityList(activities)
}

internal suspend fun FoyerDao.upsertActivityFrom(response: JSONObject): CachedActivity {
    val value = response.optJSONObject("activity") ?: response
    val activity = cachedActivity(value)
    upsertActivity(activity)
    value.optJSONArray("messages")?.let { messages ->
        replaceActivityMessages(
            activity.id,
            messages.activityObjects().map { cachedActivityMessage(activity.id, it) },
        )
    }
    return activity
}

internal fun cachedActivity(value: JSONObject): CachedActivity {
    val schedule = value.optJSONObject("schedule")
    val definition = value.optJSONObject("definition")
    val runs = value.optJSONArray("runs")
    val latestFailedRunId = runs?.let { values ->
        (0 until values.length())
            .mapNotNull(values::optJSONObject)
            .firstOrNull { it.optString("status") == "failed" }
            ?.optString("id")
            ?.takeIf(String::isNotBlank)
    }
    return CachedActivity(
        id = value.getString("id"),
        kind = value.optString("kind", "conversation"),
        title = value.optString("title", "Untitled activity"),
        status = value.optString("status", "queued"),
        summary = value.optString("summary"),
        latestResult = value.nullableActivityString("latestResult"),
        scheduleFrequency = schedule?.nullableActivityString("frequency"),
        scheduleInterval = schedule?.optInt("interval")?.takeIf { it > 0 },
        scheduleTimezone = schedule?.nullableActivityString("timezone"),
        nextRunAt = schedule?.nullableActivityString("nextRunAt"),
        scheduleEnabled = schedule?.optBoolean("enabled") == true,
        definitionVersion = definition?.optInt("version")?.takeIf { it > 0 },
        jobObjective = definition?.nullableActivityString("objective"),
        jobInstructions = definition?.nullableActivityString("instructions"),
        expectedOutput = definition?.nullableActivityString("expectedOutput"),
        latestFailedRunId = latestFailedRunId,
        createdAt = value.optString("createdAt"),
        updatedAt = value.optString("updatedAt"),
    )
}

internal fun cachedActivityMessage(activityId: String, value: JSONObject) = CachedActivityMessage(
    id = value.getString("id"),
    activityId = activityId,
    role = value.optString("role", "assistant"),
    content = value.optString("content"),
    state = value.optString("state", "delivered"),
    runId = value.nullableActivityString("runId"),
    createdAt = value.optString("createdAt"),
)

internal fun agentTask(activity: CachedActivity): AgentTask {
    val taskState = when (activity.status) {
        "queued" -> TaskState.Queued
        "running" -> TaskState.Running
        "scheduled" -> TaskState.Scheduled
        "failed", "cancelled" -> TaskState.Failed
        else -> TaskState.Done
    }
    val statusLabel = when (taskState) {
        TaskState.Scheduled -> scheduleLabel(activity)
        TaskState.Running -> "running"
        TaskState.Queued -> "queued"
        TaskState.Done -> "completed"
        TaskState.Failed -> if (activity.status == "cancelled") "cancelled" else "failed"
    }
    val subtitle = "${if (activity.kind == "job") "job" else "chat"} · $statusLabel"
    return AgentTask(
        id = activity.id,
        title = activity.title,
        subtitle = subtitle,
        state = taskState,
        result = activity.latestResult,
        nextRunAt = activity.nextRunAt,
        scheduleFrequency = activity.scheduleFrequency,
        scheduleInterval = activity.scheduleInterval,
        scheduleTimezone = activity.scheduleTimezone,
        kind = activity.kind,
        definitionVersion = activity.definitionVersion,
        jobObjective = activity.jobObjective,
        jobInstructions = activity.jobInstructions,
        expectedOutput = activity.expectedOutput,
        latestFailedRunId = activity.latestFailedRunId,
    )
}

internal fun chatMessage(message: CachedActivityMessage): ChatMessage = ChatMessage(
    id = message.id,
    role = when (message.role) {
        "user" -> ChatRole.User
        "system" -> ChatRole.System
        else -> ChatRole.Assistant
    },
    content = message.content,
    timestamp = displayActivityTime(message.createdAt),
    state = when (message.state) {
        "sending" -> ChatMessageState.Sending
        "failed" -> ChatMessageState.Failed
        else -> ChatMessageState.Delivered
    },
)

private fun scheduleLabel(activity: CachedActivity): String {
    val frequency = when (activity.scheduleFrequency) {
        "daily" -> if (activity.scheduleInterval == 1) "daily" else "every ${activity.scheduleInterval} days"
        "weekly" -> if (activity.scheduleInterval == 1) "weekly" else "every ${activity.scheduleInterval} weeks"
        else -> "scheduled"
    }
    val time = activity.nextRunAt?.let(::displayActivityTime)
    return listOfNotNull(frequency, time?.takeIf(String::isNotBlank)).joinToString(" · ")
}

private fun displayActivityTime(value: String): String {
    val instant = runCatching { Instant.parse(value) }.getOrElse {
        runCatching { Instant.parse(value.replace(' ', 'T') + "Z") }.getOrNull()
    } ?: return "Now"
    return DateTimeFormatter.ofPattern("MMM d, HH:mm")
        .withZone(ZoneId.systemDefault())
        .format(instant)
}

private fun JSONArray?.activityObjects(): List<JSONObject> = buildList {
    val values = this@activityObjects ?: return@buildList
    for (index in 0 until values.length()) values.optJSONObject(index)?.let(::add)
}

private fun JSONObject.nullableActivityString(key: String): String? =
    takeUnless { isNull(key) }?.optString(key)?.takeIf(String::isNotBlank)
