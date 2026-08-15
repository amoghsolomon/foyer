package com.amazity.foyer.tasks

import com.amazity.foyer.model.TaskDue
import com.amazity.foyer.model.TasksCatalog
import com.amazity.foyer.model.TasksStatus
import com.amazity.foyer.model.TasksSyncBanner
import com.amazity.foyer.model.VaultTask
import com.amazity.foyer.model.VaultTaskList
import com.amazity.foyer.model.taskSummary
import com.amazity.foyer.model.tasksSyncBanner
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class TasksCatalogTest {
    @Test
    fun `lists and tasks keep list membership after a move`() {
        val catalog = sampleCatalog().copy(
            tasks = listOf(sampleTask(listId = "later", position = 0)),
        )
        assertTrue(catalog.tasksIn("inbox").isEmpty())
        assertEquals("Write ADR", catalog.tasksIn("later").single().title)
    }

    @Test
    fun `open tasks sort by position then due then title`() {
        val catalog = TasksCatalog(
            lists = listOf(VaultTaskList("inbox", "Inbox")),
            tasks = listOf(
                sampleTask(id = "c", title = "C", position = 1, due = TaskDue.dateOnly("2026-08-20")),
                sampleTask(id = "a", title = "A", position = 0, due = TaskDue.dateOnly("2026-08-19")),
                sampleTask(id = "b", title = "B", position = 0, due = TaskDue.dateOnly("2026-08-18")),
                sampleTask(id = "done", title = "Done", completed = true, position = 0),
            ),
        )
        assertEquals(listOf("b", "a", "c"), catalog.openTasksIn("inbox").map(VaultTask::id))
        assertEquals(listOf("done"), catalog.completedTasksIn("inbox").map(VaultTask::id))
    }

    @Test
    fun `list delete is rejected while tasks remain`() {
        val catalog = sampleCatalog()
        assertEquals(
            "List is not empty. Move or delete its tasks first.",
            catalog.validateListDelete(catalog.list("inbox")!!),
        )
        assertNull(catalog.validateListDelete(catalog.list("later")!!))
    }

    @Test
    fun `sync banner prefers stale revision over offline and pending`() {
        assertEquals(
            TasksSyncBanner.StaleRevision("The expected revision does not match the current revision."),
            tasksSyncBanner(
                TasksStatus(
                    loading = false,
                    offline = true,
                    pendingUploads = 2,
                    conflictCode = "stale_revision",
                    conflictMessage = "The expected revision does not match the current revision.",
                    lastError = "download failed",
                ),
            ),
        )
        assertEquals(
            TasksSyncBanner.Offline(3),
            tasksSyncBanner(TasksStatus(loading = false, offline = true, pendingUploads = 3)),
        )
        assertEquals(
            TasksSyncBanner.Pending(1),
            tasksSyncBanner(TasksStatus(loading = false, connected = true, pendingUploads = 1)),
        )
        assertTrue(
            tasksSyncBanner(TasksStatus(loading = false, connected = true, lastError = "upload failed"))
                is TasksSyncBanner.Error,
        )
        assertNull(tasksSyncBanner(TasksStatus(loading = false, connected = true)))
    }

    @Test
    fun `summary prefers the first non-heading markdown line`() {
        assertEquals(
            "Keep this sentence",
            taskSummary("# Title\n\nKeep this sentence\n\nMore"),
        )
    }

    @Test
    fun `schema fragments use the notes replica table names`() {
        assertEquals("task_lists", TASK_LISTS_TABLE)
        assertEquals("tasks", TASKS_TABLE)
        assertEquals("foyer-personal-powersync.db", SHARED_REPLICA_FILENAME)
        assertEquals(2, taskTables().size)
    }

    private fun sampleCatalog() = TasksCatalog(
        lists = listOf(VaultTaskList("inbox", "Inbox"), VaultTaskList("later", "Later", position = 1)),
        tasks = listOf(sampleTask()),
    )

    private fun sampleTask(
        id: String = "t1",
        listId: String = "inbox",
        title: String = "Write ADR",
        completed: Boolean = false,
        position: Int = 0,
        due: TaskDue? = TaskDue.dateOnly("2026-08-15"),
    ) = VaultTask(
        id = id,
        listId = listId,
        title = title,
        description = "# Heading\n\nBody",
        due = due,
        completed = completed,
        position = position,
    )
}

class TaskDueTest {
    @Test
    fun `all-day dates keep calendar day identity`() {
        val due = TaskDue.parse("2026-08-15", "America/Chicago", true)
        assertEquals("2026-08-15", due?.local)
        assertTrue(due?.allDay == true)
        assertEquals("2026-08-15", due?.displayLabel())
        assertNull(TaskDue.parse("08/15/2026", null, true))
    }

    @Test
    fun `timed values keep floating and named time zones`() {
        val floating = TaskDue.parse("2026-08-15T18:00:00", null, false)
        assertEquals("2026-08-15T18:00:00", floating?.local)
        assertNull(floating?.timeZone)
        assertEquals("2026-08-15 18:00:00", floating?.displayLabel())

        val zoned = TaskDue.parse("2026-08-15T18:00:00", "America/New_York", false)
        assertEquals("America/New_York", zoned?.timeZone)
        assertTrue(zoned?.displayLabel()?.contains("America/New_York") == true)
        assertNull(TaskDue.parse("2026-08-15T18:00:00", "not-a-zone", false))
    }
}
