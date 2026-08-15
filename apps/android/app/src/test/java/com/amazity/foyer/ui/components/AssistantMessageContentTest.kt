package com.amazity.foyer.ui.components

import org.junit.Assert.assertEquals
import org.junit.Test

class AssistantMessageContentTest {
    @Test
    fun findsSupportedUrisAndTrimsSentencePunctuation() {
        val links = assistantLinks(
            "Read https://example.com/a. Map geo:12.1,77.2?q=Cafe and call tel:+15551234!",
        )

        assertEquals(
            listOf("https://example.com/a", "geo:12.1,77.2?q=Cafe", "tel:+15551234"),
            links.map(AssistantLink::uri),
        )
    }

    @Test
    fun ignoresHttpAndUnrelatedSchemes() {
        assertEquals(emptyList<AssistantLink>(), assistantLinks("http://example.com mailto:a@example.com"))
    }
}
