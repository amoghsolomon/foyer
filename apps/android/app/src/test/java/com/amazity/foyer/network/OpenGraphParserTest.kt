package com.amazity.foyer.network

import org.junit.Assert.assertEquals
import org.junit.Test

class OpenGraphParserTest {
    @Test
    fun parsesOpenGraphAttributesInEitherOrder() {
        val result = OpenGraphParser.parse(
            """<html><head>
                <meta content="A useful page" property="og:title">
                <meta property='og:description' content='Details &amp; context'>
                <meta content="/image.jpg" property="og:image">
            </head></html>""".trimIndent(),
        )

        assertEquals("A useful page", result.title)
        assertEquals("Details & context", result.description)
        assertEquals("/image.jpg", result.imageUrl)
    }

    @Test
    fun fallsBackToDocumentTitleAndDescriptionMeta() {
        val result = OpenGraphParser.parse(
            "<title>Fallback title</title><meta name=\"description\" content=\"Summary\">",
        )

        assertEquals("Fallback title", result.title)
        assertEquals("Summary", result.description)
    }
}
