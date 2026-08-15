package com.amazity.foyer.assistant

enum class AssistantPhase {
    Ready,
    Preparing,
    Listening,
    Sending,
    AwaitingConfirmation,
    Executing,
    Complete,
    Error,
}

enum class AssistantMessageRole {
    User,
    Assistant,
}

data class AssistantMessage(
    val id: Long,
    val role: AssistantMessageRole,
    val text: String,
)

enum class ClientActionType(val wireName: String) {
    OpenApp("open_app"),
    DialNumber("dial_number"),
    ComposeSms("compose_sms"),
    ComposeEmail("compose_email"),
    SetAlarm("set_alarm"),
    OpenSettings("open_settings"),
    CreateNote("create_note"),
    CreateReminder("create_reminder"),
    StartCloudTask("start_cloud_task");

    companion object {
        fun fromWireName(value: String): ClientActionType? = entries.firstOrNull {
            it.wireName == value
        }
    }
}

data class ClientAction(
    val type: ClientActionType,
    val arguments: Map<String, String>,
) {
    val requiresConfirmation: Boolean
        get() = type == ClientActionType.CreateNote || type == ClientActionType.CreateReminder

    fun summary(): String = when (type) {
        ClientActionType.OpenApp -> "Open ${argument("app") ?: "an app"}"
        ClientActionType.DialNumber -> "Open the dialer for ${argument("phone_number") ?: "this number"}"
        ClientActionType.ComposeSms -> "Prepare a text message"
        ClientActionType.ComposeEmail -> "Prepare an email"
        ClientActionType.SetAlarm -> "Open a new alarm"
        ClientActionType.OpenSettings -> "Open device settings"
        ClientActionType.CreateNote -> "Create a note named ${argument("title") ?: "Voice note"}"
        ClientActionType.CreateReminder -> "Create reminder: ${argument("title") ?: "Untitled reminder"}"
        ClientActionType.StartCloudTask -> "Start this as a background agent task"
    }

    fun argument(name: String): String? = arguments[name]?.trim()?.takeIf(String::isNotEmpty)
}

data class AssistantTurn(
    val text: String,
    val action: ClientAction?,
)

data class AssistantUiState(
    val visible: Boolean = false,
    val input: String = "",
    val phase: AssistantPhase = AssistantPhase.Ready,
    val messages: List<AssistantMessage> = emptyList(),
    val levels: List<Float> = emptyList(),
    val elapsedMillis: Long = 0,
    val preparationProgress: Float = 0f,
    val pendingAction: ClientAction? = null,
    val errorMessage: String? = null,
)

data class ActionExecutionResult(
    val successful: Boolean,
    val message: String? = null,
    val dismissAssistant: Boolean = false,
)
