package com.amazity.foyer.assistant

import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class AssistantProtocolDeviceTest {
    @Test
    fun actionBlockIsRemovedFromDisplayTextAndParsed() {
        val turn = AssistantProtocol.parseText(
            "Opening Spotify.\n" +
                "<foyer-client-action>" +
                "{\"type\":\"open_app\",\"arguments\":{\"app\":\"Spotify\"}}" +
                "</foyer-client-action>",
        )

        assertEquals("Opening Spotify.", turn.text)
        assertEquals(ClientActionType.OpenApp, turn.action?.type)
        assertEquals("Spotify", turn.action?.argument("app"))
        assertFalse(requireNotNull(turn.action).requiresConfirmation)
    }
}
