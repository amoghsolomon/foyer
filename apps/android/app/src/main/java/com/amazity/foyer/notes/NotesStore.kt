package com.amazity.foyer.notes

import android.content.Context
import com.amazity.foyer.BuildConfig
import com.amazity.foyer.model.NotesCatalog
import com.amazity.foyer.model.NotesStatus
import com.amazity.foyer.model.VaultFolder
import com.amazity.foyer.model.VaultNote
import com.powersync.PowerSyncDatabase
import com.powersync.db.getLongOptional
import com.powersync.db.getString
import com.powersync.db.getStringOptional
import java.time.Duration
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
import org.json.JSONObject

/**
 * Optimistic notes operations against the shared personal-data replica.
 * This store never connects or disconnects PowerSync on its own.
 */
class NotesStore(
    @Suppress("UNUSED_PARAMETER") context: Context,
    private val databaseProvider: suspend () -> PowerSyncDatabase,
) {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val _catalog = MutableStateFlow(NotesCatalog(emptyList(), emptyList(), emptyList()))
    private var watchJob: Job? = null
    private var statusJob: Job? = null
    private var startRequested = false
    @Volatile private var lastConflict: Pair<String, String>? = null
    @Volatile private var attachedDatabase: PowerSyncDatabase? = null

    val catalog: StateFlow<NotesCatalog> = _catalog

    @Suppress("UNUSED_PARAMETER")
    constructor(context: Context, api: NotesApi, databaseProvider: suspend () -> PowerSyncDatabase) : this(
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

    suspend fun ensureInbox(): VaultFolder {
        val existing = _catalog.value.childFolders(null).firstOrNull { it.name.equals("Inbox", true) }
            ?: _catalog.value.folders.firstOrNull()
        return existing ?: createFolder("Inbox")
    }

    suspend fun createFolder(name: String, parentId: String? = null): VaultFolder {
        val cleanName = name.trim().also { require(it.isNotEmpty()) { "Folder name is required" } }
        val id = UUID.randomUUID().toString()
        val now = Instant.now().toString()
        requireDatabase().execute(
            """INSERT INTO notes_folders
                (id, user_id, parent_id, name, position, revision, created_at, updated_at,
                 client_operation, operation_id, expected_revision, deleted_local)
                VALUES (?, '', ?, ?, 0, 1, ?, ?, 'create', ?, NULL, 0)""",
            listOf(id, parentId, cleanName, now, now, UUID.randomUUID().toString()),
        )
        return VaultFolder(id = id, name = cleanName, parentId = parentId, position = 0, revision = 1)
    }

    suspend fun renameFolder(folder: VaultFolder, name: String): VaultFolder {
        val cleanName = name.trim().also { require(it.isNotEmpty()) { "Folder name is required" } }
        val nextRevision = folder.revision + 1
        requireDatabase().execute(
            """UPDATE notes_folders
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

    suspend fun moveFolder(folder: VaultFolder, parentId: String?): VaultFolder {
        _catalog.value.validateFolderMove(folder, parentId)?.let { error(it) }
        val nextRevision = folder.revision + 1
        requireDatabase().execute(
            """UPDATE notes_folders
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

    suspend fun deleteFolder(folder: VaultFolder) {
        _catalog.value.validateFolderDelete(folder)?.let { error(it) }
        requireDatabase().execute(
            """UPDATE notes_folders
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

    suspend fun createNote(title: String, body: String, folderId: String): VaultNote {
        val cleanTitle = title.trim().also { require(it.isNotEmpty()) { "Note title is required" } }
        val id = UUID.randomUUID().toString()
        val now = Instant.now().toString()
        requireDatabase().execute(
            """INSERT INTO notes
                (id, user_id, folder_id, title, body, revision, created_at, updated_at,
                 client_operation, operation_id, expected_revision, deleted_local)
                VALUES (?, '', ?, ?, ?, 1, ?, ?, 'create', ?, NULL, 0)""",
            listOf(id, folderId, cleanTitle, body, now, now, UUID.randomUUID().toString()),
        )
        return vaultNote(
            NoteRecord(id, "", folderId, cleanTitle, body, 1, now, now, null),
        )
    }

    suspend fun updateNote(note: VaultNote, title: String, body: String, folderId: String): VaultNote {
        val cleanTitle = title.trim().also { require(it.isNotEmpty()) { "Note title is required" } }
        val db = requireDatabase()
        val now = Instant.now().toString()
        var revision = note.version
        val operationId = UUID.randomUUID().toString()
        val payload = JSONObject()
            .put("operationId", operationId)
            .put("title", cleanTitle)
            .put("body", body)
            .toString()
        db.execute(
            """UPDATE notes
                SET title = ?, body = ?, revision = ?, updated_at = ?, client_operation = 'update',
                    operation_id = ?, expected_revision = ?, client_payload = ?
                WHERE id = ?""",
            listOf(
                cleanTitle,
                body,
                ++revision,
                now,
                operationId,
                note.version,
                payload,
                note.id,
            ),
        )
        if (folderId != note.folderId) {
            db.execute(
                """UPDATE notes
                    SET folder_id = ?, revision = ?, updated_at = ?, client_operation = 'move',
                        operation_id = ?, expected_revision = ?
                    WHERE id = ?""",
                listOf(
                    folderId,
                    ++revision,
                    now,
                    UUID.randomUUID().toString(),
                    revision - 1,
                    note.id,
                ),
            )
        }
        return note.copy(
            title = cleanTitle,
            body = body,
            folderId = folderId,
            summary = summaryOf(body),
            version = revision,
            updatedAt = now,
            updatedLabel = "Updated just now",
        )
    }

    suspend fun deleteNote(note: VaultNote) {
        requireDatabase().execute(
            """UPDATE notes
                SET deleted_local = 1, revision = ?, updated_at = ?, client_operation = 'delete',
                    operation_id = ?, expected_revision = ?
                WHERE id = ?""",
            listOf(
                note.version + 1,
                Instant.now().toString(),
                UUID.randomUUID().toString(),
                note.version,
                note.id,
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
                FROM notes_folders WHERE COALESCE(deleted_local, 0) = 0""",
        ) { cursor ->
            FolderRecord(
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
        val notes = db.watch(
            """SELECT id, user_id, folder_id, title, body, revision, created_at, updated_at
                FROM notes WHERE COALESCE(deleted_local, 0) = 0""",
        ) { cursor ->
            NoteRecord(
                id = cursor.getString("id"),
                userId = cursor.getStringOptional("user_id").orEmpty(),
                folderId = cursor.getString("folder_id"),
                title = cursor.getString("title"),
                body = cursor.getString("body"),
                revision = cursor.getLongOptional("revision") ?: 1L,
                createdAt = cursor.getStringOptional("created_at").orEmpty(),
                updatedAt = cursor.getStringOptional("updated_at").orEmpty(),
                deletedAt = null,
            )
        }
        val pending = db.watch(
            """SELECT
                (SELECT COUNT(*) FROM notes_folders WHERE operation_id IS NOT NULL) +
                (SELECT COUNT(*) FROM notes WHERE operation_id IS NOT NULL) AS count""",
        ) { cursor -> cursor.getLongOptional("count")?.toInt() ?: 0 }
        watchJob = scope.launch {
            combine(folders, notes, pending) { folderRows, noteRows, pendingRows ->
                Triple(folderRows, noteRows, pendingRows.firstOrNull() ?: 0)
            }.collect { (folderRows, noteRows, pendingCount) ->
                val status = db.currentStatus
                _catalog.value = notesCatalog(
                    folders = folderRows,
                    notes = noteRows,
                    status = NotesStatus(
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
            api: NotesApi,
            databaseProvider: suspend () -> PowerSyncDatabase,
        ): NotesStore = NotesStore(context, api, databaseProvider)
    }
}

private fun notesCatalog(
    folders: Collection<FolderRecord>,
    notes: Collection<NoteRecord>,
    status: NotesStatus,
) = NotesCatalog(
    folders = folders.map(::vaultFolder)
        .sortedWith(compareBy(VaultFolder::position, VaultFolder::name, VaultFolder::id)),
    notes = notes.map(::vaultNote).sortedByDescending { it.updatedAt },
    recentNoteIds = notes.sortedByDescending { it.updatedAt }.take(20).map(NoteRecord::id),
    status = status,
)

internal fun vaultFolder(folder: FolderRecord) = VaultFolder(
    id = folder.id,
    name = folder.name,
    parentId = folder.parentId,
    position = folder.position,
    revision = folder.revision,
)

internal fun vaultNote(note: NoteRecord) = VaultNote(
    id = note.id,
    folderId = note.folderId,
    title = note.title,
    summary = summaryOf(note.body),
    updatedLabel = updatedLabel(note.updatedAt),
    body = note.body,
    version = note.revision,
    createdAt = note.createdAt,
    updatedAt = note.updatedAt,
)

internal fun summaryOf(body: String): String =
    body.lineSequence().map { it.trim() }
        .firstOrNull { it.isNotEmpty() && !it.startsWith("#") }
        ?.take(140)
        .orEmpty()

internal fun updatedLabel(value: String): String {
    val instant = runCatching { Instant.parse(value) }.getOrNull() ?: return "Updated locally"
    val age = Duration.between(instant, Instant.now()).coerceAtLeast(Duration.ZERO)
    return when {
        age.toMinutes() < 1 -> "Updated just now"
        age.toHours() < 1 -> "Updated ${age.toMinutes()}m ago"
        age.toDays() < 1 -> "Updated ${age.toHours()}h ago"
        else -> "Updated ${age.toDays()}d ago"
    }
}
