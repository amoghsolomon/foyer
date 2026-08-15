package com.amazity.foyer.bookmarks

import android.content.Context
import com.amazity.foyer.BuildConfig
import com.amazity.foyer.model.BookmarkFolder
import com.amazity.foyer.model.BookmarkItem
import com.amazity.foyer.model.BookmarksCatalog
import com.amazity.foyer.model.BookmarksStatus
import com.powersync.PowerSyncDatabase
import com.powersync.db.getLongOptional
import com.powersync.db.getString
import com.powersync.db.getStringOptional
import java.time.Instant
import java.util.UUID
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import org.json.JSONArray
import org.json.JSONObject

/**
 * Optimistic bookmark operations against the shared personal-data replica.
 * This store never connects or disconnects PowerSync on its own.
 */
class BookmarksStore(
    @Suppress("UNUSED_PARAMETER") context: Context,
    private val databaseProvider: suspend () -> PowerSyncDatabase,
) {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val _catalog = MutableStateFlow(BookmarksCatalog(emptyList(), emptyList(), emptyList()))
    private var watchJob: Job? = null
    private var statusJob: Job? = null
    private var startRequested = false
    @Volatile private var lastConflict: Pair<String, String>? = null
    @Volatile private var attachedDatabase: PowerSyncDatabase? = null

    val catalog: StateFlow<BookmarksCatalog> = _catalog

    @Suppress("UNUSED_PARAMETER")
    constructor(context: Context, api: BookmarksApi, databaseProvider: suspend () -> PowerSyncDatabase) : this(
        context,
        databaseProvider,
    )

    @Synchronized
    fun start() {
        if (startRequested) return
        startRequested = true
        scope.launch { initialize() }
    }

    fun stop() {
        watchJob?.cancel()
        statusJob?.cancel()
        startRequested = false
        attachedDatabase = null
    }

    fun reportConflict(code: String, message: String) {
        lastConflict = code to message
        attachedDatabase?.let { publishStatus(it) }
    }

    fun markUnavailable(message: String) {
        _catalog.update {
            it.copy(
                status = it.status.copy(
                    loading = false,
                    connected = false,
                    offline = true,
                    lastError = message,
                    usingPowerSync = false,
                ),
            )
        }
    }

    suspend fun refreshFromServer() {
        publishStatus(requireDatabase())
    }

    suspend fun ensureInbox(): BookmarkFolder {
        val existing = _catalog.value.childFolders(null).firstOrNull { it.name.equals("Inbox", true) }
            ?: _catalog.value.folders.firstOrNull()
        return existing ?: createFolder("Inbox")
    }

    suspend fun createFolder(name: String, parentId: String? = null): BookmarkFolder {
        val cleanName = requiredFolderName(name)
        val id = UUID.randomUUID().toString()
        val now = Instant.now().toString()
        requireDatabase().execute(
            """INSERT INTO bookmarks_folders
                (id, user_id, parent_id, name, position, revision, created_at, updated_at,
                 client_operation, operation_id, expected_revision, deleted_local)
                VALUES (?, '', ?, ?, 0, 1, ?, ?, 'create', ?, NULL, 0)""",
            listOf(id, parentId, cleanName, now, now, UUID.randomUUID().toString()),
        )
        return BookmarkFolder(id = id, name = cleanName, parentId = parentId, position = 0, revision = 1)
    }

    suspend fun renameFolder(folder: BookmarkFolder, name: String): BookmarkFolder {
        val cleanName = requiredFolderName(name)
        val nextRevision = folder.revision + 1
        requireDatabase().execute(
            """UPDATE bookmarks_folders
                SET name = ?, revision = ?, updated_at = ?, client_operation = 'rename',
                    operation_id = ?, expected_revision = ?
                WHERE id = ?""",
            listOf(
                cleanName,
                nextRevision,
                Instant.now().toString(),
                UUID.randomUUID().toString(),
                folder.revision,
                folder.id,
            ),
        )
        return folder.copy(name = cleanName, revision = nextRevision)
    }

    suspend fun moveFolder(folder: BookmarkFolder, parentId: String?): BookmarkFolder {
        _catalog.value.validateFolderMove(folder, parentId)?.let { error(it) }
        val nextRevision = folder.revision + 1
        requireDatabase().execute(
            """UPDATE bookmarks_folders
                SET parent_id = ?, revision = ?, updated_at = ?, client_operation = 'move',
                    operation_id = ?, expected_revision = ?
                WHERE id = ?""",
            listOf(
                parentId,
                nextRevision,
                Instant.now().toString(),
                UUID.randomUUID().toString(),
                folder.revision,
                folder.id,
            ),
        )
        return folder.copy(parentId = parentId, revision = nextRevision)
    }

    suspend fun deleteFolder(folder: BookmarkFolder) {
        _catalog.value.validateFolderDelete(folder)?.let { error(it) }
        requireDatabase().execute(
            """UPDATE bookmarks_folders
                SET deleted_local = 1, revision = ?, updated_at = ?, client_operation = 'delete',
                    operation_id = ?, expected_revision = ?
                WHERE id = ?""",
            listOf(
                folder.revision + 1,
                Instant.now().toString(),
                UUID.randomUUID().toString(),
                folder.revision,
                folder.id,
            ),
        )
    }

    suspend fun createBookmark(
        folderId: String,
        url: String,
        title: String,
        description: String,
        tags: List<String>,
        favorite: Boolean = false,
    ): BookmarkItem {
        val cleanUrl = validateBookmarkUrl(url)
        val cleanTitle = requiredBookmarkTitle(title)
        val cleanDescription = losslessDescription(description)
        val cleanTags = normalizeBookmarkTags(tags)
        val id = UUID.randomUUID().toString()
        val now = Instant.now().toString()
        requireDatabase().execute(
            """INSERT INTO bookmarks
                (id, user_id, folder_id, url, title, description, tags, favorite, archived, position,
                 revision, created_at, updated_at, client_operation, operation_id, expected_revision, deleted_local)
                VALUES (?, '', ?, ?, ?, ?, ?, ?, 0, 0, 1, ?, ?, 'create', ?, NULL, 0)""",
            listOf(
                id,
                folderId,
                cleanUrl,
                cleanTitle,
                cleanDescription,
                encodeTags(cleanTags),
                if (favorite) 1 else 0,
                now,
                now,
                UUID.randomUUID().toString(),
            ),
        )
        return vaultBookmark(
            BookmarkRecord(
                id = id,
                userId = "",
                folderId = folderId,
                url = cleanUrl,
                title = cleanTitle,
                description = cleanDescription,
                tags = cleanTags,
                favorite = favorite,
                archived = false,
                position = 0,
                revision = 1,
                createdAt = now,
                updatedAt = now,
                deletedAt = null,
            ),
        )
    }

    suspend fun updateBookmark(
        bookmark: BookmarkItem,
        url: String,
        title: String,
        description: String,
        tags: List<String>,
        folderId: String = bookmark.folderId,
    ): BookmarkItem {
        val cleanUrl = validateBookmarkUrl(url)
        val cleanTitle = requiredBookmarkTitle(title)
        val cleanDescription = losslessDescription(description)
        val cleanTags = normalizeBookmarkTags(tags)
        val db = requireDatabase()
        val now = Instant.now().toString()
        var revision = bookmark.revision
        val operationId = UUID.randomUUID().toString()
        val payload = JSONObject()
            .put("operationId", operationId)
            .put("url", cleanUrl)
            .put("title", cleanTitle)
            .put("description", cleanDescription)
            .put("tags", JSONArray(cleanTags))
            .toString()
        db.execute(
            """UPDATE bookmarks
                SET url = ?, title = ?, description = ?, tags = ?, revision = ?, updated_at = ?,
                    client_operation = 'update', operation_id = ?, expected_revision = ?, client_payload = ?
                WHERE id = ?""",
            listOf(
                cleanUrl,
                cleanTitle,
                cleanDescription,
                encodeTags(cleanTags),
                ++revision,
                now,
                operationId,
                bookmark.revision,
                payload,
                bookmark.id,
            ),
        )
        if (folderId != bookmark.folderId) {
            db.execute(
                """UPDATE bookmarks
                    SET folder_id = ?, revision = ?, updated_at = ?, client_operation = 'move',
                        operation_id = ?, expected_revision = ?
                    WHERE id = ?""",
                listOf(
                    folderId,
                    ++revision,
                    now,
                    UUID.randomUUID().toString(),
                    revision - 1,
                    bookmark.id,
                ),
            )
        }
        return bookmark.copy(
            url = cleanUrl,
            title = cleanTitle,
            description = cleanDescription,
            tags = cleanTags,
            folderId = folderId,
            revision = revision,
            updatedAt = now,
        )
    }

    suspend fun setFavorite(bookmark: BookmarkItem, favorite: Boolean): BookmarkItem {
        val operationId = UUID.randomUUID().toString()
        val payload = JSONObject()
            .put("operationId", operationId)
            .put("favorite", favorite)
            .toString()
        requireDatabase().execute(
            """UPDATE bookmarks
                SET favorite = ?, revision = ?, updated_at = ?, client_operation = 'favorite',
                    operation_id = ?, expected_revision = ?, client_payload = ?
                WHERE id = ?""",
            listOf(
                if (favorite) 1 else 0,
                bookmark.revision + 1,
                Instant.now().toString(),
                operationId,
                bookmark.revision,
                payload,
                bookmark.id,
            ),
        )
        return bookmark.copy(favorite = favorite, revision = bookmark.revision + 1)
    }

    suspend fun setArchived(bookmark: BookmarkItem, archived: Boolean): BookmarkItem {
        val operationId = UUID.randomUUID().toString()
        val payload = JSONObject()
            .put("operationId", operationId)
            .put("archived", archived)
            .toString()
        requireDatabase().execute(
            """UPDATE bookmarks
                SET archived = ?, revision = ?, updated_at = ?, client_operation = 'archive',
                    operation_id = ?, expected_revision = ?, client_payload = ?
                WHERE id = ?""",
            listOf(
                if (archived) 1 else 0,
                bookmark.revision + 1,
                Instant.now().toString(),
                operationId,
                bookmark.revision,
                payload,
                bookmark.id,
            ),
        )
        return bookmark.copy(archived = archived, revision = bookmark.revision + 1)
    }

    suspend fun deleteBookmark(bookmark: BookmarkItem) {
        requireDatabase().execute(
            """UPDATE bookmarks
                SET deleted_local = 1, revision = ?, updated_at = ?, client_operation = 'delete',
                    operation_id = ?, expected_revision = ?
                WHERE id = ?""",
            listOf(
                bookmark.revision + 1,
                Instant.now().toString(),
                UUID.randomUUID().toString(),
                bookmark.revision,
                bookmark.id,
            ),
        )
    }

    private suspend fun initialize() {
        try {
            val db = requireDatabase()
            watchReplica(db)
            watchStatus(db)
        } catch (error: Throwable) {
            markUnavailable("PowerSync replica unavailable: ${error.message}")
        }
    }

    private fun watchReplica(db: PowerSyncDatabase) {
        val folders = db.watch(
            """SELECT id, user_id, parent_id, name, position, revision, created_at, updated_at
                FROM bookmarks_folders WHERE COALESCE(deleted_local, 0) = 0""",
        ) { cursor ->
            BookmarkFolderRecord(
                id = cursor.getString("id"),
                userId = cursor.getStringOptional("user_id").orEmpty(),
                parentId = cursor.getStringOptional("parent_id"),
                name = cursor.getString("name"),
                position = cursor.getLongOptional("position")?.toInt() ?: 0,
                revision = cursor.getLongOptional("revision") ?: 1L,
                createdAt = cursor.getStringOptional("created_at").orEmpty(),
                updatedAt = cursor.getStringOptional("updated_at").orEmpty(),
                deletedAt = null,
            )
        }
        val bookmarks = db.watch(
            """SELECT id, user_id, folder_id, url, title, description, tags, favorite, archived,
                      position, revision, created_at, updated_at
                FROM bookmarks WHERE COALESCE(deleted_local, 0) = 0""",
        ) { cursor ->
            BookmarkRecord(
                id = cursor.getString("id"),
                userId = cursor.getStringOptional("user_id").orEmpty(),
                folderId = cursor.getString("folder_id"),
                url = cursor.getString("url"),
                title = cursor.getString("title"),
                description = cursor.getStringOptional("description").orEmpty(),
                tags = decodeTags(cursor.getStringOptional("tags")),
                favorite = flagValue(cursor.getLongOptional("favorite"), null),
                archived = flagValue(cursor.getLongOptional("archived"), null),
                position = cursor.getLongOptional("position")?.toInt() ?: 0,
                revision = cursor.getLongOptional("revision") ?: 1L,
                createdAt = cursor.getStringOptional("created_at").orEmpty(),
                updatedAt = cursor.getStringOptional("updated_at").orEmpty(),
                deletedAt = null,
            )
        }
        val pending = db.watch(
            """SELECT
                (SELECT COUNT(*) FROM bookmarks_folders WHERE operation_id IS NOT NULL) +
                (SELECT COUNT(*) FROM bookmarks WHERE operation_id IS NOT NULL) AS count""",
        ) { cursor -> cursor.getLongOptional("count")?.toInt() ?: 0 }
        watchJob = scope.launch {
            combine(folders, bookmarks, pending) { folderRows, bookmarkRows, pendingRows ->
                Triple(folderRows, bookmarkRows, pendingRows.firstOrNull() ?: 0)
            }.collect { (folderRows, bookmarkRows, pendingCount) ->
                val status = db.currentStatus
                _catalog.value = bookmarksCatalog(
                    folders = folderRows,
                    bookmarks = bookmarkRows,
                    status = BookmarksStatus(
                        loading = false,
                        connected = status.connected,
                        offline = !status.connected,
                        pendingUploads = pendingCount,
                        lastError = status.anyError?.toString(),
                        conflictCode = lastConflict?.first,
                        conflictMessage = lastConflict?.second,
                        developmentAuth = BuildConfig.FOYER_DEVELOPMENT_AUTH,
                        usingPowerSync = true,
                    ),
                )
            }
        }
    }

    private fun watchStatus(db: PowerSyncDatabase) {
        statusJob = scope.launch {
            db.currentStatus.asFlow().collect { publishStatus(db) }
        }
    }

    private fun publishStatus(db: PowerSyncDatabase) {
        val sync = db.currentStatus
        _catalog.update { current ->
            current.copy(
                status = current.status.copy(
                    loading = false,
                    connected = sync.connected,
                    offline = !sync.connected,
                    lastError = sync.anyError?.toString(),
                    conflictCode = lastConflict?.first,
                    conflictMessage = lastConflict?.second,
                    developmentAuth = BuildConfig.FOYER_DEVELOPMENT_AUTH,
                    usingPowerSync = true,
                ),
            )
        }
    }

    private suspend fun requireDatabase(): PowerSyncDatabase {
        attachedDatabase?.let { return it }
        val db = databaseProvider()
        attachedDatabase = db
        return db
    }

    companion object {
        fun attach(
            context: Context,
            api: BookmarksApi,
            databaseProvider: suspend () -> PowerSyncDatabase,
        ): BookmarksStore = BookmarksStore(context, api, databaseProvider)
    }
}

internal fun encodeTags(tags: List<String>): String =
    tags.joinToString(prefix = "[", postfix = "]") { tag -> jsonString(tag) }

internal fun decodeTags(value: String?): List<String> {
    if (value.isNullOrBlank() || value == "null") return emptyList()
    val trimmed = value.trim()
    if (!trimmed.startsWith('[') || !trimmed.endsWith(']')) return emptyList()
    val body = trimmed.substring(1, trimmed.lastIndex).trim()
    if (body.isEmpty()) return emptyList()
    return buildList {
        var index = 0
        while (index < body.length) {
            while (index < body.length && (body[index].isWhitespace() || body[index] == ',')) index++
            if (index >= body.length) break
            if (body[index] != '"') return emptyList()
            val parsed = readJsonString(body, index) ?: return emptyList()
            add(parsed.first)
            index = parsed.second
        }
    }
}

private fun jsonString(value: String): String = buildString {
    append('"')
    value.forEach { ch ->
        when (ch) {
            '\\' -> append("\\\\")
            '"' -> append("\\\"")
            '\n' -> append("\\n")
            '\r' -> append("\\r")
            '\t' -> append("\\t")
            else -> append(ch)
        }
    }
    append('"')
}

private fun readJsonString(source: String, start: Int): Pair<String, Int>? {
    if (start >= source.length || source[start] != '"') return null
    val out = StringBuilder()
    var index = start + 1
    while (index < source.length) {
        when (val ch = source[index]) {
            '\\' -> {
                val next = source.getOrNull(index + 1) ?: return null
                out.append(
                    when (next) {
                        'n' -> '\n'
                        'r' -> '\r'
                        't' -> '\t'
                        else -> next
                    },
                )
                index += 2
            }
            '"' -> return out.toString() to index + 1
            else -> {
                out.append(ch)
                index += 1
            }
        }
    }
    return null
}

internal fun flagValue(number: Long?, text: String?): Boolean = when {
    number != null -> number != 0L
    text.equals("true", ignoreCase = true) || text == "t" || text == "1" -> true
    else -> false
}

internal fun bookmarksCatalog(
    folders: Collection<BookmarkFolderRecord>,
    bookmarks: Collection<BookmarkRecord>,
    status: BookmarksStatus,
) = BookmarksCatalog(
    folders = folders.map(::vaultFolder)
        .sortedWith(compareBy(BookmarkFolder::position, BookmarkFolder::name, BookmarkFolder::id)),
    bookmarks = bookmarks.map(::vaultBookmark).sortedByDescending { it.updatedAt },
    recentBookmarkIds = bookmarks.sortedByDescending { it.updatedAt }.take(20).map(BookmarkRecord::id),
    status = status,
)

internal fun vaultFolder(folder: BookmarkFolderRecord) = BookmarkFolder(
    id = folder.id,
    name = folder.name,
    parentId = folder.parentId,
    position = folder.position,
    revision = folder.revision,
)

internal fun vaultBookmark(bookmark: BookmarkRecord) = BookmarkItem(
    id = bookmark.id,
    folderId = bookmark.folderId,
    url = bookmark.url,
    title = bookmark.title,
    description = bookmark.description,
    tags = bookmark.tags,
    favorite = bookmark.favorite,
    archived = bookmark.archived,
    position = bookmark.position,
    revision = bookmark.revision,
    createdAt = bookmark.createdAt,
    updatedAt = bookmark.updatedAt,
)
