package com.amazity.foyer.tasks

import android.net.Uri
import com.amazity.foyer.model.TaskDue
import com.amazity.foyer.network.ApiException
import com.amazity.foyer.network.FoyerApiClient
import java.util.UUID
import org.json.JSONObject

data class TaskListRecord(
    val id: String,
    val userId: String,
    val name: String,
    val position: Int,
    val href: String,
    val etag: String?,
    val revision: Long,
    val createdAt: String,
    val updatedAt: String,
    val deletedAt: String?,
)

data class TaskRecord(
    val id: String,
    val userId: String,
    val listId: String,
    val title: String,
    val description: String,
    val due: TaskDue?,
    val priority: Int,
    val completed: Boolean,
    val completedAt: String?,
    val position: Int,
    val href: String,
    val etag: String,
    val revision: Long,
    val createdAt: String,
    val updatedAt: String,
    val deletedAt: String?,
)

class TasksConflictException(
    val code: String,
    val detail: String,
) : IllegalStateException(detail) {
    fun publicMessage(): String = when (code) {
        "stale_revision" ->
            "Stale revision: another device changed this item. The server copy will replace the rejected edit."
        "gone" -> "This item was deleted on the server and cannot be restored."
        "invalid_parent" -> "That task list destination is not valid."
        else -> detail
    }
}

class TasksApi(private val api: FoyerApiClient) {
    suspend fun syncCredentials(): com.amazity.foyer.notes.SyncCredentials {
        val body = api.request("/v1/sync/credentials").let { response ->
            if (!response.successful) throw ApiException(response.status, response.body?.toString())
            response.body ?: JSONObject()
        }
        return com.amazity.foyer.notes.SyncCredentials(
            endpoint = body.getString("endpoint"),
            token = body.getString("token"),
            userId = body.getString("userId"),
            expiresAt = body.optString("expiresAt"),
        )
    }

    suspend fun createList(
        id: String = UUID.randomUUID().toString(),
        operationId: String = UUID.randomUUID().toString(),
        name: String,
        position: Int? = null,
    ): TaskListRecord = mutate(
        "/v1/task-lists",
        JSONObject()
            .put("operationId", operationId)
            .put("id", id)
            .put("name", name)
            .apply { if (position != null) put("position", position) },
    ).let(::taskListRecord)

    suspend fun renameList(
        id: String,
        expectedRevision: Long,
        name: String,
        operationId: String = UUID.randomUUID().toString(),
    ): TaskListRecord = mutate(
        "/v1/task-lists/${Uri.encode(id)}/rename",
        JSONObject()
            .put("operationId", operationId)
            .put("expectedRevision", expectedRevision)
            .put("name", name),
    ).let(::taskListRecord)

    suspend fun deleteList(
        id: String,
        expectedRevision: Long,
        operationId: String = UUID.randomUUID().toString(),
    ): TaskListRecord = mutate(
        "/v1/task-lists/${Uri.encode(id)}/delete",
        JSONObject()
            .put("operationId", operationId)
            .put("expectedRevision", expectedRevision),
    ).let(::taskListRecord)

    suspend fun createTask(
        id: String = UUID.randomUUID().toString(),
        operationId: String = UUID.randomUUID().toString(),
        listId: String,
        title: String,
        description: String,
        due: TaskDue? = null,
        priority: Int = 0,
        position: Int? = null,
    ): TaskRecord = mutate(
        "/v1/tasks",
        JSONObject()
            .put("operationId", operationId)
            .put("id", id)
            .put("listId", listId)
            .put("title", title)
            .put("description", description)
            .put("priority", priority)
            .put("due", due.toJson())
            .apply { if (position != null) put("position", position) },
    ).let(::taskRecord)

    suspend fun updateTask(
        id: String,
        expectedRevision: Long,
        title: String,
        description: String,
        due: TaskDue?,
        priority: Int,
        position: Int,
        operationId: String = UUID.randomUUID().toString(),
    ): TaskRecord = mutate(
        "/v1/tasks/${Uri.encode(id)}/update",
        JSONObject()
            .put("operationId", operationId)
            .put("expectedRevision", expectedRevision)
            .put("title", title)
            .put("description", description)
            .put("due", due.toJson())
            .put("priority", priority)
            .put("position", position),
    ).let(::taskRecord)

    suspend fun moveTask(
        id: String,
        expectedRevision: Long,
        listId: String,
        position: Int? = null,
        operationId: String = UUID.randomUUID().toString(),
    ): TaskRecord = mutate(
        "/v1/tasks/${Uri.encode(id)}/move",
        JSONObject()
            .put("operationId", operationId)
            .put("expectedRevision", expectedRevision)
            .put("listId", listId)
            .apply { if (position != null) put("position", position) },
    ).let(::taskRecord)

    suspend fun completeTask(
        id: String,
        expectedRevision: Long,
        operationId: String = UUID.randomUUID().toString(),
    ): TaskRecord = mutate(
        "/v1/tasks/${Uri.encode(id)}/complete",
        JSONObject()
            .put("operationId", operationId)
            .put("expectedRevision", expectedRevision),
    ).let(::taskRecord)

    suspend fun reopenTask(
        id: String,
        expectedRevision: Long,
        operationId: String = UUID.randomUUID().toString(),
    ): TaskRecord = mutate(
        "/v1/tasks/${Uri.encode(id)}/reopen",
        JSONObject()
            .put("operationId", operationId)
            .put("expectedRevision", expectedRevision),
    ).let(::taskRecord)

    suspend fun deleteTask(
        id: String,
        expectedRevision: Long,
        operationId: String = UUID.randomUUID().toString(),
    ): TaskRecord = mutate(
        "/v1/tasks/${Uri.encode(id)}/delete",
        JSONObject()
            .put("operationId", operationId)
            .put("expectedRevision", expectedRevision),
    ).let(::taskRecord)

    private suspend fun mutate(path: String, payload: JSONObject): JSONObject {
        val response = api.request(path, "POST", payload)
        if (!response.successful) {
            val code = response.body?.optJSONObject("error")?.optString("code").orEmpty()
            val message = response.body?.optJSONObject("error")?.optString("message")
                ?: response.body?.toString()
                ?: "Foyer tasks request failed (${response.status})"
            if (code == "stale_revision" || code == "conflict" || code == "invalid_parent" || code == "gone") {
                throw TasksConflictException(code, message)
            }
            throw ApiException(response.status, message)
        }
        return response.body ?: JSONObject()
    }
}

private fun taskListRecord(value: JSONObject) = TaskListRecord(
    id = value.getString("id"),
    userId = value.optString("userId"),
    name = value.getString("name"),
    position = value.optInt("position"),
    href = value.optString("href"),
    etag = value.optionalId("etag"),
    revision = value.optLong("revision", 1L),
    createdAt = value.optString("createdAt"),
    updatedAt = value.optString("updatedAt"),
    deletedAt = value.optionalId("deletedAt"),
)

private fun taskRecord(value: JSONObject) = TaskRecord(
    id = value.getString("id"),
    userId = value.optString("userId"),
    listId = value.getString("listId"),
    title = value.getString("title"),
    description = value.optString("description"),
    due = value.optJSONObject("due")?.let(::taskDue),
    priority = value.optInt("priority"),
    completed = value.optBoolean("completed"),
    completedAt = value.optionalId("completedAt"),
    position = value.optInt("position"),
    href = value.optString("href"),
    etag = value.optString("etag"),
    revision = value.optLong("revision", 1L),
    createdAt = value.optString("createdAt"),
    updatedAt = value.optString("updatedAt"),
    deletedAt = value.optionalId("deletedAt"),
)

private fun taskDue(value: JSONObject) = TaskDue(
    local = value.getString("local"),
    timeZone = value.optionalId("timeZone"),
    allDay = value.optBoolean("allDay"),
    at = value.optionalId("at"),
)

private fun TaskDue?.toJson(): Any {
    val due = this ?: return JSONObject.NULL
    return JSONObject()
        .put("local", due.local)
        .put("timeZone", due.timeZone ?: JSONObject.NULL)
        .put("allDay", due.allDay)
        .put("at", due.at ?: JSONObject.NULL)
}

private fun JSONObject.optionalId(key: String): String? =
    if (isNull(key)) null else optString(key).takeIf(String::isNotBlank)
