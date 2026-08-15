package com.amazity.foyer.model

enum class FoyerDestination {
    Onboarding,
    Home,
    ActivityChat,
    NoteEditor,
    NoteDetail,
    TaskList,
    TaskDetail,
    TaskEditor,
    EventDetail,
    EventEditor,
    ContactDetail,
    ContactEditor,
    BookmarkFolder,
    BookmarkDetail,
    BookmarkEditor,
    SearchResults,
    SyncStatus,
    MemoryProfile,
    NotificationContext,
    Settings,
}

enum class HomePanel(val label: String) {
    Apps("Apps"),
    Activity("Activity"),
    Calendar("Calendar"),
    Tasks("Tasks"),
    Notes("Notes"),
    Contacts("Contacts"),
    Bookmarks("Bookmarks"),
}

enum class AgendaDay(val label: String) {
    Today("Today"),
    Tomorrow("Tomorrow"),
    Upcoming("Upcoming"),
}

data class AgendaItem(
    val day: AgendaDay,
    val time: String,
    val title: String,
    val detail: String? = null,
    val id: String = title,
    val seriesId: String = id,
    val startsAtEpochMillis: Long? = null,
    val endsAtEpochMillis: Long? = null,
    val seriesStartsAt: String? = null,
    val seriesEndsAt: String? = null,
    val allDay: Boolean = false,
    val timezone: String = "UTC",
    val recurrenceJson: String? = null,
    val version: Long = 1,
)

data class TodoItem(
    val title: String,
    val completed: Boolean = false,
    val id: String = title,
    val description: String? = null,
    val dueAt: String? = null,
    val version: Long = 1,
)

enum class MomentTarget {
    Activity,
    Calendar,
    Task,
}

data class MomentInsight(
    val message: String,
    val linkedText: String,
    val target: MomentTarget,
    val targetId: String,
)

enum class TaskState {
    Running,
    Queued,
    Scheduled,
    Done,
    Failed,
}

data class AgentTask(
    val title: String,
    val subtitle: String,
    val state: TaskState,
    val result: String? = null,
    val id: String = title,
    val nextRunAt: String? = null,
    val scheduleFrequency: String? = null,
    val scheduleInterval: Int? = null,
    val scheduleTimezone: String? = null,
    val kind: String = "conversation",
    val definitionVersion: Int? = null,
    val jobObjective: String? = null,
    val jobInstructions: String? = null,
    val expectedOutput: String? = null,
    val latestFailedRunId: String? = null,
)

data class LauncherApp(
    val name: String,
    val packageName: String = "",
    val activityName: String = "",
    val emphasized: Boolean = false,
) {
    val stableKey: String
        get() = if (packageName.isBlank() || activityName.isBlank()) {
            name
        } else {
            "$packageName/$activityName"
        }
}

data class FoyerUiState(
    val dailyMessage: String,
    val moment: MomentInsight?,
    val agendaItems: List<AgendaItem>,
    val todoItems: List<TodoItem>,
    val tasks: List<AgentTask>,
    val apps: List<LauncherApp>,
)
