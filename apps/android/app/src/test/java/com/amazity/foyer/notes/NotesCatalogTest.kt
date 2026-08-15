package com.amazity.foyer.notes

import com.amazity.foyer.model.NotesCatalog
import com.amazity.foyer.model.NotesStatus
import com.amazity.foyer.model.NotesSyncBanner
import com.amazity.foyer.model.VaultFolder
import com.amazity.foyer.model.VaultNote
import com.amazity.foyer.model.notesSyncBanner
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class NotesCatalogTest {
    @Test
    fun `child folders are ordered by position then name`() {
        val catalog = NotesCatalog(
            folders = listOf(
                VaultFolder("b", "Beta", parentId = "root", position = 1),
                VaultFolder("a", "Alpha", parentId = "root", position = 0),
                VaultFolder("root", "Root", parentId = null, position = 0),
                VaultFolder("z", "Other", parentId = null, position = 1),
            ),
            notes = emptyList(),
            recentNoteIds = emptyList(),
        )
        assertEquals(listOf("a", "b"), catalog.childFolders("root").map(VaultFolder::id))
        assertEquals(listOf("root", "z"), catalog.childFolders(null).map(VaultFolder::id))
    }

    @Test
    fun `notes stay in their folder after a move`() {
        val note = VaultNote(
            id = "n1",
            folderId = "inbox",
            title = "Moved",
            summary = "body",
            updatedLabel = "",
            body = "body",
        )
        val catalog = NotesCatalog(
            folders = listOf(VaultFolder("inbox", "Inbox"), VaultFolder("later", "Later")),
            notes = listOf(note.copy(folderId = "later")),
            recentNoteIds = listOf("n1"),
        )
        assertEquals(0, catalog.notesIn("inbox").size)
        assertEquals("Moved", catalog.notesIn("later").single().title)
    }

    @Test
    fun `folder path walks parents without looping`() {
        val catalog = nestedCatalog()
        assertEquals(listOf("root", "child", "leaf"), catalog.folderPath("leaf").map(VaultFolder::id))
        assertEquals("Root / Child / Leaf", catalog.folderPathLabel("leaf"))
    }

    @Test
    fun `folder move targets exclude self and descendants`() {
        val catalog = nestedCatalog()
        val child = catalog.folder("child")!!
        assertEquals(listOf("other", "root"), catalog.validFolderMoveTargets(child).map(VaultFolder::id))
        assertEquals("A folder cannot be moved into itself.", catalog.validateFolderMove(child, "child"))
        assertEquals(
            "A folder cannot be moved into its own descendant.",
            catalog.validateFolderMove(child, "leaf"),
        )
        assertNull(catalog.validateFolderMove(child, "root"))
        assertNull(catalog.validateFolderMove(child, null))
    }

    @Test
    fun `folder delete is rejected while children remain`() {
        val catalog = nestedCatalog()
        assertEquals(
            "Folder is not empty. Move or delete its notes and folders first.",
            catalog.validateFolderDelete(catalog.folder("child")!!),
        )
        assertNull(catalog.validateFolderDelete(catalog.folder("other")!!))
    }

    @Test
    fun `sync banner prefers stale revision over offline and pending`() {
        assertEquals(
            NotesSyncBanner.StaleRevision("The expected revision does not match the current revision."),
            notesSyncBanner(
                NotesStatus(
                    loading = false,
                    offline = true,
                    pendingUploads = 2,
                    conflictCode = "stale_revision",
                    conflictMessage = "The expected revision does not match the current revision.",
                    lastError = "download failed",
                ),
            ),
        )
        assertEquals(
            NotesSyncBanner.Offline(3),
            notesSyncBanner(NotesStatus(loading = false, offline = true, pendingUploads = 3)),
        )
        assertEquals(
            NotesSyncBanner.Pending(1),
            notesSyncBanner(NotesStatus(loading = false, connected = true, pendingUploads = 1)),
        )
        assertTrue(
            notesSyncBanner(NotesStatus(loading = false, connected = true, lastError = "upload failed"))
                is NotesSyncBanner.Error,
        )
        assertNull(notesSyncBanner(NotesStatus(loading = false, connected = true)))
    }

    private fun nestedCatalog() = NotesCatalog(
        folders = listOf(
            VaultFolder("root", "Root"),
            VaultFolder("child", "Child", parentId = "root"),
            VaultFolder("leaf", "Leaf", parentId = "child"),
            VaultFolder("other", "Other"),
        ),
        notes = listOf(
            VaultNote(
                id = "n1",
                folderId = "child",
                title = "Nested",
                summary = "body",
                updatedLabel = "",
                body = "body",
            ),
        ),
        recentNoteIds = listOf("n1"),
    )
}
