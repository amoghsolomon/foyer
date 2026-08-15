package com.amazity.foyer.assistant

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class AssistantProtocolTest {
    @Test
    fun plainReplyHasNoAction() {
        val turn = AssistantProtocol.parseText("Here is the answer.")

        assertEquals("Here is the answer.", turn.text)
        assertNull(turn.action)
    }

    @Test
    fun mutatingFoyerActionsRequireConfirmation() {
        assertTrue(ClientAction(ClientActionType.CreateNote, emptyMap()).requiresConfirmation)
        assertTrue(ClientAction(ClientActionType.CreateReminder, emptyMap()).requiresConfirmation)
        assertFalse(ClientAction(ClientActionType.ComposeSms, emptyMap()).requiresConfirmation)
    }

    @Test
    fun unknownActionIsIgnoredButNotShownToUser() {
        val turn = AssistantProtocol.parseText(
            "I can't do that.\n" +
                "<foyer-client-action>{\"type\":\"run_shell\",\"arguments\":{}}</foyer-client-action>",
        )

        assertEquals("I can't do that.", turn.text)
        assertNull(turn.action)
    }

    @Test
    fun removedLinkActionsAreIgnoredButNotShownToUser() {
        listOf("open_url", "web_search", "navigate").forEach { type ->
            val turn = AssistantProtocol.parseText(
                "Use the link below.\n" +
                    "<foyer-client-action>{\"type\":\"$type\",\"arguments\":{}}</foyer-client-action>",
            )

            assertEquals("Use the link below.", turn.text)
            assertNull(turn.action)
        }
    }
}
