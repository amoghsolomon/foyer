package com.amazity.foyer.data

import com.amazity.foyer.model.ChatMessageState
import com.amazity.foyer.model.ChatRole
import com.amazity.foyer.model.TaskState
import org.junit.Assert.assertEquals
import org.junit.Test

class ActivityCacheTest {
    @Test
    fun mapsRecurringActivityToScheduledTask() {
        val cached = CachedActivity(
            id = "activity_1",
            kind = "job",
            title = "Daily summary",
            status = "scheduled",
            summary = "Latest summary",
            latestResult = null,
            scheduleFrequency = "daily",
            scheduleInterval = 2,
            scheduleTimezone = "Asia/Kolkata",
            nextRunAt = "2026-07-17T10:00:00.000Z",
            scheduleEnabled = true,
            definitionVersion = 2,
            jobObjective = "Summarize the project",
            jobInstructions = "Only include the main branch",
            expectedOutput = "A short Markdown summary",
            latestFailedRunId = null,
            createdAt = "2026-07-15T10:00:00.000Z",
            updatedAt = "2026-07-15T10:00:00.000Z",
        )

        val task = agentTask(cached)
        assertEquals(TaskState.Scheduled, task.state)
        assertEquals("daily", task.scheduleFrequency)
        assertEquals(2, task.scheduleInterval)
        assertEquals("2026-07-17T10:00:00.000Z", task.nextRunAt)
        assertEquals("job", task.kind)
        assertEquals("Summarize the project", task.jobObjective)
    }

    @Test
    fun mapsFailedSystemMessageWithoutPretendingItWasAnAssistantReply() {
        val cached = CachedActivityMessage(
            id = "message_1",
            activityId = "activity_1",
            role = "system",
            content = "This run failed",
            state = "failed",
            runId = null,
            createdAt = "2026-07-15T10:00:00.000Z",
        )

        val message = chatMessage(cached)
        assertEquals(ChatRole.System, message.role)
        assertEquals(ChatMessageState.Failed, message.state)
    }
}
