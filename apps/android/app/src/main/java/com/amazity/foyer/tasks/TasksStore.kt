package com.amazity.foyer.tasks

import android.content.Context
import com.amazity.foyer.BuildConfig
import com.amazity.foyer.model.TaskDue
import com.amazity.foyer.model.TasksCatalog
import com.amazity.foyer.model.TasksStatus
import com.amazity.foyer.model.VaultTask
import com.amazity.foyer.model.VaultTaskList
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
import org.json.JSONObject

/**
 * Optimistic task operations against the shared personal-data replica.
 * This store never connects or disconnects PowerSync on its own.
 */
class TasksStore(
    @Suppress("UNUSED_PARAMETER") context: Context,
    private val databaseProvider: suspend () -> PowerSyncDatabase,
) {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val _catalog = MutableStateFlow(TasksCatalog(emptyList(), emptyList()))
    private var watchJob: Job? = null
    private var statusJob: Job? = null
    private var startRequested = false
    @Volatile private var lastConflict: Pair<String, String>? = null
    @Volatile private var attachedDatabase: PowerSyncDatabase? = null

    val catalog: StateFlow<TasksCatalog> = _catalog
    val sharingReplica: Boolean = true

    @Suppress("UNUSED_PARAMETER")
    constructor(context: Context, api: TasksApi, databaseProvider: suspend () -> PowerSyncDatabase) : this(
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
                    sharingReplica = true,
                ),
            )
        }
    }

    suspend fun refreshFromServer() {
        publishStatus(requireDatabase())
    }

    suspend fun ensureInbox(): VaultTaskList {
        val existing = _catalog.value.lists.firstOrNull { it.name.equals("Inbox", true) }
            ?: _catalog.value.lists.firstOrNull()
        return existing ?: createList("Inbox")
    }

    suspend fun createList(name: String, position: Int? = null): VaultTaskList {
        val cleanName = name.trim().also { require(it.isNotEmpty()) { "List name is required" } }
        val id = UUID.randomUUID().toString()
        val now = Instant.now().toString()
        requireDatabase().execute(
            """INSERT INTO task_lists
                (id, user_id, name, position, href, etag, revision, created_at, updated_at,
                 client_operation, operation_id, expected_revision, deleted_local)
                VALUES (?, '', ?, ?, '', NULL, 1, ?, ?, 'create', ?, NULL, 0)""",
            listOf(id, cleanName, position ?: 0, now, now, UUID.randomUUID().toString()),
        )
        return VaultTaskList(id = id, name = cleanName, position = position ?: 0, revision = 1)
    }

    suspend fun renameList(list: VaultTaskList, name: String): VaultTaskList {
        val cleanName = name.trim().also { require(it.isNotEmpty()) { "List name is required" } }
        val nextRevision = list.revision + 1
        requireDatabase().execute(
            """UPDATE task_lists
                SET name = ?, revision = ?, updated_at = ?, client_operation = 'rename',
                    operation_id = ?, expected_revision = ?
                WHERE id = ?""",
            listOf(
                cleanName,
                nextRevision,
                Instant.now().toString(),
                UUID.randomUUID().toString(),
                list.revision,
                list.id,
            ),
        )
        return list.copy(name = cleanName, revision = nextRevision)
    }

    suspend fun deleteList(list: VaultTaskList) {
        _catalog.value.validateListDelete(list)?.let { error(it) }
        requireDatabase().execute(
            """UPDATE task_lists
                SET deleted_local = 1, revision = ?, updated_at = ?, client_operation = 'delete',
                    operation_id = ?, expected_revision = ?
                WHERE id = ?""",
            listOf(
                list.revision + 1,
                Instant.now().toString(),
                UUID.randomUUID().toString(),
                list.revision,
                list.id,
            ),
        )
    }

    suspend fun createTask(
        listId: String,
        title: String,
        description: String,
        due: TaskDue? = null,
        priority: Int = 0,
        position: Int? = null,
    ): VaultTask {
        val cleanTitle = title.trim().also { require(it.isNotEmpty()) { "Task title is required" } }
        val id = UUID.randomUUID().toString()
        val now = Instant.now().toString()
        val operationId = UUID.randomUUID().toString()
        val payload = taskPayload(operationId, cleanTitle, description, due, priority, position ?: 0)
        requireDatabase().execute(
            """INSERT INTO tasks
                (id, user_id, list_id, title, description, due_at, due_local, due_time_zone, due_all_day,
                 priority, completed, completed_at, position, href, etag, revision, created_at, updated_at,
                 client_operation, operation_id, expected_revision, deleted_local, client_payload)
                VALUES (?, '', ?, ?, ?, ?, ?, ?, ?, ?, 0, NULL, ?, '', '', 1, ?, ?, 'create', ?, NULL, 0, ?)""",
            listOf(
                id,
                listId,
                cleanTitle,
                description,
                due?.at,
                due?.local,
                due?.timeZone,
                if (due?.allDay == true) 1 else 0,
                priority,
                position ?: 0,
                now,
                now,
                operationId,
                payload,
            ),
        )
        return VaultTask(
            id = id,
            listId = listId,
            title = cleanTitle,
            description = description,
            due = due,
            priority = priority,
            position = position ?: 0,
            revision = 1,
            createdAt = now,
            updatedAt = now,
        )
    }

    suspend fun updateTask(
        task: VaultTask,
        title: String,
        description: String,
        due: TaskDue?,
        priority: Int,
        position: Int,
        listId: String = task.listId,
    ): VaultTask {
        val cleanTitle = title.trim().also { require(it.isNotEmpty()) { "Task title is required" } }
        val db = requireDatabase()
        val now = Instant.now().toString()
        var revision = task.revision
        val operationId = UUID.randomUUID().toString()
        val payload = taskPayload(operationId, cleanTitle, description, due, priority, position)
        db.execute(
            """UPDATE tasks
                SET title = ?, description = ?, due_at = ?, due_local = ?, due_time_zone = ?,
                    due_all_day = ?, priority = ?, position = ?, revision = ?, updated_at = ?,
                    client_operation = 'update', operation_id = ?, expected_revision = ?, client_payload = ?
                WHERE id = ?""",
            listOf(
                cleanTitle,
                description,
                due?.at,
                due?.local,
                due?.timeZone,
                if (due?.allDay == true) 1 else 0,
                priority,
                position,
                ++revision,
                now,
                operationId,
                task.revision,
                payload,
                task.id,
            ),
        )
        if (listId != task.listId) {
            db.execute(
                """UPDATE tasks
                    SET list_id = ?, revision = ?, updated_at = ?, client_operation = 'move',
                        operation_id = ?, expected_revision = ?
                    WHERE id = ?""",
                listOf(
                    listId,
                    ++revision,
                    now,
                    UUID.randomUUID().toString(),
                    revision - 1,
                    task.id,
                ),
            )
        }
        return task.copy(
            listId = listId,
            title = cleanTitle,
            description = description,
            due = due,
            priority = priority,
            position = position,
            revision = revision,
            updatedAt = now,
        )
    }

    suspend fun completeTask(task: VaultTask): VaultTask {
        if (task.completed) return task
        val now = Instant.now().toString()
        requireDatabase().execute(
            """UPDATE tasks
                SET completed = 1, completed_at = ?, revision = ?, updated_at = ?,
                    client_operation = 'complete', operation_id = ?, expected_revision = ?
                WHERE id = ?""",
            listOf(
                now,
                task.revision + 1,
                now,
                UUID.randomUUID().toString(),
                task.revision,
                task.id,
            ),
        )
        return task.copy(completed = true, completedAt = now, revision = task.revision + 1, updatedAt = now)
    }

    suspend fun reopenTask(task: VaultTask): VaultTask {
        if (!task.completed) return task
        val now = Instant.now().toString()
        requireDatabase().execute(
            """UPDATE tasks
                SET completed = 0, completed_at = NULL, revision = ?, updated_at = ?,
                    client_operation = 'reopen', operation_id = ?, expected_revision = ?
                WHERE id = ?""",
            listOf(
                task.revision + 1,
                now,
                UUID.randomUUID().toString(),
                task.revision,
                task.id,
            ),
        )
        return task.copy(completed = false, completedAt = null, revision = task.revision + 1, updatedAt = now)
    }

    suspend fun deleteTask(task: VaultTask) {
        requireDatabase().execute(
            """UPDATE tasks
                SET deleted_local = 1, revision = ?, updated_at = ?, client_operation = 'delete',
                    operation_id = ?, expected_revision = ?
                WHERE id = ?""",
            listOf(
                task.revision + 1,
                Instant.now().toString(),
                UUID.randomUUID().toString(),
                task.revision,
                task.id,
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
        val lists = db.watch(
            """SELECT id, user_id, name, position, href, etag, revision, created_at, updated_at
                FROM task_lists WHERE COALESCE(deleted_local, 0) = 0""",
        ) { cursor ->
            TaskListRecord(
                id = cursor.getString("id"),
                userId = cursor.getStringOptional("user_id").orEmpty(),
                name = cursor.getString("name"),
                position = cursor.getLongOptional("position")?.toInt() ?: 0,
                href = cursor.getStringOptional("href").orEmpty(),
                etag = cursor.getStringOptional("etag"),
                revision = cursor.getLongOptional("revision") ?: 1L,
                createdAt = cursor.getStringOptional("created_at").orEmpty(),
                updatedAt = cursor.getStringOptional("updated_at").orEmpty(),
                deletedAt = null,
            )
        }
        val tasks = db.watch(
            """SELECT id, user_id, list_id, title, description, due_at, due_local, due_time_zone,
                      due_all_day, priority, completed, completed_at, position, href, etag,
                      revision, created_at, updated_at
                FROM tasks WHERE COALESCE(deleted_local, 0) = 0""",
        ) { cursor ->
            TaskRecord(
                id = cursor.getString("id"),
                userId = cursor.getStringOptional("user_id").orEmpty(),
                listId = cursor.getString("list_id"),
                title = cursor.getString("title"),
                description = cursor.getStringOptional("description").orEmpty(),
                due = TaskDue.parse(
                    cursor.getStringOptional("due_local").orEmpty(),
                    cursor.getStringOptional("due_time_zone"),
                    cursor.getLongOptional("due_all_day")?.toInt() == 1,
                )?.copy(at = cursor.getStringOptional("due_at")),
                priority = cursor.getLongOptional("priority")?.toInt() ?: 0,
                completed = isTruthy(cursor.getLongOptional("completed"), cursor.getStringOptional("completed")),
                completedAt = cursor.getStringOptional("completed_at"),
                position = cursor.getLongOptional("position")?.toInt() ?: 0,
                href = cursor.getStringOptional("href").orEmpty(),
                etag = cursor.getStringOptional("etag").orEmpty(),
                revision = cursor.getLongOptional("revision") ?: 1L,
                createdAt = cursor.getStringOptional("created_at").orEmpty(),
                updatedAt = cursor.getStringOptional("updated_at").orEmpty(),
                deletedAt = null,
            )
        }
        val pending = db.watch(
            """SELECT
                (SELECT COUNT(*) FROM task_lists WHERE operation_id IS NOT NULL) +
                (SELECT COUNT(*) FROM tasks WHERE operation_id IS NOT NULL) AS count""",
        ) { cursor -> cursor.getLongOptional("count")?.toInt() ?: 0 }
        watchJob = scope.launch {
            combine(lists, tasks, pending) { listRows, taskRows, pendingRows ->
                Triple(listRows, taskRows, pendingRows.firstOrNull() ?: 0)
            }.collect { (listRows, taskRows, pendingCount) ->
                val status = db.currentStatus
                _catalog.value = tasksCatalog(
                    lists = listRows,
                    tasks = taskRows,
                    status = TasksStatus(
                        loading = false,
                        connected = status.connected,
                        offline = !status.connected,
                        pendingUploads = pendingCount,
                        lastError = status.anyError?.toString(),
                        conflictCode = lastConflict?.first,
                        conflictMessage = lastConflict?.second,
                        developmentAuth = BuildConfig.FOYER_DEVELOPMENT_AUTH,
                        usingPowerSync = true,
                        sharingReplica = true,
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
                    sharingReplica = true,
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
            api: TasksApi,
            databaseProvider: suspend () -> PowerSyncDatabase,
        ): TasksStore = TasksStore(context, api, databaseProvider)
    }
}

private fun taskPayload(
    operationId: String,
    title: String,
    description: String,
    due: TaskDue?,
    priority: Int,
    position: Int,
): String = JSONObject()
    .put("operationId", operationId)
    .put("title", title)
    .put("description", description)
    .put("due", due?.let {
        JSONObject()
            .put("local", it.local)
            .put("timeZone", it.timeZone ?: JSONObject.NULL)
            .put("allDay", it.allDay)
            .put("at", it.at ?: JSONObject.NULL)
    } ?: JSONObject.NULL)
    .put("priority", priority)
    .put("position", position)
    .toString()

private fun isTruthy(number: Long?, text: String?): Boolean =
    number == 1L || text.equals("true", true) || text.equals("t", true)

private fun tasksCatalog(
    lists: Collection<TaskListRecord>,
    tasks: Collection<TaskRecord>,
    status: TasksStatus,
) = TasksCatalog(
    lists = lists.map(::vaultTaskList)
        .sortedWith(compareBy(VaultTaskList::position, VaultTaskList::name, VaultTaskList::id)),
    tasks = tasks.map(::vaultTask),
    status = status,
)

internal fun vaultTaskList(list: TaskListRecord) = VaultTaskList(
    id = list.id,
    name = list.name,
    position = list.position,
    href = list.href,
    etag = list.etag,
    revision = list.revision,
)

internal fun vaultTask(task: TaskRecord) = VaultTask(
    id = task.id,
    listId = task.listId,
    title = task.title,
    description = task.description,
    due = task.due,
    priority = task.priority,
    completed = task.completed,
    completedAt = task.completedAt,
    position = task.position,
    href = task.href,
    etag = task.etag,
    revision = task.revision,
    createdAt = task.createdAt,
    updatedAt = task.updatedAt,
)
