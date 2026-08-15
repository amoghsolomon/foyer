package com.amazity.foyer.notes

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class SafeMarkdownTest {
    @Test
    fun `html is kept as literal text and never becomes a script block`() {
        val blocks = markdownBlocks("# Title\n\n<script>alert(1)</script>\n\n- item")
        assertTrue(blocks.any { it is MarkdownBlock.Heading && it.text == "Title" })
        assertTrue(blocks.any { it is MarkdownBlock.Paragraph && it.text.contains("<script>alert(1)</script>") })
        assertTrue(blocks.any { it is MarkdownBlock.ListItem && it.text == "item" })
    }

    @Test
    fun `summary prefers the first non-heading line`() {
        assertEquals(
            "Keep this sentence",
            summaryOf("# Title\n\nKeep this sentence\n\nMore"),
        )
    }
}
