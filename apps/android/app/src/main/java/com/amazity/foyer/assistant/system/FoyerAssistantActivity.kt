package com.amazity.foyer.assistant.system

import android.Manifest
import android.content.Intent
import android.content.pm.PackageManager
import android.graphics.Color
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.SystemBarStyle
import androidx.activity.compose.BackHandler
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.core.content.ContextCompat
import com.amazity.foyer.assistant.AssistantOverlayHost
import com.amazity.foyer.foyerApplication
import com.amazity.foyer.ui.theme.FoyerTheme
import com.amazity.foyer.voice.MoonshineKokoroReadAloud
import com.amazity.foyer.voice.ReadAloudState

class FoyerAssistantActivity : ComponentActivity() {
    private lateinit var readAloud: MoonshineKokoroReadAloud
    private val microphonePermission = registerForActivityResult(
        ActivityResultContracts.RequestPermission(),
    ) { granted ->
        if (granted) controller.startListening() else controller.microphonePermissionDenied()
    }

    private val controller get() = foyerApplication.assistantController

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        readAloud = MoonshineKokoroReadAloud(applicationContext)
        enableEdgeToEdge(
            statusBarStyle = SystemBarStyle.dark(Color.TRANSPARENT),
            navigationBarStyle = SystemBarStyle.dark(Color.TRANSPARENT),
        )
        showAssistant()
        setContent {
            FoyerTheme {
                val state by controller.state.collectAsState()
                val readAloudState by readAloud.state.collectAsState()
                var activeReadAloudMessageId by remember { mutableStateOf<String?>(null) }
                BackHandler {
                    readAloud.stop()
                    controller.dismiss()
                }
                LaunchedEffect(state.visible) {
                    if (!state.visible) {
                        readAloud.stop()
                        finish()
                    }
                }
                LaunchedEffect(readAloudState) {
                    if (readAloudState is ReadAloudState.Idle) activeReadAloudMessageId = null
                }
                AssistantOverlayHost(
                    state = state,
                    onInputChange = controller::editInput,
                    onToggleListening = controller::toggleListening,
                    onSubmit = controller::submit,
                    onConfirm = controller::confirmPendingAction,
                    onCancelAction = controller::cancelPendingAction,
                    onDismiss = {
                        readAloud.stop()
                        controller.dismiss()
                    },
                    readAloudState = readAloudState,
                    activeReadAloudMessageId = activeReadAloudMessageId,
                    onToggleReadAloud = { id, text ->
                        val active = readAloudState is ReadAloudState.Preparing ||
                            readAloudState is ReadAloudState.Speaking
                        if (active && activeReadAloudMessageId == id) {
                            readAloud.stop()
                            activeReadAloudMessageId = null
                        } else {
                            readAloud.stop()
                            activeReadAloudMessageId = id
                            readAloud.read(text)
                        }
                    },
                )
            }
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        showAssistant()
    }

    override fun onDestroy() {
        if (isFinishing) controller.dismiss()
        readAloud.close()
        super.onDestroy()
    }

    private fun showAssistant() {
        controller.show()
        if (
            ContextCompat.checkSelfPermission(this, Manifest.permission.RECORD_AUDIO) ==
            PackageManager.PERMISSION_GRANTED
        ) {
            controller.startListening()
        } else {
            microphonePermission.launch(Manifest.permission.RECORD_AUDIO)
        }
    }
}
