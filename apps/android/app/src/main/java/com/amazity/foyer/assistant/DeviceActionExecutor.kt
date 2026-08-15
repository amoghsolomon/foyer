package com.amazity.foyer.assistant

import android.content.ActivityNotFoundException
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.provider.AlarmClock
import android.provider.Settings
import com.amazity.foyer.MainActivity
import com.amazity.foyer.data.FoyerRepository
import com.amazity.foyer.launcher.LauncherAppsRepository
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

class DeviceActionExecutor(
    context: Context,
    private val appCommands: AppCommandBus,
) {
    private val appContext = context.applicationContext
    private val launcherRepository = LauncherAppsRepository(appContext)
    private val foyerRepository = FoyerRepository(appContext)

    suspend fun execute(action: ClientAction): ActionExecutionResult = withContext(Dispatchers.IO) {
        runCatching {
            when (action.type) {
                ClientActionType.OpenApp -> openApp(action)
                ClientActionType.DialNumber -> dial(action)
                ClientActionType.ComposeSms -> composeSms(action)
                ClientActionType.ComposeEmail -> composeEmail(action)
                ClientActionType.SetAlarm -> setAlarm(action)
                ClientActionType.OpenSettings -> openSettings(action)
                ClientActionType.CreateNote -> createNote(action)
                ClientActionType.CreateReminder -> createReminder(action)
                ClientActionType.StartCloudTask -> startCloudTask(action)
            }
        }.getOrElse { error ->
            ActionExecutionResult(
                successful = false,
                message = when (error) {
                    is ActivityNotFoundException -> "No installed app can handle that action"
                    else -> error.message ?: "The device action failed"
                },
            )
        }
    }

    private fun openApp(action: ClientAction): ActionExecutionResult {
        val requestedName = action.requireArgument("app")
        val apps = launcherRepository.loadApps()
        val app = apps.firstOrNull { it.name.equals(requestedName, ignoreCase = true) }
            ?: apps.singleOrNull { it.name.contains(requestedName, ignoreCase = true) }
            ?: error("I couldn't find an installed app named $requestedName")
        launcherRepository.launch(app).getOrThrow()
        return launched("Opened ${app.name}")
    }

    private fun dial(action: ClientAction): ActionExecutionResult {
        val number = sanitizedPhoneNumber(action.requireArgument("phone_number"))
        startActivity(Intent(Intent.ACTION_DIAL, Uri.fromParts("tel", number, null)))
        return launched()
    }

    private fun composeSms(action: ClientAction): ActionExecutionResult {
        val number = action.argument("phone_number")?.let(::sanitizedPhoneNumber).orEmpty()
        val intent = Intent(Intent.ACTION_SENDTO, Uri.fromParts("smsto", number, null)).apply {
            action.argument("body")?.let { putExtra("sms_body", it.take(MAX_BODY_LENGTH)) }
        }
        startActivity(intent)
        return launched()
    }

    private fun composeEmail(action: ClientAction): ActionExecutionResult {
        val recipient = Uri.encode(action.argument("to").orEmpty())
        val uri = Uri.parse("mailto:$recipient")
            .buildUpon()
            .apply {
                action.argument("subject")?.let { appendQueryParameter("subject", it.take(MAX_TITLE_LENGTH)) }
                action.argument("body")?.let { appendQueryParameter("body", it.take(MAX_BODY_LENGTH)) }
            }
            .build()
        startActivity(Intent(Intent.ACTION_SENDTO, uri))
        return launched()
    }

    private fun setAlarm(action: ClientAction): ActionExecutionResult {
        val hour = action.requireArgument("hour").toIntOrNull()
            ?.takeIf { it in 0..23 } ?: error("Alarm hour must be between 0 and 23")
        val minute = action.requireArgument("minute").toIntOrNull()
            ?.takeIf { it in 0..59 } ?: error("Alarm minute must be between 0 and 59")
        val intent = Intent(AlarmClock.ACTION_SET_ALARM).apply {
            putExtra(AlarmClock.EXTRA_HOUR, hour)
            putExtra(AlarmClock.EXTRA_MINUTES, minute)
            putExtra(AlarmClock.EXTRA_SKIP_UI, false)
            action.argument("label")?.let { putExtra(AlarmClock.EXTRA_MESSAGE, it.take(MAX_TITLE_LENGTH)) }
        }
        startActivity(intent)
        return launched()
    }

    private fun openSettings(action: ClientAction): ActionExecutionResult {
        val intent = when (action.argument("page")?.lowercase()) {
            "wifi" -> Intent(Settings.ACTION_WIFI_SETTINGS)
            "bluetooth" -> Intent(Settings.ACTION_BLUETOOTH_SETTINGS)
            "assistant" -> Intent(Settings.ACTION_VOICE_INPUT_SETTINGS)
            else -> Intent(
                Settings.ACTION_APPLICATION_DETAILS_SETTINGS,
                Uri.fromParts("package", appContext.packageName, null),
            )
        }
        startActivity(intent)
        return launched()
    }

    private fun createNote(action: ClientAction): ActionExecutionResult {
        val title = action.argument("title")?.take(MAX_TITLE_LENGTH) ?: "Voice note"
        val body = action.requireArgument("body").take(MAX_BODY_LENGTH)
        check(appCommands.send(AppCommand.CreateNote(title, body))) { "Foyer couldn't queue the note" }
        startActivity(
            Intent(appContext, MainActivity::class.java).apply {
                this.action = ACTION_SHOW_FOYER
                addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP)
            },
        )
        return launched("Created $title")
    }

    private suspend fun createReminder(action: ClientAction): ActionExecutionResult {
        foyerRepository.createTask(action.requireArgument("title").take(MAX_TITLE_LENGTH))
        return ActionExecutionResult(true, "Reminder created", dismissAssistant = true)
    }

    private suspend fun startCloudTask(action: ClientAction): ActionExecutionResult {
        foyerRepository.askAgent(
            action.requireArgument("prompt").take(MAX_BODY_LENGTH),
            background = true,
        )
        return ActionExecutionResult(true, "Started in Activity", dismissAssistant = true)
    }

    private fun startActivity(intent: Intent) {
        appContext.startActivity(intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
    }

    private fun launched(message: String? = null) = ActionExecutionResult(
        successful = true,
        message = message,
        dismissAssistant = true,
    )

    private fun ClientAction.requireArgument(name: String): String =
        argument(name)?.take(MAX_BODY_LENGTH) ?: error("Missing $name")

    private fun sanitizedPhoneNumber(value: String): String {
        val sanitized = value.filter { it.isDigit() || it == '+' || it == '*' || it == '#' }
        require(sanitized.isNotBlank() && sanitized.length <= 32) { "Invalid phone number" }
        return sanitized
    }

    private companion object {
        const val ACTION_SHOW_FOYER = "com.amazity.foyer.action.SHOW_FOYER"
        const val MAX_TITLE_LENGTH = 200
        const val MAX_BODY_LENGTH = 8_000
    }
}
