package com.amazity.foyer.notes

import android.net.Uri
import com.amazity.foyer.network.ApiException
import com.amazity.foyer.network.FoyerApiClient
import java.util.UUID
import org.json.JSONObject

data class SyncCredentials(
    val endpoint: String,
    val token: String,
    val userId: String,
    val expiresAt: String,
)

data class FolderRecord(
    val id: String,
    val userId: String,
    val parentId: String?,
    val name: String,
    val position: Int,
    val revision: Long,
    val createdAt: String,
    val updatedAt: String,
    val deletedAt: String?,
)

data class NoteRecord(
    val id: String,
    val userId: String,
    val folderId: String,
    val title: String,
    val body: String,
    val revision: Long,
    val createdAt: String,
    val updatedAt: String,
    val deletedAt: String?,
)

class NotesConflictException(
    val code: String,
    val detail: String,
) : IllegalStateException(detail) {
    fun publicMessage(): String = when (code) {
        "stale_revision" ->
            "Stale revision: another device changed this item. The server copy will replace the rejected edit."
        "folder_not_empty" -> "Folder is not empty. Move or delete its notes and folders first."
        "cycle" -> "A folder cannot be moved into its own descendant."
        "invalid_parent" -> "That folder destination is not valid."
        "gone" -> "This item was deleted on the server and cannot be restored."
        else -> detail
    }
}

class NotesApi(private val api: FoyerApiClient) {
    suspend fun session(): JSONObject = api.request("/v1/session").requireJson()

    suspend fun syncCredentials(): SyncCredentials {
        val body = api.request("/v1/sync/credentials").requireJson()
        return SyncCredentials(
            endpoint = body.getString("endpoint"),
            token = body.getString("token"),
            userId = body.getString("userId"),
            expiresAt = body.optString("expiresAt"),
        )
    }

    suspend fun folders(): List<FolderRecord> {
        val body = api.request("/v1/folders").requireJson()
        return body.optJSONArray("folders").objects().map(::folderRecord)
    }

    suspend fun notes(folderId: String? = null): List<NoteRecord> {
        val path = if (folderId.isNullOrBlank()) "/v1/notes" else "/v1/notes?folderId=${Uri.encode(folderId)}"
        val body = api.request(path).requireJson()
        return body.optJSONArray("notes").objects().map(::noteRecord)
    }

    suspend fun createFolder(
        id: String = UUID.randomUUID().toString(),
        operationId: String = UUID.randomUUID().toString(),
        name: String,
        parentId: String? = null,
        position: Int? = null,
    ): FolderRecord = mutate(
        "/v1/folders",
        JSONObject()
            .put("operationId", operationId)
            .put("id", id)
            .put("name", name)
            .put("parentId", parentId ?: JSONObject.NULL)
            .apply { if (position != null) put("position", position) },
    ).let(::folderRecord)

    suspend fun renameFolder(
        id: String,
        expectedRevision: Long,
        name: String,
        operationId: String = UUID.randomUUID().toString(),
    ): FolderRecord = mutate(
        "/v1/folders/${Uri.encode(id)}/rename",
        JSONObject()
            .put("operationId", operationId)
            .put("expectedRevision", expectedRevision)
            .put("name", name),
    ).let(::folderRecord)

    suspend fun moveFolder(
        id: String,
        expectedRevision: Long,
        parentId: String?,
        position: Int? = null,
        operationId: String = UUID.randomUUID().toString(),
    ): FolderRecord = mutate(
        "/v1/folders/${Uri.encode(id)}/move",
        JSONObject()
            .put("operationId", operationId)
            .put("expectedRevision", expectedRevision)
            .put("parentId", parentId ?: JSONObject.NULL)
            .apply { if (position != null) put("position", position) },
    ).let(::folderRecord)

    suspend fun deleteFolder(
        id: String,
        expectedRevision: Long,
        operationId: String = UUID.randomUUID().toString(),
    ): FolderRecord = mutate(
        "/v1/folders/${Uri.encode(id)}/delete",
        JSONObject()
            .put("operationId", operationId)
            .put("expectedRevision", expectedRevision),
    ).let(::folderRecord)

    suspend fun createNote(
        id: String = UUID.randomUUID().toString(),
        operationId: String = UUID.randomUUID().toString(),
        folderId: String,
        title: String,
        body: String,
    ): NoteRecord = mutate(
        "/v1/notes",
        JSONObject()
            .put("operationId", operationId)
            .put("id", id)
            .put("folderId", folderId)
            .put("title", title)
            .put("body", body),
    ).let(::noteRecord)

    suspend fun updateNote(
        id: String,
        expectedRevision: Long,
        title: String,
        body: String,
        operationId: String = UUID.randomUUID().toString(),
    ): NoteRecord = mutate(
        "/v1/notes/${Uri.encode(id)}/update",
        JSONObject()
            .put("operationId", operationId)
            .put("expectedRevision", expectedRevision)
            .put("title", title)
            .put("body", body),
    ).let(::noteRecord)

    suspend fun moveNote(
        id: String,
        expectedRevision: Long,
        folderId: String,
        operationId: String = UUID.randomUUID().toString(),
    ): NoteRecord = mutate(
        "/v1/notes/${Uri.encode(id)}/move",
        JSONObject()
            .put("operationId", operationId)
            .put("expectedRevision", expectedRevision)
            .put("folderId", folderId),
    ).let(::noteRecord)

    suspend fun deleteNote(
        id: String,
        expectedRevision: Long,
        operationId: String = UUID.randomUUID().toString(),
    ): NoteRecord = mutate(
        "/v1/notes/${Uri.encode(id)}/delete",
        JSONObject()
            .put("operationId", operationId)
            .put("expectedRevision", expectedRevision),
    ).let(::noteRecord)

    private suspend fun mutate(path: String, payload: JSONObject): JSONObject {
        val response = api.request(path, "POST", payload)
        if (!response.successful) {
            val code = response.body?.optJSONObject("error")?.optString("code").orEmpty()
            val message = response.body?.optJSONObject("error")?.optString("message")
                ?: response.body?.toString()
                ?: "Foyer notes request failed (${response.status})"
            if (
                code == "stale_revision" || code == "conflict" || code == "cycle" ||
                code == "invalid_parent" || code == "folder_not_empty" || code == "gone"
            ) {
                throw NotesConflictException(code, message)
            }
            throw ApiException(response.status, message)
        }
        return response.body ?: JSONObject()
    }
}

private fun folderRecord(value: JSONObject) = FolderRecord(
    id = value.getString("id"),
    userId = value.optString("userId"),
    parentId = value.optionalId("parentId"),
    name = value.getString("name"),
    position = value.optInt("position"),
    revision = value.optLong("revision", 1L),
    createdAt = value.optString("createdAt"),
    updatedAt = value.optString("updatedAt"),
    deletedAt = value.optionalId("deletedAt"),
)

private fun noteRecord(value: JSONObject) = NoteRecord(
    id = value.getString("id"),
    userId = value.optString("userId"),
    folderId = value.getString("folderId"),
    title = value.getString("title"),
    body = value.getString("body"),
    revision = value.optLong("revision", 1L),
    createdAt = value.optString("createdAt"),
    updatedAt = value.optString("updatedAt"),
    deletedAt = value.optionalId("deletedAt"),
)

private fun JSONObject.optionalId(key: String): String? =
    if (isNull(key)) null else optString(key).takeIf(String::isNotBlank)

private fun org.json.JSONArray?.objects(): List<JSONObject> = buildList {
    val array = this@objects ?: return@buildList
    for (index in 0 until array.length()) array.optJSONObject(index)?.let(::add)
}

private fun com.amazity.foyer.network.ApiResponse.requireJson(): JSONObject {
    if (!successful) throw ApiException(status, body?.toString())
    return body ?: JSONObject()
}
