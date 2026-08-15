package com.amazity.foyer.notifications

import android.app.AlarmManager
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import com.amazity.foyer.data.CachedTaskItem
import com.amazity.foyer.data.FoyerDatabase
import java.time.LocalDate
import java.time.LocalTime
import java.time.ZoneId
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch

object TaskReminderScheduler {
    private val reminderTime = LocalTime.of(9, 0)

    fun schedule(context: Context, task: CachedTaskItem) {
        cancel(context, task.id)
        if (task.completedAt != null) return
        val dueDate = task.dueAt?.take(10)?.let { runCatching { LocalDate.parse(it) }.getOrNull() }
            ?: return
        val triggerAt = dueDate.atTime(reminderTime).atZone(ZoneId.systemDefault()).toInstant()
        if (!triggerAt.isAfter(java.time.Instant.now())) return
        context.getSystemService(AlarmManager::class.java).setAndAllowWhileIdle(
            AlarmManager.RTC_WAKEUP,
            triggerAt.toEpochMilli(),
            reminderIntent(context, task.id, task.title),
        )
    }

    fun cancel(context: Context, taskId: String) {
        context.getSystemService(AlarmManager::class.java).cancel(
            reminderIntent(context, taskId, null),
        )
    }

    suspend fun rescheduleAll(context: Context) {
        FoyerDatabase.get(context).foyerDao().taskItems().forEach { schedule(context, it) }
    }

    private fun reminderIntent(context: Context, taskId: String, title: String?): PendingIntent {
        val intent = Intent(context, TaskReminderReceiver::class.java)
            .setAction(ACTION_TASK_DUE)
            .putExtra(EXTRA_TASK_ID, taskId)
        if (title != null) intent.putExtra(EXTRA_TASK_TITLE, title)
        return PendingIntent.getBroadcast(
            context,
            taskId.hashCode(),
            intent,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
    }

    internal const val ACTION_TASK_DUE = "com.amazity.foyer.action.TASK_DUE"
    internal const val EXTRA_TASK_ID = "taskId"
    internal const val EXTRA_TASK_TITLE = "taskTitle"
}

class TaskReminderReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action != TaskReminderScheduler.ACTION_TASK_DUE) return
        val taskId = intent.getStringExtra(TaskReminderScheduler.EXTRA_TASK_ID) ?: return
        val pending = goAsync()
        CoroutineScope(Dispatchers.IO).launch {
            try {
                val task = FoyerDatabase.get(context).foyerDao().taskItem(taskId)
                if (task != null && task.completedAt == null && task.dueAt != null) {
                    FoyerNotifications.show(
                        context = context,
                        title = "Task due today",
                        body = task.title,
                        messageId = "task-reminder:$taskId",
                        targetType = "task",
                        targetId = taskId,
                    )
                }
            } finally {
                pending.finish()
            }
        }
    }
}

class TaskReminderRescheduleReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        when (intent.action) {
            Intent.ACTION_BOOT_COMPLETED,
            Intent.ACTION_MY_PACKAGE_REPLACED,
            Intent.ACTION_TIME_CHANGED,
            Intent.ACTION_TIMEZONE_CHANGED,
            -> Unit
            else -> return
        }
        val pending = goAsync()
        CoroutineScope(Dispatchers.IO).launch {
            try {
                TaskReminderScheduler.rescheduleAll(context.applicationContext)
            } finally {
                pending.finish()
            }
        }
    }
}
