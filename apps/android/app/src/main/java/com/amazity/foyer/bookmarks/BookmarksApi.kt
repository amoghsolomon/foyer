package com.amazity.foyer.bookmarks

import android.net.Uri
import com.amazity.foyer.network.ApiException
import com.amazity.foyer.network.FoyerApiClient
import java.util.UUID
import org.json.JSONArray
import org.json.JSONObject

data class BookmarksSyncCredentials(
    val endpoint: String,
    val token: String,
    val userId: String,
    val expiresAt: String,
)

data class BookmarkFolderRecord(
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

data class BookmarkRecord(
    val id: String,
    val userId: String,
    val folderId: String,
    val url: String,
    val title: String,
    val description: String,
    val tags: List<String>,
    val favorite: Boolean,
    val archived: Boolean,
    val position: Int,
    val revision: Long,
    val createdAt: String,
    val updatedAt: String,
    val deletedAt: String?,
)

class BookmarksConflictException(
    val code: String,
    val detail: String,
) : IllegalStateException(detail) {
    fun publicMessage(): String = when (code) {
        "stale_revision" ->
            "Stale revision: another device changed this item. The server copy will replace the rejected edit."
        "folder_not_empty" -> "Folder is not empty. Move or delete its bookmarks and folders first."
        "cycle" -> "A folder cannot be moved into its own descendant."
        "invalid_parent" -> "That folder destination is not valid."
        "gone" -> "This item was deleted on the server and cannot be restored."
        else -> detail
    }
}

class BookmarksApi(private val api: FoyerApiClient) {
    suspend fun session(): JSONObject = api.request("/v1/session").requireJson()

    suspend fun syncCredentials(): BookmarksSyncCredentials {
        val body = api.request("/v1/sync/credentials").requireJson()
        return BookmarksSyncCredentials(
            endpoint = body.getString("endpoint"),
            token = body.getString("token"),
            userId = body.getString("userId"),
            expiresAt = body.optString("expiresAt"),
        )
    }

    suspend fun folders(): List<BookmarkFolderRecord> {
        val body = api.request("/v1/bookmark-folders").requireJson()
        return body.optJSONArray("folders").objects().map(::folderRecord)
    }

    suspend fun bookmarks(folderId: String? = null): List<BookmarkRecord> {
        val path = if (folderId.isNullOrBlank()) {
            "/v1/bookmarks"
        } else {
            "/v1/bookmarks?folderId=${Uri.encode(folderId)}"
        }
        val body = api.request(path).requireJson()
        return body.optJSONArray("bookmarks").objects().map(::bookmarkRecord)
    }

    suspend fun createFolder(
        id: String = UUID.randomUUID().toString(),
        operationId: String = UUID.randomUUID().toString(),
        name: String,
        parentId: String? = null,
        position: Int? = null,
    ): BookmarkFolderRecord = mutate(
        "/v1/bookmark-folders",
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
    ): BookmarkFolderRecord = mutate(
        "/v1/bookmark-folders/${Uri.encode(id)}/rename",
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
    ): BookmarkFolderRecord = mutate(
        "/v1/bookmark-folders/${Uri.encode(id)}/move",
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
    ): BookmarkFolderRecord = mutate(
        "/v1/bookmark-folders/${Uri.encode(id)}/delete",
        JSONObject()
            .put("operationId", operationId)
            .put("expectedRevision", expectedRevision),
    ).let(::folderRecord)

    suspend fun createBookmark(
        id: String = UUID.randomUUID().toString(),
        operationId: String = UUID.randomUUID().toString(),
        folderId: String,
        url: String,
        title: String,
        description: String,
        tags: List<String>,
        favorite: Boolean = false,
        archived: Boolean = false,
        position: Int? = null,
    ): BookmarkRecord = mutate(
        "/v1/bookmarks",
        JSONObject()
            .put("operationId", operationId)
            .put("id", id)
            .put("folderId", folderId)
            .put("url", url)
            .put("title", title)
            .put("description", description)
            .put("tags", JSONArray(tags))
            .put("favorite", favorite)
            .put("archived", archived)
            .apply { if (position != null) put("position", position) },
    ).let(::bookmarkRecord)

    suspend fun updateBookmark(
        id: String,
        expectedRevision: Long,
        url: String,
        title: String,
        description: String,
        tags: List<String>,
        operationId: String = UUID.randomUUID().toString(),
    ): BookmarkRecord = mutate(
        "/v1/bookmarks/${Uri.encode(id)}/update",
        JSONObject()
            .put("operationId", operationId)
            .put("expectedRevision", expectedRevision)
            .put("url", url)
            .put("title", title)
            .put("description", description)
            .put("tags", JSONArray(tags)),
    ).let(::bookmarkRecord)

    suspend fun moveBookmark(
        id: String,
        expectedRevision: Long,
        folderId: String,
        position: Int? = null,
        operationId: String = UUID.randomUUID().toString(),
    ): BookmarkRecord = mutate(
        "/v1/bookmarks/${Uri.encode(id)}/move",
        JSONObject()
            .put("operationId", operationId)
            .put("expectedRevision", expectedRevision)
            .put("folderId", folderId)
            .apply { if (position != null) put("position", position) },
    ).let(::bookmarkRecord)

    suspend fun favoriteBookmark(
        id: String,
        expectedRevision: Long,
        favorite: Boolean,
        operationId: String = UUID.randomUUID().toString(),
    ): BookmarkRecord = mutate(
        "/v1/bookmarks/${Uri.encode(id)}/favorite",
        JSONObject()
            .put("operationId", operationId)
            .put("expectedRevision", expectedRevision)
            .put("favorite", favorite),
    ).let(::bookmarkRecord)

    suspend fun archiveBookmark(
        id: String,
        expectedRevision: Long,
        archived: Boolean,
        operationId: String = UUID.randomUUID().toString(),
    ): BookmarkRecord = mutate(
        "/v1/bookmarks/${Uri.encode(id)}/archive",
        JSONObject()
            .put("operationId", operationId)
            .put("expectedRevision", expectedRevision)
            .put("archived", archived),
    ).let(::bookmarkRecord)

    suspend fun deleteBookmark(
        id: String,
        expectedRevision: Long,
        operationId: String = UUID.randomUUID().toString(),
    ): BookmarkRecord = mutate(
        "/v1/bookmarks/${Uri.encode(id)}/delete",
        JSONObject()
            .put("operationId", operationId)
            .put("expectedRevision", expectedRevision),
    ).let(::bookmarkRecord)

    private suspend fun mutate(path: String, payload: JSONObject): JSONObject {
        val response = api.request(path, "POST", payload)
        if (!response.successful) {
            val code = response.body?.optJSONObject("error")?.optString("code").orEmpty()
            val message = response.body?.optJSONObject("error")?.optString("message")
                ?: response.body?.toString()
                ?: "Foyer bookmarks request failed (${response.status})"
            if (
                code == "stale_revision" || code == "conflict" || code == "cycle" ||
                code == "invalid_parent" || code == "folder_not_empty" || code == "gone"
            ) {
                throw BookmarksConflictException(code, message)
            }
            throw ApiException(response.status, message)
        }
        return response.body ?: JSONObject()
    }
}

internal fun folderRecord(value: JSONObject) = BookmarkFolderRecord(
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

internal fun bookmarkRecord(value: JSONObject) = BookmarkRecord(
    id = value.getString("id"),
    userId = value.optString("userId"),
    folderId = value.getString("folderId"),
    url = value.getString("url"),
    title = value.getString("title"),
    description = value.optString("description"),
    tags = value.optJSONArray("tags").strings(),
    favorite = value.optBoolean("favorite"),
    archived = value.optBoolean("archived"),
    position = value.optInt("position"),
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

private fun org.json.JSONArray?.strings(): List<String> = buildList {
    val array = this@strings ?: return@buildList
    for (index in 0 until array.length()) {
        array.optString(index).takeIf(String::isNotBlank)?.let(::add)
    }
}

private fun com.amazity.foyer.network.ApiResponse.requireJson(): JSONObject {
    if (!successful) throw ApiException(status, body?.toString())
    return body ?: JSONObject()
}
