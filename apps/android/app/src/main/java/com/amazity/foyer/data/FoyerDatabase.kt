package com.amazity.foyer.data

import android.content.Context
import androidx.room.Dao
import androidx.room.Database
import androidx.room.Entity
import androidx.room.Insert
import androidx.room.OnConflictStrategy
import androidx.room.PrimaryKey
import androidx.room.Query
import androidx.room.Room
import androidx.room.RoomDatabase
import androidx.room.Transaction
import androidx.room.migration.Migration
import androidx.sqlite.db.SupportSQLiteDatabase
import kotlinx.coroutines.flow.Flow

@Entity(tableName = "cached_calendar_items")
data class CachedCalendarItem(
    @PrimaryKey val id: String,
    val seriesId: String,
    val title: String,
    val description: String?,
    val startsAt: String,
    val endsAt: String,
    val seriesStartsAt: String,
    val seriesEndsAt: String,
    val allDay: Boolean,
    val timezone: String,
    val recurrenceJson: String?,
    val syncStatus: String,
    val version: Long,
    val updatedAt: String,
)

@Entity(tableName = "cached_task_items")
data class CachedTaskItem(
    @PrimaryKey val id: String,
    val title: String,
    val notes: String?,
    val dueAt: String?,
    val completedAt: String?,
    val syncStatus: String,
    val version: Long,
    val updatedAt: String,
)

@Entity(tableName = "cached_memories")
data class CachedMemory(
    @PrimaryKey val id: String,
    val kind: String,
    val content: String,
    val importance: Double,
    val confidence: Double,
    val updatedAt: String,
)

@Entity(tableName = "cached_link_previews")
data class CachedLinkPreview(
    @PrimaryKey val url: String,
    val title: String?,
    val description: String?,
    val imageBytes: ByteArray?,
    val failed: Boolean,
    val fetchedAt: Long,
)

@Entity(tableName = "cached_messages")
data class CachedMessage(
    @PrimaryKey val id: String,
    val role: String,
    val content: String,
    val state: String,
    val createdAt: String,
)

@Entity(tableName = "cached_activities")
data class CachedActivity(
    @PrimaryKey val id: String,
    val kind: String,
    val title: String,
    val status: String,
    val summary: String,
    val latestResult: String?,
    val scheduleFrequency: String?,
    val scheduleInterval: Int?,
    val scheduleTimezone: String?,
    val nextRunAt: String?,
    val scheduleEnabled: Boolean,
    val definitionVersion: Int?,
    val jobObjective: String?,
    val jobInstructions: String?,
    val expectedOutput: String?,
    val latestFailedRunId: String?,
    val createdAt: String,
    val updatedAt: String,
)

@Entity(tableName = "cached_activity_messages")
data class CachedActivityMessage(
    @PrimaryKey val id: String,
    val activityId: String,
    val role: String,
    val content: String,
    val state: String,
    val runId: String?,
    val createdAt: String,
)

@Entity(tableName = "cached_note_folders")
data class CachedNoteFolder(
    @PrimaryKey val id: String,
    val name: String,
    val position: Int,
    val updatedAt: String,
)

@Entity(tableName = "cached_notes")
data class CachedNote(
    @PrimaryKey val id: String,
    val folderId: String,
    val title: String,
    val body: String,
    val summary: String,
    val tagsJson: String,
    val linkedFromJson: String,
    val version: Long,
    val createdAt: String,
    val updatedAt: String,
)

@Entity(tableName = "cached_home_briefing")
data class CachedHomeBriefing(
    @PrimaryKey val id: String = "current",
    val dailyMessage: String,
    val insightMessage: String?,
    val targetType: String?,
    val targetId: String?,
    val targetLabel: String?,
    val generatedAt: String,
    val expiresAt: String,
)

@Entity(tableName = "pending_mutations")
data class PendingMutation(
    @PrimaryKey val mutationId: String,
    val deviceId: String,
    val entityType: String,
    val entityId: String?,
    val operation: String,
    val payloadJson: String?,
    val createdAt: Long = System.currentTimeMillis(),
    val attempts: Int = 0,
)

@Entity(tableName = "pending_notifications")
data class PendingNotification(
    @PrimaryKey val id: String,
    val appPackage: String,
    val title: String?,
    val body: String,
    val postedAt: String,
    val redacted: Boolean,
    val createdAt: Long = System.currentTimeMillis(),
    val attempts: Int = 0,
)

@Entity(tableName = "sync_state")
data class SyncState(
    @PrimaryKey val key: String,
    val value: String,
)

@Dao
interface FoyerDao {
    @Query("SELECT * FROM cached_calendar_items ORDER BY startsAt")
    fun observeCalendar(): Flow<List<CachedCalendarItem>>

    @Query("SELECT * FROM cached_task_items ORDER BY completedAt IS NOT NULL, dueAt")
    fun observeTasks(): Flow<List<CachedTaskItem>>

    @Query("SELECT * FROM cached_task_items")
    suspend fun taskItems(): List<CachedTaskItem>

    @Query("SELECT * FROM cached_task_items WHERE id = :id")
    suspend fun taskItem(id: String): CachedTaskItem?

    @Query("SELECT * FROM cached_memories ORDER BY importance DESC, updatedAt DESC")
    fun observeMemories(): Flow<List<CachedMemory>>

    @Query("SELECT * FROM cached_link_previews WHERE url = :url")
    suspend fun linkPreview(url: String): CachedLinkPreview?

    @Query("SELECT * FROM cached_messages ORDER BY createdAt")
    fun observeMessages(): Flow<List<CachedMessage>>

    @Query("SELECT * FROM cached_activities ORDER BY updatedAt DESC, id DESC")
    fun observeActivities(): Flow<List<CachedActivity>>

    @Query("SELECT * FROM cached_activity_messages WHERE activityId = :activityId ORDER BY createdAt, id")
    fun observeActivityMessages(activityId: String): Flow<List<CachedActivityMessage>>

    @Query("SELECT * FROM cached_note_folders ORDER BY position, name COLLATE NOCASE")
    fun observeNoteFolders(): Flow<List<CachedNoteFolder>>

    @Query("SELECT * FROM cached_notes ORDER BY updatedAt DESC, id DESC")
    fun observeNotes(): Flow<List<CachedNote>>

    @Query("SELECT * FROM cached_home_briefing WHERE id = 'current'")
    fun observeHomeBriefing(): Flow<CachedHomeBriefing?>

    @Insert(onConflict = OnConflictStrategy.REPLACE)
    suspend fun upsertCalendar(item: CachedCalendarItem)

    @Insert(onConflict = OnConflictStrategy.REPLACE)
    suspend fun upsertTask(item: CachedTaskItem)

    @Insert(onConflict = OnConflictStrategy.REPLACE)
    suspend fun upsertCalendarItems(items: List<CachedCalendarItem>)

    @Insert(onConflict = OnConflictStrategy.REPLACE)
    suspend fun upsertTaskItems(items: List<CachedTaskItem>)

    @Query("DELETE FROM cached_calendar_items")
    suspend fun clearCalendar()

    @Query("DELETE FROM cached_task_items")
    suspend fun clearTasks()

    @Query("DELETE FROM cached_memories")
    suspend fun clearMemories()

    @Query("DELETE FROM cached_link_previews")
    suspend fun clearLinkPreviews()

    @Query("DELETE FROM cached_messages")
    suspend fun clearMessages()

    @Query("DELETE FROM cached_activity_messages")
    suspend fun clearActivityMessages()

    @Query("DELETE FROM cached_home_briefing")
    suspend fun clearHomeBriefing()

    @Query("DELETE FROM pending_mutations")
    suspend fun clearPendingMutations()

    @Query("DELETE FROM pending_notifications")
    suspend fun clearPendingNotifications()

    @Query("DELETE FROM sync_state")
    suspend fun clearSyncState()

    @Transaction
    suspend fun replaceAgenda(calendar: List<CachedCalendarItem>, tasks: List<CachedTaskItem>) {
        clearCalendar()
        clearTasks()
        if (calendar.isNotEmpty()) upsertCalendarItems(calendar)
        if (tasks.isNotEmpty()) upsertTaskItems(tasks)
    }

    @Transaction
    suspend fun clearSessionData() {
        clearCalendar()
        clearTasks()
        clearMemories()
        clearMessages()
        clearActivityMessages()
        clearActivities()
        clearNotes()
        clearNoteFolders()
        clearHomeBriefing()
        clearPendingMutations()
        clearPendingNotifications()
        clearSyncState()
    }

    @Insert(onConflict = OnConflictStrategy.REPLACE)
    suspend fun upsertMemory(item: CachedMemory)

    @Insert(onConflict = OnConflictStrategy.REPLACE)
    suspend fun upsertLinkPreview(item: CachedLinkPreview)

    @Insert(onConflict = OnConflictStrategy.REPLACE)
    suspend fun upsertMessage(item: CachedMessage)

    @Insert(onConflict = OnConflictStrategy.REPLACE)
    suspend fun upsertActivity(item: CachedActivity)

    @Insert(onConflict = OnConflictStrategy.REPLACE)
    suspend fun upsertActivities(items: List<CachedActivity>)

    @Query("DELETE FROM cached_activities")
    suspend fun clearActivities()

    @Query("DELETE FROM cached_activity_messages WHERE activityId NOT IN (SELECT id FROM cached_activities)")
    suspend fun clearOrphanedActivityMessages()

    @Transaction
    suspend fun replaceActivityList(items: List<CachedActivity>) {
        clearActivities()
        if (items.isNotEmpty()) upsertActivities(items)
        clearOrphanedActivityMessages()
    }

    @Insert(onConflict = OnConflictStrategy.REPLACE)
    suspend fun upsertActivityMessages(items: List<CachedActivityMessage>)

    @Insert(onConflict = OnConflictStrategy.REPLACE)
    suspend fun upsertNoteFolder(item: CachedNoteFolder)

    @Insert(onConflict = OnConflictStrategy.REPLACE)
    suspend fun upsertNoteFolders(items: List<CachedNoteFolder>)

    @Insert(onConflict = OnConflictStrategy.REPLACE)
    suspend fun upsertNote(item: CachedNote)

    @Insert(onConflict = OnConflictStrategy.REPLACE)
    suspend fun upsertNotes(items: List<CachedNote>)

    @Insert(onConflict = OnConflictStrategy.REPLACE)
    suspend fun upsertHomeBriefing(item: CachedHomeBriefing)

    @Insert(onConflict = OnConflictStrategy.REPLACE)
    suspend fun enqueueMutation(mutation: PendingMutation)

    @Insert(onConflict = OnConflictStrategy.IGNORE)
    suspend fun enqueueNotification(notification: PendingNotification)

    @Query("SELECT * FROM pending_notifications ORDER BY createdAt LIMIT :limit")
    suspend fun pendingNotifications(limit: Int = 50): List<PendingNotification>

    @Query("SELECT COUNT(*) FROM pending_notifications")
    suspend fun pendingNotificationCount(): Int

    @Query("DELETE FROM pending_notifications WHERE id IN (:ids)")
    suspend fun deleteNotifications(ids: List<String>)

    @Query("UPDATE pending_notifications SET attempts = attempts + 1 WHERE id IN (:ids)")
    suspend fun incrementNotificationAttempts(ids: List<String>)

    @Query("SELECT * FROM pending_mutations ORDER BY createdAt LIMIT :limit")
    suspend fun pendingMutations(limit: Int = 100): List<PendingMutation>

    @Query("SELECT COUNT(*) FROM pending_mutations")
    fun observePendingMutationCount(): Flow<Int>

    @Query("DELETE FROM pending_mutations WHERE mutationId IN (:ids)")
    suspend fun deleteMutations(ids: List<String>)

    @Query("UPDATE pending_mutations SET attempts = attempts + 1 WHERE mutationId IN (:ids)")
    suspend fun incrementMutationAttempts(ids: List<String>)

    @Insert(onConflict = OnConflictStrategy.REPLACE)
    suspend fun putSyncState(state: SyncState)

    @Query("SELECT value FROM sync_state WHERE `key` = :key")
    suspend fun syncState(key: String): String?

    @Query("SELECT value FROM sync_state WHERE `key` = :key")
    fun observeSyncState(key: String): Flow<String?>

    @Query("DELETE FROM cached_calendar_items WHERE id = :id")
    suspend fun deleteCalendar(id: String)

    @Query("DELETE FROM cached_calendar_items WHERE seriesId = :seriesId")
    suspend fun deleteCalendarSeries(seriesId: String)

    @Query("DELETE FROM cached_task_items WHERE id = :id")
    suspend fun deleteTask(id: String)

    @Query("DELETE FROM cached_memories WHERE id = :id")
    suspend fun deleteMemory(id: String)

    @Query("DELETE FROM cached_activities WHERE id = :id")
    suspend fun deleteActivity(id: String)

    @Query("DELETE FROM cached_activity_messages WHERE activityId = :activityId")
    suspend fun deleteActivityMessages(activityId: String)

    @Transaction
    suspend fun deleteActivityCache(activityId: String) {
        deleteActivityMessages(activityId)
        deleteActivity(activityId)
    }

    @Query("DELETE FROM cached_notes WHERE id = :id")
    suspend fun deleteNote(id: String)

    @Query("DELETE FROM cached_notes")
    suspend fun clearNotes()

    @Query("DELETE FROM cached_note_folders")
    suspend fun clearNoteFolders()

    @Transaction
    suspend fun replaceNoteCache(folders: List<CachedNoteFolder>, notes: List<CachedNote>) {
        clearNotes()
        clearNoteFolders()
        if (folders.isNotEmpty()) upsertNoteFolders(folders)
        if (notes.isNotEmpty()) upsertNotes(notes)
    }

    @Transaction
    suspend fun replaceActivityMessages(activityId: String, messages: List<CachedActivityMessage>) {
        deleteActivityMessages(activityId)
        if (messages.isNotEmpty()) upsertActivityMessages(messages)
    }
}

@Database(
    entities = [
        CachedCalendarItem::class,
        CachedTaskItem::class,
        CachedMemory::class,
        CachedLinkPreview::class,
        CachedMessage::class,
        CachedActivity::class,
        CachedActivityMessage::class,
        PendingMutation::class,
        PendingNotification::class,
        SyncState::class,
        CachedNoteFolder::class,
        CachedNote::class,
        CachedHomeBriefing::class,
    ],
    version = 9,
    exportSchema = true,
)
abstract class FoyerDatabase : RoomDatabase() {
    abstract fun foyerDao(): FoyerDao

    companion object {
        @Volatile private var instance: FoyerDatabase? = null

        fun get(context: Context): FoyerDatabase = instance ?: synchronized(this) {
            instance ?: Room.databaseBuilder(
                context.applicationContext,
                FoyerDatabase::class.java,
                "foyer.db",
            ).addMigrations(
                MIGRATION_1_2,
                MIGRATION_2_3,
                MIGRATION_3_4,
                MIGRATION_4_5,
                MIGRATION_5_6,
                MIGRATION_6_7,
                MIGRATION_7_8,
                MIGRATION_8_9,
            ).build().also { instance = it }
        }

        private val MIGRATION_1_2 = object : Migration(1, 2) {
            override fun migrate(db: SupportSQLiteDatabase) {
                db.execSQL(
                    """CREATE TABLE IF NOT EXISTS `cached_note_folders` (
                        `id` TEXT NOT NULL,
                        `name` TEXT NOT NULL,
                        `position` INTEGER NOT NULL,
                        `updatedAt` TEXT NOT NULL,
                        PRIMARY KEY(`id`)
                    )""".trimIndent(),
                )
                db.execSQL(
                    """CREATE TABLE IF NOT EXISTS `cached_notes` (
                        `id` TEXT NOT NULL,
                        `folderId` TEXT NOT NULL,
                        `title` TEXT NOT NULL,
                        `body` TEXT NOT NULL,
                        `summary` TEXT NOT NULL,
                        `tagsJson` TEXT NOT NULL,
                        `linkedFromJson` TEXT NOT NULL,
                        `version` INTEGER NOT NULL,
                        `createdAt` TEXT NOT NULL,
                        `updatedAt` TEXT NOT NULL,
                        PRIMARY KEY(`id`)
                    )""".trimIndent(),
                )
            }
        }


        private val MIGRATION_2_3 = object : Migration(2, 3) {
            override fun migrate(db: SupportSQLiteDatabase) {
                db.execSQL(
                    """CREATE TABLE IF NOT EXISTS `cached_activities` (
                        `id` TEXT NOT NULL,
                        `title` TEXT NOT NULL,
                        `status` TEXT NOT NULL,
                        `summary` TEXT NOT NULL,
                        `latestResult` TEXT,
                        `scheduleFrequency` TEXT,
                        `scheduleInterval` INTEGER,
                        `scheduleTimezone` TEXT,
                        `nextRunAt` TEXT,
                        `scheduleEnabled` INTEGER NOT NULL,
                        `createdAt` TEXT NOT NULL,
                        `updatedAt` TEXT NOT NULL,
                        PRIMARY KEY(`id`)
                    )""".trimIndent(),
                )
                db.execSQL(
                    """CREATE TABLE IF NOT EXISTS `cached_activity_messages` (
                        `id` TEXT NOT NULL,
                        `activityId` TEXT NOT NULL,
                        `role` TEXT NOT NULL,
                        `content` TEXT NOT NULL,
                        `state` TEXT NOT NULL,
                        `runId` TEXT,
                        `createdAt` TEXT NOT NULL,
                        PRIMARY KEY(`id`)
                    )""".trimIndent(),
                )
            }
        }

        private val MIGRATION_3_4 = object : Migration(3, 4) {
            override fun migrate(db: SupportSQLiteDatabase) {
                db.execSQL("ALTER TABLE `cached_activities` ADD COLUMN `kind` TEXT NOT NULL DEFAULT 'conversation'")
                db.execSQL("ALTER TABLE `cached_activities` ADD COLUMN `definitionVersion` INTEGER")
                db.execSQL("ALTER TABLE `cached_activities` ADD COLUMN `jobObjective` TEXT")
                db.execSQL("ALTER TABLE `cached_activities` ADD COLUMN `jobInstructions` TEXT")
                db.execSQL("ALTER TABLE `cached_activities` ADD COLUMN `expectedOutput` TEXT")
                db.execSQL("ALTER TABLE `cached_activities` ADD COLUMN `latestFailedRunId` TEXT")
            }
        }

        private val MIGRATION_4_5 = object : Migration(4, 5) {
            override fun migrate(db: SupportSQLiteDatabase) {
                db.execSQL(
                    """CREATE TABLE IF NOT EXISTS `cached_home_briefing` (
                        `id` TEXT NOT NULL,
                        `dailyMessage` TEXT NOT NULL,
                        `insightMessage` TEXT,
                        `targetType` TEXT,
                        `targetId` TEXT,
                        `targetLabel` TEXT,
                        `generatedAt` TEXT NOT NULL,
                        `expiresAt` TEXT NOT NULL,
                        PRIMARY KEY(`id`)
                    )""".trimIndent(),
                )
            }
        }

        private val MIGRATION_5_6 = object : Migration(5, 6) {
            override fun migrate(db: SupportSQLiteDatabase) {
                db.execSQL("ALTER TABLE `cached_calendar_items` ADD COLUMN `seriesId` TEXT NOT NULL DEFAULT ''")
                db.execSQL("ALTER TABLE `cached_calendar_items` ADD COLUMN `seriesStartsAt` TEXT NOT NULL DEFAULT ''")
                db.execSQL("ALTER TABLE `cached_calendar_items` ADD COLUMN `seriesEndsAt` TEXT NOT NULL DEFAULT ''")
                db.execSQL("ALTER TABLE `cached_calendar_items` ADD COLUMN `timezone` TEXT NOT NULL DEFAULT 'UTC'")
                db.execSQL("ALTER TABLE `cached_calendar_items` ADD COLUMN `recurrenceJson` TEXT")
                db.execSQL("ALTER TABLE `cached_calendar_items` ADD COLUMN `version` INTEGER NOT NULL DEFAULT 1")
                db.execSQL("UPDATE `cached_calendar_items` SET `seriesId` = `id`, `seriesStartsAt` = `startsAt`, `seriesEndsAt` = `endsAt`")
                db.execSQL("ALTER TABLE `cached_task_items` ADD COLUMN `version` INTEGER NOT NULL DEFAULT 1")
            }
        }

        private val MIGRATION_6_7 = object : Migration(6, 7) {
            override fun migrate(db: SupportSQLiteDatabase) {
                db.execSQL(
                    """CREATE TABLE `cached_calendar_items_new` (
                        `id` TEXT NOT NULL,
                        `seriesId` TEXT NOT NULL,
                        `title` TEXT NOT NULL,
                        `description` TEXT,
                        `startsAt` TEXT NOT NULL,
                        `endsAt` TEXT NOT NULL,
                        `seriesStartsAt` TEXT NOT NULL,
                        `seriesEndsAt` TEXT NOT NULL,
                        `allDay` INTEGER NOT NULL,
                        `timezone` TEXT NOT NULL,
                        `recurrenceJson` TEXT,
                        `syncStatus` TEXT NOT NULL,
                        `version` INTEGER NOT NULL,
                        `updatedAt` TEXT NOT NULL,
                        PRIMARY KEY(`id`)
                    )""".trimIndent(),
                )
                db.execSQL(
                    """INSERT INTO `cached_calendar_items_new`
                        (`id`, `seriesId`, `title`, `description`, `startsAt`, `endsAt`,
                         `seriesStartsAt`, `seriesEndsAt`, `allDay`, `timezone`,
                         `recurrenceJson`, `syncStatus`, `version`, `updatedAt`)
                       SELECT `id`, `seriesId`, `title`, `description`, `startsAt`, `endsAt`,
                              `seriesStartsAt`, `seriesEndsAt`, `allDay`, `timezone`,
                              `recurrenceJson`, `syncStatus`, `version`, `updatedAt`
                       FROM `cached_calendar_items`""".trimIndent(),
                )
                db.execSQL("DROP TABLE `cached_calendar_items`")
                db.execSQL("ALTER TABLE `cached_calendar_items_new` RENAME TO `cached_calendar_items`")

                db.execSQL(
                    """CREATE TABLE `cached_task_items_new` (
                        `id` TEXT NOT NULL,
                        `title` TEXT NOT NULL,
                        `notes` TEXT,
                        `dueAt` TEXT,
                        `completedAt` TEXT,
                        `syncStatus` TEXT NOT NULL,
                        `version` INTEGER NOT NULL,
                        `updatedAt` TEXT NOT NULL,
                        PRIMARY KEY(`id`)
                    )""".trimIndent(),
                )
                db.execSQL(
                    """INSERT INTO `cached_task_items_new`
                        (`id`, `title`, `notes`, `dueAt`, `completedAt`, `syncStatus`, `version`, `updatedAt`)
                       SELECT `id`, `title`, `notes`, `dueAt`, `completedAt`, `syncStatus`, `version`, `updatedAt`
                       FROM `cached_task_items`""".trimIndent(),
                )
                db.execSQL("DROP TABLE `cached_task_items`")
                db.execSQL("ALTER TABLE `cached_task_items_new` RENAME TO `cached_task_items`")
            }
        }

        private val MIGRATION_7_8 = object : Migration(7, 8) {
            override fun migrate(db: SupportSQLiteDatabase) {
                db.execSQL(
                    """CREATE TABLE IF NOT EXISTS `cached_link_previews` (
                        `url` TEXT NOT NULL,
                        `title` TEXT,
                        `description` TEXT,
                        `imageBytes` BLOB,
                        `failed` INTEGER NOT NULL,
                        `fetchedAt` INTEGER NOT NULL,
                        PRIMARY KEY(`url`)
                    )""".trimIndent(),
                )
            }
        }

        private val MIGRATION_8_9 = object : Migration(8, 9) {
            override fun migrate(db: SupportSQLiteDatabase) {
                db.execSQL(
                    """CREATE TABLE IF NOT EXISTS `pending_notifications` (
                        `id` TEXT NOT NULL,
                        `appPackage` TEXT NOT NULL,
                        `title` TEXT,
                        `body` TEXT NOT NULL,
                        `postedAt` TEXT NOT NULL,
                        `redacted` INTEGER NOT NULL,
                        `createdAt` INTEGER NOT NULL,
                        `attempts` INTEGER NOT NULL,
                        PRIMARY KEY(`id`)
                    )""".trimIndent(),
                )
            }
        }
    }
}
