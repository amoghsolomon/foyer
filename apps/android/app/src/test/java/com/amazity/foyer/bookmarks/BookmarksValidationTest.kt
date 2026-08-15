package com.amazity.foyer.bookmarks

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class BookmarksValidationTest {
    @Test
    fun `http and https urls are accepted and scheme is normalized`() {
        assertEquals(
            "https://Example.COM/path?q=1",
            validateBookmarkUrl("  HTTPS://Example.COM/path?q=1  "),
        )
        assertEquals("http://localhost:8080/a", validateBookmarkUrl("http://localhost:8080/a"))
    }

    @Test
    fun `non http urls and empty hosts are rejected`() {
        assertFailsWithMessage("javascript:alert(1)")
        assertFailsWithMessage("ftp://example.com/file")
        assertFailsWithMessage("file:///etc/passwd")
        assertFailsWithMessage("https://")
        assertFailsWithMessage("https:///no-host")
        assertFailsWithMessage("https://exam ple.com")
        assertFailsWithMessage("not-a-url")
    }

    @Test
    fun `description is lossless including html and trailing newline`() {
        val description = "Keep <script>alert(1)</script> and **bold** losslessly.\n"
        assertEquals(description, losslessDescription(description))
    }

    @Test
    fun `description rejects nul bytes`() {
        val error = runCatching { losslessDescription("ok\u0000no") }.exceptionOrNull()
        assertTrue(error is IllegalArgumentException)
    }

    @Test
    fun `tags are trimmed lowercased and deduplicated`() {
        assertEquals(listOf("work", "docs"), normalizeBookmarkTags(listOf("  Work  ", "WORK", "docs", " Docs ")))
        assertEquals(listOf("work", "docs"), parseTagInput("Work, WORK, docs"))
    }

    @Test
    fun `empty and oversized tags are rejected`() {
        assertTrue(runCatching { normalizeBookmarkTags(listOf("   ")) }.isFailure)
        assertTrue(runCatching { normalizeBookmarkTag("x".repeat(MAX_TAG_LENGTH + 1)) }.isFailure)
        assertTrue(
            runCatching { normalizeBookmarkTags((0..MAX_TAGS).map { "t$it" }) }.isFailure,
        )
    }

    private fun assertFailsWithMessage(url: String) {
        val error = runCatching { validateBookmarkUrl(url) }.exceptionOrNull()
        assertTrue("expected $url to fail", error is IllegalArgumentException || error is IllegalStateException)
    }
}
