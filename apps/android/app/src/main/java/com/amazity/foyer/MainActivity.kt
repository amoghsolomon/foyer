package com.amazity.foyer

import android.content.Intent
import android.graphics.Color
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.SystemBarStyle
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import com.amazity.foyer.notifications.FoyerNotifications
import com.amazity.foyer.sync.SyncScheduler
import com.amazity.foyer.ui.FoyerApp
import com.amazity.foyer.ui.theme.FoyerTheme

class MainActivity : ComponentActivity() {
    private val homeRequestVersion = mutableIntStateOf(0)
    private val deepLinkRequestVersion = mutableIntStateOf(0)
    private val deepLinkTargetType = mutableStateOf<String?>(null)
    private val deepLinkTargetId = mutableStateOf<String?>(null)
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        handleIntent(intent)
        SyncScheduler.ensureScheduled(this)
        enableEdgeToEdge(
            statusBarStyle = SystemBarStyle.dark(Color.TRANSPARENT),
            navigationBarStyle = SystemBarStyle.dark(Color.TRANSPARENT),
        )
        setContent {
            FoyerTheme {
                FoyerApp(
                    homeRequestVersion = homeRequestVersion.intValue,
                    deepLinkRequestVersion = deepLinkRequestVersion.intValue,
                    deepLinkTargetType = deepLinkTargetType.value,
                    deepLinkTargetId = deepLinkTargetId.value,
                )
            }
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        if (intent.action == Intent.ACTION_MAIN) {
            homeRequestVersion.intValue += 1
        }
        handleIntent(intent)
    }

    private fun handleIntent(intent: Intent?) {
        if (intent?.action != FoyerNotifications.ACTION_OPEN_TARGET) return
        val type = intent.getStringExtra("targetType")
        val id = intent.getStringExtra("targetId")
        if (type !in setOf("activity", "calendar", "task") || id.isNullOrBlank()) return
        deepLinkTargetType.value = type
        deepLinkTargetId.value = id
        deepLinkRequestVersion.intValue += 1
    }
}
