package com.amazity.foyer.model

/** Read models for the server-owned task projection and its PowerSync replica. */
data class VaultTaskList(
    val id: String,
    val name: String,
    val position: Int = 0,
    val href: String = "",
    val etag: String? = null,
    val revision: Long = 1,
)

data class TaskDue(
    val local: String,
    val timeZone: String? = null,
    val allDay: Boolean = false,
    val at: String? = null,
) {
    fun displayLabel(): String {
        val zone = timeZone?.takeIf(String::isNotBlank)
        return when {
            allDay -> local
            zone.isNullOrBlank() -> local.replace('T', ' ')
            else -> "${local.replace('T', ' ')} $zone"
        }
    }

    companion object {
        private val dateOnly = Regex("^\\d{4}-\\d{2}-\\d{2}$")
        private val localDateTime = Regex("^\\d{4}-\\d{2}-\\d{2}T\\d{2}:\\d{2}:\\d{2}$")

        fun dateOnly(date: String): TaskDue = TaskDue(local = date, allDay = true)

        fun timed(local: String, timeZone: String? = null): TaskDue =
            TaskDue(local = local, timeZone = timeZone?.takeIf(String::isNotBlank), allDay = false)

        fun parse(local: String, timeZone: String?, allDay: Boolean): TaskDue? {
            val clean = local.trim()
            if (clean.isEmpty()) return null
            val zone = timeZone?.trim()?.takeIf(String::isNotBlank)
            if (allDay) {
                if (!dateOnly.matches(clean)) return null
                return TaskDue(local = clean, timeZone = zone, allDay = true)
            }
            if (!localDateTime.matches(clean)) return null
            if (zone != null && zone != "UTC" && !zone.contains('/')) return null
            return TaskDue(local = clean, timeZone = zone, allDay = false)
        }
    }
}

data class VaultTask(
    val id: String,
    val listId: String,
    val title: String,
    val description: String,
    val due: TaskDue? = null,
    val priority: Int = 0,
    val completed: Boolean = false,
    val completedAt: String? = null,
    val position: Int = 0,
    val href: String = "",
    val etag: String = "",
    val revision: Long = 1,
    val createdAt: String = "",
    val updatedAt: String = "",
) {
    val summary: String
        get() = taskSummary(description)

    val priorityLabel: String
        get() = when (priority) {
            0 -> "None"
            in 1..3 -> "High"
            in 4..6 -> "Medium"
            else -> "Low"
        }
}

data class TasksStatus(
    val loading: Boolean = true,
    val connected: Boolean = false,
    val offline: Boolean = false,
    val pendingUploads: Int = 0,
    val lastError: String? = null,
    val conflictCode: String? = null,
    val conflictMessage: String? = null,
    val developmentAuth: Boolean = false,
    val usingPowerSync: Boolean = false,
    val sharingReplica: Boolean = false,
) {
    fun banner(): TasksSyncBanner? = tasksSyncBanner(this)
}

sealed class TasksSyncBanner {
    data class Offline(val pendingUploads: Int) : TasksSyncBanner()
    data class Pending(val pendingUploads: Int) : TasksSyncBanner()
    data class StaleRevision(val message: String) : TasksSyncBanner()
    data class Error(val message: String) : TasksSyncBanner()
}

fun tasksSyncBanner(status: TasksStatus): TasksSyncBanner? {
    val conflict = status.conflictMessage?.takeIf { it.isNotBlank() }
    if (conflict != null) {
        return if (status.conflictCode == "stale_revision" || conflict.contains("stale revision", ignoreCase = true)) {
            TasksSyncBanner.StaleRevision(conflict)
        } else {
            TasksSyncBanner.Error(conflict)
        }
    }
    status.lastError?.takeIf { it.isNotBlank() }?.let { return TasksSyncBanner.Error(it) }
    if (status.offline) return TasksSyncBanner.Offline(status.pendingUploads)
    if (status.pendingUploads > 0) return TasksSyncBanner.Pending(status.pendingUploads)
    return null
}

data class TasksCatalog(
    val lists: List<VaultTaskList>,
    val tasks: List<VaultTask>,
    val status: TasksStatus = TasksStatus(loading = false),
) {
    fun list(listId: String): VaultTaskList? = lists.firstOrNull { it.id == listId }

    fun task(taskId: String): VaultTask? = tasks.firstOrNull { it.id == taskId }

    fun tasksIn(listId: String): List<VaultTask> =
        tasks.filter { it.listId == listId }.sortedWith(taskOrder)

    fun openTasks(): List<VaultTask> = tasks.filterNot(VaultTask::completed).sortedWith(taskOrder)

    fun completedTasks(): List<VaultTask> = tasks.filter(VaultTask::completed).sortedWith(taskOrder)

    fun openTasksIn(listId: String): List<VaultTask> = tasksIn(listId).filterNot(VaultTask::completed)

    fun completedTasksIn(listId: String): List<VaultTask> = tasksIn(listId).filter(VaultTask::completed)

    fun listIsEmpty(listId: String): Boolean = tasksIn(listId).isEmpty()

    fun validateListDelete(list: VaultTaskList): String? =
        if (listIsEmpty(list.id)) {
            null
        } else {
            "List is not empty. Move or delete its tasks first."
        }

    fun validMoveTargets(@Suppress("UNUSED_PARAMETER") task: VaultTask): List<VaultTaskList> =
        lists.sortedWith(compareBy(VaultTaskList::position, VaultTaskList::name, VaultTaskList::id))

    fun recentOpenTasks(limit: Int = 20): List<VaultTask> = openTasks().take(limit)
}

private val taskOrder = compareBy<VaultTask>(
    { it.completed },
    VaultTask::position,
    { it.due?.local.orEmpty() },
    VaultTask::title,
    VaultTask::id,
)

fun taskSummary(description: String): String =
    description.lineSequence().map { it.trim() }
        .firstOrNull { it.isNotEmpty() && !it.startsWith("#") }
        ?.take(140)
        .orEmpty()
