package com.amazity.foyer.assistant

import android.Manifest
import android.content.Context
import android.content.pm.PackageManager
import androidx.core.content.ContextCompat
import com.amazity.foyer.auth.foyerApiClient
import com.amazity.foyer.voice.MoonshineVoiceInput
import com.amazity.foyer.voice.VoiceInputState
import java.util.concurrent.atomic.AtomicLong
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

class AssistantSessionController(
    context: Context,
    appCommands: AppCommandBus,
) {
    private val appContext = context.applicationContext
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private val voiceInput = MoonshineVoiceInput(appContext)
    private val api = foyerApiClient(appContext)
    private val actionExecutor = DeviceActionExecutor(appContext, appCommands)
    private val messageIds = AtomicLong(0)
    private val _state = MutableStateFlow(AssistantUiState())
    val state: StateFlow<AssistantUiState> = _state.asStateFlow()

    private var transcriptBaseline = ""
    private var acceptingTranscript = false

    init {
        scope.launch {
            voiceInput.state.collectLatest(::handleVoiceState)
        }
    }

    fun show() {
        _state.update { current ->
            if (current.visible) current else AssistantUiState(visible = true)
        }
    }

    fun showAndStartListening() {
        show()
        startListening()
    }

    fun startListening() {
        show()
        if (!hasMicrophonePermission()) {
            _state.update {
                it.copy(
                    phase = AssistantPhase.Error,
                    errorMessage = "Microphone permission is required",
                )
            }
            return
        }
        val current = _state.value
        if (current.phase == AssistantPhase.Sending || current.phase == AssistantPhase.Executing) return
        transcriptBaseline = current.input.trimEnd()
        acceptingTranscript = true
        _state.update { it.copy(errorMessage = null, phase = AssistantPhase.Preparing) }
        voiceInput.start { transcript ->
            if (!acceptingTranscript) return@start
            val combined = listOf(transcriptBaseline, transcript.trim())
                .filter(String::isNotBlank)
                .joinToString(" ")
            _state.update { it.copy(input = combined) }
        }
    }

    fun stopListening() {
        acceptingTranscript = false
        voiceInput.stop()
        _state.update { current ->
            if (current.visible && (
                    current.phase == AssistantPhase.Listening ||
                        current.phase == AssistantPhase.Preparing
                )
            ) {
                current.copy(phase = AssistantPhase.Ready, levels = emptyList())
            } else {
                current
            }
        }
    }

    fun toggleListening() {
        if (_state.value.phase == AssistantPhase.Listening ||
            _state.value.phase == AssistantPhase.Preparing
        ) {
            stopListening()
        } else {
            startListening()
        }
    }

    fun editInput(value: String) {
        if (acceptingTranscript) stopListening()
        _state.update {
            it.copy(
                input = value.take(MAX_INPUT_LENGTH),
                phase = if (it.phase == AssistantPhase.Error || it.phase == AssistantPhase.Complete) {
                    AssistantPhase.Ready
                } else {
                    it.phase
                },
                errorMessage = null,
            )
        }
    }

    fun submit() {
        val message = _state.value.input.trim()
        if (message.isBlank()) return
        if (_state.value.phase == AssistantPhase.Sending || _state.value.phase == AssistantPhase.Executing) return
        stopListening()
        _state.update {
            it.copy(
                input = "",
                phase = AssistantPhase.Sending,
                errorMessage = null,
                pendingAction = null,
                messages = it.messages + AssistantMessage(
                    id = messageIds.incrementAndGet(),
                    role = AssistantMessageRole.User,
                    text = message,
                ),
            )
        }
        scope.launch {
            runCatching { AssistantProtocol.parse(api.assistantTurn(message)) }
                .onSuccess(::handleTurn)
                .onFailure { error ->
                    _state.update {
                        it.copy(
                            phase = AssistantPhase.Error,
                            errorMessage = error.message ?: "The agent could not respond",
                        )
                    }
                }
        }
    }

    fun confirmPendingAction() {
        val action = _state.value.pendingAction ?: return
        scope.launch { execute(action) }
    }

    fun cancelPendingAction() {
        _state.update {
            it.copy(
                phase = AssistantPhase.Ready,
                pendingAction = null,
                messages = it.messages + AssistantMessage(
                    id = messageIds.incrementAndGet(),
                    role = AssistantMessageRole.Assistant,
                    text = "Cancelled.",
                ),
            )
        }
    }

    fun microphonePermissionDenied() {
        show()
        _state.update {
            it.copy(
                phase = AssistantPhase.Error,
                errorMessage = "Allow microphone access to dictate, or type your request instead",
            )
        }
    }

    fun dismiss() {
        stopListening()
        _state.value = AssistantUiState()
    }

    private fun handleTurn(turn: AssistantTurn) {
        _state.update { current ->
            current.copy(
                messages = if (turn.text.isBlank()) {
                    current.messages
                } else {
                    current.messages + AssistantMessage(
                        id = messageIds.incrementAndGet(),
                        role = AssistantMessageRole.Assistant,
                        text = turn.text,
                    )
                },
                phase = when {
                    turn.action?.requiresConfirmation == true -> AssistantPhase.AwaitingConfirmation
                    turn.action != null -> AssistantPhase.Executing
                    else -> AssistantPhase.Complete
                },
                pendingAction = turn.action?.takeIf(ClientAction::requiresConfirmation),
            )
        }
        val action = turn.action ?: return
        if (!action.requiresConfirmation) scope.launch { execute(action) }
    }

    private suspend fun execute(action: ClientAction) {
        _state.update {
            it.copy(
                phase = AssistantPhase.Executing,
                pendingAction = null,
                errorMessage = null,
            )
        }
        val result = actionExecutor.execute(action)
        if (!result.successful) {
            _state.update {
                it.copy(
                    phase = AssistantPhase.Error,
                    errorMessage = result.message ?: "The action failed",
                )
            }
            return
        }
        result.message?.let { message ->
            _state.update {
                it.copy(
                    messages = it.messages + AssistantMessage(
                        id = messageIds.incrementAndGet(),
                        role = AssistantMessageRole.Assistant,
                        text = message,
                    ),
                )
            }
        }
        _state.update { it.copy(phase = AssistantPhase.Complete) }
        if (result.dismissAssistant) {
            delay(350)
            dismiss()
        }
    }

    private fun handleVoiceState(voiceState: VoiceInputState) {
        if (!_state.value.visible) return
        when (voiceState) {
            VoiceInputState.Idle -> if (
                _state.value.phase == AssistantPhase.Listening ||
                _state.value.phase == AssistantPhase.Preparing
            ) {
                _state.update { it.copy(phase = AssistantPhase.Ready, levels = emptyList()) }
            }
            is VoiceInputState.Preparing -> _state.update {
                it.copy(
                    phase = AssistantPhase.Preparing,
                    preparationProgress = voiceState.progress,
                )
            }
            is VoiceInputState.Listening -> _state.update {
                it.copy(
                    phase = AssistantPhase.Listening,
                    levels = voiceState.levels,
                    elapsedMillis = voiceState.elapsedMillis,
                )
            }
            is VoiceInputState.Error -> _state.update {
                it.copy(phase = AssistantPhase.Error, errorMessage = voiceState.message)
            }
        }
    }

    private fun hasMicrophonePermission(): Boolean =
        ContextCompat.checkSelfPermission(appContext, Manifest.permission.RECORD_AUDIO) ==
            PackageManager.PERMISSION_GRANTED

    private companion object {
        const val MAX_INPUT_LENGTH = 8_000
    }
}
