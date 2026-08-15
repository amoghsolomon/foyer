package com.amazity.foyer.ui.screens

import com.amazity.foyer.model.VaultFolder
import com.amazity.foyer.model.VaultNote
import org.junit.Assert.assertEquals
import org.junit.Test

class NoteEditorScreenTest {
    @Test
    fun `dictation is appended once and partial text replaces the dictated segment`() {
        val base = "Existing note"

        assertEquals("Existing note\n\nhello", mergeDictation(base, "hello"))
        assertEquals("Existing note\n\nhello world", mergeDictation(base, "hello world"))
    }

    @Test
    fun `dictation becomes the body when the note is empty`() {
        assertEquals("first words", mergeDictation("", "first words"))
    }

    @Test
    fun `folder picker labels include ancestors`() {
        val folders = listOf(
            VaultFolder("root", "Root"),
            VaultFolder("child", "Child", parentId = "root"),
            VaultFolder("leaf", "Leaf", parentId = "child"),
        )
        assertEquals("Root / Child / Leaf", folderPathLabel(folders, "leaf"))
    }

    @Test
    fun `read aloud text removes markdown and wikilink syntax`() {
        val note = VaultNote(
            id = "note-1",
            folderId = "inbox",
            title = "Voice plan",
            summary = "",
            updatedLabel = "",
            body = "# Next step\nUse [[Moonshine]] with **Kokoro**.",
        )

        assertEquals(
            "Voice plan. Next step Use Moonshine with Kokoro.",
            readableNoteText(note),
        )
    }
}
