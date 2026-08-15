package com.amazity.foyer.data

import android.content.Context
import com.amazity.foyer.auth.foyerApiClient
import com.amazity.foyer.contacts.ContactDraft
import com.amazity.foyer.foyerApplication
import com.amazity.foyer.model.AddressBook
import com.amazity.foyer.model.BookmarkFolder
import com.amazity.foyer.model.BookmarkItem
import com.amazity.foyer.model.BookmarksCatalog
import com.amazity.foyer.model.CalendarCatalog
import com.amazity.foyer.model.ConsolidatedProfile
import com.amazity.foyer.model.Contact
import com.amazity.foyer.model.ContactsCatalog
import com.amazity.foyer.model.EventDraft
import com.amazity.foyer.model.FoyerCalendar
import com.amazity.foyer.model.FoyerEvent
import com.amazity.foyer.model.MemoryPage
import com.amazity.foyer.model.MemoryRecord
import com.amazity.foyer.model.NotesCatalog
import com.amazity.foyer.model.TaskDue
import com.amazity.foyer.model.TasksCatalog
import com.amazity.foyer.model.VaultFolder
import com.amazity.foyer.model.VaultNote
import com.amazity.foyer.model.VaultTask
import com.amazity.foyer.model.VaultTaskList
import com.amazity.foyer.sync.SettingsSyncScheduler
import com.amazity.foyer.sync.SyncMetadata
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.delay

class FoyerRepository(context: Context) {
    private val appContext = context.applicationContext
    private val dao = FoyerDatabase.get(appContext).foyerDao()
    private val api = foyerApiClient(appContext)
    private val replica = appContext.foyerApplication.personalData
    private val settingsStore = UserSettingsStore(appContext)

    val messages: Flow<List<CachedMessage>> = dao.observeMessages()
    val activities: Flow<List<CachedActivity>> = dao.observeActivities()
    val notes: StateFlow<NotesCatalog> = replica.notes.catalog
    val hostedTasks: StateFlow<TasksCatalog> = replica.tasks.catalog
    val hostedCalendar: StateFlow<CalendarCatalog> = replica.calendar.catalog
    val contacts: StateFlow<ContactsCatalog> = replica.contacts.catalog
    val bookmarks: StateFlow<BookmarksCatalog> = replica.bookmarks.catalog
    val homeBriefing: Flow<CachedHomeBriefing?> = dao.observeHomeBriefing()
    val syncStatus: Flow<SyncStatusSnapshot> = combine(
        dao.observePendingMutationCount(),
        dao.observeSyncState(SyncMetadata.LAST_SUCCESS),
        dao.observeSyncState(SyncMetadata.LAST_ERROR),
    ) { pending, lastSuccess, lastError ->
        SyncStatusSnapshot(
            pendingMutations = pending,
            lastSuccessfulAt = lastSuccess,
            lastError = lastError?.takeIf(String::isNotBlank),
        )
    }

    suspend fun clearSessionData() {
        dao.taskItems().forEach { com.amazity.foyer.notifications.TaskReminderScheduler.cancel(appContext, it.id) }
        dao.clearSessionData()
    }

    fun currentTimezone(): String = settingsStore.timezone()

    fun queueTimezone(timezone: String) {
        settingsStore.queueTimezone(timezone)
        SettingsSyncScheduler.requestNow(appContext)
    }

    suspend fun syncPendingTimezone() {
        val pendingTimezone = settingsStore.pendingTimezone()
        val pendingWhitelist = settingsStore.pendingNotificationWhitelist()
        if (pendingTimezone == null && pendingWhitelist == null) return
        api.updateSettings(
            timezone = pendingTimezone ?: settingsStore.timezone().takeIf { pendingWhitelist != null },
            notificationWhitelist = pendingWhitelist?.toList()?.sorted(),
        )
        pendingTimezone?.let(settingsStore::markTimezoneSynced)
        pendingWhitelist?.let(settingsStore::markNotificationWhitelistSynced)
    }

    suspend fun refreshTimezone(): String {
        settingsStore.pendingTimezone()?.let { return it }
        val settings = api.settings().optJSONObject("settings") ?: return settingsStore.timezone()
        val timezone = settings.optString("timezone")
            .takeIf(UserSettingsStore::isValidTimezone) ?: settingsStore.timezone()
        if (settingsStore.pendingNotificationWhitelist() == null) {
            val whitelist = settings.optJSONArray("notificationWhitelist")?.let { array ->
                buildSet {
                    for (index in 0 until array.length()) {
                        array.optString(index).takeIf(String::isNotBlank)?.let(::add)
                    }
                }
            }.orEmpty()
            settingsStore.cacheNotificationWhitelist(whitelist)
        }
        settingsStore.cacheTimezone(timezone)
        return timezone
    }

    fun queueNotificationWhitelist(packages: Set<String>) {
        settingsStore.queueNotificationWhitelist(packages)
        SettingsSyncScheduler.requestNow(appContext)
    }

    suspend fun profile(): ConsolidatedProfile? = api.profile().optJSONObject("profile")?.let {
        ConsolidatedProfile(
            text = it.optString("text"),
            updatedAt = it.optString("updatedAt", it.optString("updated_at")),
        )
    }

    suspend fun memories(cursor: String? = null, limit: Int = 30): MemoryPage {
        val response = api.memories(limit, cursor)
        val array = response.optJSONArray("memories") ?: response.optJSONArray("items")
        val items = buildList {
            if (array != null) {
                for (index in 0 until array.length()) {
                    val item = array.optJSONObject(index) ?: continue
                    val id = item.optString("id").takeIf(String::isNotBlank) ?: continue
                    add(
                        MemoryRecord(
                            id = id,
                            kind = item.optString("kind", "fact"),
                            content = item.optString("content"),
                            createdAt = item.optString("created_at", item.optString("createdAt")),
                        ),
                    )
                }
            }
        }
        return MemoryPage(
            items = items,
            nextCursor = response.optString("nextCursor")
                .takeIf { it.isNotBlank() && it != "null" },
        )
    }

    suspend fun deleteMemory(memory: MemoryRecord) {
        api.deleteMemory(memory.id)
        dao.deleteMemory(memory.id)
    }

    suspend fun refreshHomeBriefing() {
        val response = api.homeBriefing(java.time.ZoneId.systemDefault().id)
        val briefing = response.optJSONObject("briefing") ?: return
        val insight = briefing.optJSONObject("insight")
        val target = insight?.optJSONObject("target")
        dao.upsertHomeBriefing(
            CachedHomeBriefing(
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

    fun startPersonalData() {
        replica.start()
    }

    suspend fun refreshNotes() {
        replica.start()
        replica.notes.refreshFromServer()
    }

    suspend fun refreshHostedPersonalData() {
        replica.start()
        replica.notes.refreshFromServer()
        replica.tasks.refreshFromServer()
        replica.bookmarks.refreshFromServer()
    }

    suspend fun createFolder(name: String, parentId: String? = null): VaultFolder =
        replica.notes.createFolder(name, parentId)

    suspend fun renameFolder(folder: VaultFolder, name: String): VaultFolder =
        replica.notes.renameFolder(folder, name)

    suspend fun moveFolder(folder: VaultFolder, parentId: String?): VaultFolder =
        replica.notes.moveFolder(folder, parentId)

    suspend fun deleteFolder(folder: VaultFolder) {
        replica.notes.deleteFolder(folder)
    }

    suspend fun refreshActivities() {
        dao.upsertActivitiesFrom(api.activities())
    }

    fun activityMessages(activityId: String): Flow<List<CachedActivityMessage>> =
        dao.observeActivityMessages(activityId)

    suspend fun refreshActivity(activityId: String): CachedActivity =
        dao.upsertActivityFrom(api.activity(activityId))

    suspend fun createNote(
        title: String,
        body: String,
        folderId: String,
        tags: List<String> = emptyList(),
    ): VaultNote {
        val targetFolder = folderId.ifBlank { replica.notes.ensureInbox().id }
        return replica.notes.createNote(title, body, targetFolder)
    }

    suspend fun updateNote(
        note: VaultNote,
        title: String,
        body: String,
        folderId: String,
    ): VaultNote = replica.notes.updateNote(note, title, body, folderId)

    suspend fun deleteNote(note: VaultNote) {
        replica.notes.deleteNote(note)
    }

    suspend fun createTaskList(name: String): VaultTaskList = replica.tasks.createList(name)

    suspend fun renameTaskList(list: VaultTaskList, name: String): VaultTaskList =
        replica.tasks.renameList(list, name)

    suspend fun deleteTaskList(list: VaultTaskList) {
        replica.tasks.deleteList(list)
    }

    suspend fun createHostedTask(
        title: String,
        description: String = "",
        listId: String? = null,
        due: TaskDue? = null,
        priority: Int = 0,
    ): VaultTask {
        val target = listId?.takeIf(String::isNotBlank) ?: replica.tasks.ensureInbox().id
        return replica.tasks.createTask(target, title, description, due, priority)
    }

    suspend fun updateHostedTask(
        task: VaultTask,
        title: String,
        description: String,
        due: TaskDue?,
        priority: Int,
        listId: String = task.listId,
    ): VaultTask = replica.tasks.updateTask(task, title, description, due, priority, task.position, listId)

    suspend fun completeHostedTask(task: VaultTask): VaultTask = replica.tasks.completeTask(task)

    suspend fun reopenHostedTask(task: VaultTask): VaultTask = replica.tasks.reopenTask(task)

    suspend fun deleteHostedTask(task: VaultTask) {
        replica.tasks.deleteTask(task)
    }

    suspend fun createCalendar(displayName: String, description: String = ""): FoyerCalendar =
        replica.calendar.createCalendar(displayName, description)

    suspend fun renameCalendar(calendar: FoyerCalendar, displayName: String): FoyerCalendar =
        replica.calendar.renameCalendar(calendar, displayName)

    suspend fun deleteCalendar(calendar: FoyerCalendar) {
        replica.calendar.deleteCalendar(calendar)
    }

    fun selectCalendar(calendarId: String?) {
        replica.calendar.selectCalendar(calendarId)
    }

    suspend fun createEvent(draft: EventDraft): FoyerEvent {
        val target = draft.calendarId.ifBlank {
            replica.calendar.catalog.value.calendars.firstOrNull()?.id
                ?: replica.calendar.createCalendar("Calendar").id
        }
        return replica.calendar.createEvent(draft.copy(calendarId = target))
    }

    suspend fun updateEvent(event: FoyerEvent, draft: EventDraft): FoyerEvent =
        replica.calendar.updateEvent(event, draft)

    suspend fun deleteEvent(event: FoyerEvent) {
        replica.calendar.deleteEvent(event)
    }

    suspend fun createAddressBook(displayName: String): AddressBook =
        replica.contacts.createAddressBook(displayName)

    suspend fun renameAddressBook(book: AddressBook, displayName: String): AddressBook =
        replica.contacts.renameAddressBook(book, displayName)

    suspend fun deleteAddressBook(book: AddressBook) {
        replica.contacts.deleteAddressBook(book)
    }

    suspend fun createContact(draft: ContactDraft): Contact {
        val target = draft.addressBookId.ifBlank { replica.contacts.ensureDefaultAddressBook().id }
        return replica.contacts.createContact(draft.copy(addressBookId = target))
    }

    suspend fun updateContact(contact: Contact, draft: ContactDraft): Contact =
        replica.contacts.updateContact(contact, draft)

    suspend fun deleteContact(contact: Contact) {
        replica.contacts.deleteContact(contact)
    }

    suspend fun createBookmarkFolder(name: String, parentId: String? = null): BookmarkFolder =
        replica.bookmarks.createFolder(name, parentId)

    suspend fun renameBookmarkFolder(folder: BookmarkFolder, name: String): BookmarkFolder =
        replica.bookmarks.renameFolder(folder, name)

    suspend fun moveBookmarkFolder(folder: BookmarkFolder, parentId: String?): BookmarkFolder =
        replica.bookmarks.moveFolder(folder, parentId)

    suspend fun deleteBookmarkFolder(folder: BookmarkFolder) {
        replica.bookmarks.deleteFolder(folder)
    }

    suspend fun createBookmark(
        folderId: String,
        url: String,
        title: String,
        description: String,
        tags: List<String>,
        favorite: Boolean = false,
    ): BookmarkItem {
        val target = folderId.ifBlank { replica.bookmarks.ensureInbox().id }
        return replica.bookmarks.createBookmark(target, url, title, description, tags, favorite)
    }

    suspend fun updateBookmark(
        bookmark: BookmarkItem,
        url: String,
        title: String,
        description: String,
        tags: List<String>,
        folderId: String = bookmark.folderId,
    ): BookmarkItem = replica.bookmarks.updateBookmark(bookmark, url, title, description, tags, folderId)

    suspend fun setBookmarkFavorite(bookmark: BookmarkItem, favorite: Boolean): BookmarkItem =
        replica.bookmarks.setFavorite(bookmark, favorite)

    suspend fun setBookmarkArchived(bookmark: BookmarkItem, archived: Boolean): BookmarkItem =
        replica.bookmarks.setArchived(bookmark, archived)

    suspend fun deleteBookmark(bookmark: BookmarkItem) {
        replica.bookmarks.deleteBookmark(bookmark)
    }

    suspend fun askAgent(message: String, background: Boolean = false) {
        if (message.isBlank()) return
        val response = api.createActivity(message.trim(), java.time.ZoneId.systemDefault().id)
        val activity = dao.upsertActivityFrom(response)
        pollActivity(activity.id)
    }

    suspend fun sendActivityMessage(activityId: String, message: String) {
        if (message.isBlank()) return
        dao.upsertActivityFrom(api.sendActivityMessage(activityId, message.trim()))
        pollActivity(activityId)
    }

    suspend fun scheduleActivity(
        activityId: String,
        runAt: String,
        frequency: String,
        interval: Int = 1,
        timezone: String = java.time.ZoneId.systemDefault().id,
    ) {
        dao.upsertActivityFrom(
            api.scheduleActivity(activityId, runAt, frequency, interval, timezone),
        )
    }

    suspend fun cancelActivitySchedule(activityId: String) {
        api.cancelActivitySchedule(activityId)
        refreshActivity(activityId)
    }

    suspend fun renameActivity(activityId: String, title: String) {
        require(title.isNotBlank()) { "Activity title is required" }
        dao.upsertActivityFrom(api.renameActivity(activityId, title.trim()))
    }

    suspend fun archiveActivity(activityId: String) {
        api.archiveActivity(activityId)
        dao.deleteActivityCache(activityId)
    }

    suspend fun deleteActivity(activityId: String) {
        api.deleteActivity(activityId)
        dao.deleteActivityCache(activityId)
    }

    suspend fun runActivityNow(activityId: String) {
        dao.upsertActivityFrom(api.runActivityNow(activityId))
        pollActivity(activityId)
    }

    suspend fun retryActivityRun(activityId: String, runId: String) {
        dao.upsertActivityFrom(api.retryActivityRun(activityId, runId))
        pollActivity(activityId)
    }

    private suspend fun pollActivity(activityId: String) {
        repeat(60) {
            delay(2_000)
            val activity = refreshActivity(activityId)
            if (activity.status != "queued" && activity.status != "running") return
        }
    }

    /** Hosted inbox task used by voice and omnibar reminders. */
    suspend fun createTask(title: String) {
        if (title.isBlank()) return
        createHostedTask(title.trim())
    }
}

data class SyncStatusSnapshot(
    val pendingMutations: Int = 0,
    val lastSuccessfulAt: String? = null,
    val lastError: String? = null,
)
