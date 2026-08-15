package com.amazity.foyer.voice

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class MoonshineKokoroReadAloudTest {
    @Test
    fun `short sentences stay together in one larger chunk`() {
        val chunks = kokoroChunks(
            "First sentence. Second sentence is a little longer. Third sentence.",
        )

        assertEquals(
            listOf("First sentence. Second sentence is a little longer. Third sentence."),
            chunks,
        )
    }

    @Test
    fun `long text is divided without dropping words`() {
        val source = (1..80).joinToString(" ") { "word$it" }

        val chunks = kokoroChunks(source, maximumLength = 120)
        val spokenWords = chunks
            .joinToString(" ")
            .replace(Regex("[,.;:!?]"), "")
            .split(Regex("\\s+"))

        assertTrue(chunks.size > 1)
        assertTrue(chunks.all { it.length <= 120 })
        assertEquals(source.split(" "), spokenWords)
    }

    @Test
    fun `a long token is still bounded`() {
        val chunks = kokoroChunks("a".repeat(125), maximumLength = 50)

        assertEquals(listOf(50, 50, 25), chunks.map(String::length))
    }

    @Test
    fun `requested Moonshine voice is Kokoro Heart`() {
        assertEquals("kokoro_af_heart", KOKORO_VOICE)
    }
}
