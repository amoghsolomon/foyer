package com.amazity.foyer.voice

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import kotlinx.coroutines.delay
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import org.junit.Assert.assertFalse
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class MoonshineKokoroReadAloudDeviceTest {
    @Test
    fun downloadsLoadsAndSpeaksContinuouslyWithKokoro() = runBlocking {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val readAloud = MoonshineKokoroReadAloud(context)
        try {
            var becameActive = false
            readAloud.read(
                "Read aloud is now running locally. " +
                    "A second passage verifies that buffered playback continues in order.",
            )
            val terminalState = withTimeout(10 * 60 * 1_000L) {
                while (true) {
                    when (val state = readAloud.state.value) {
                        is ReadAloudState.Preparing, ReadAloudState.Speaking -> becameActive = true
                        is ReadAloudState.Error -> return@withTimeout state
                        ReadAloudState.Idle -> if (becameActive) return@withTimeout state
                    }
                    delay(100)
                }
                error("Unreachable")
            }
            assertFalse(
                (terminalState as? ReadAloudState.Error)?.message ?: "Kokoro failed",
                terminalState is ReadAloudState.Error,
            )
        } finally {
            readAloud.close()
        }
    }
}
