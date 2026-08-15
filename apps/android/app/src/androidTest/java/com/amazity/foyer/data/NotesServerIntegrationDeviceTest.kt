package com.amazity.foyer.data

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.amazity.foyer.BuildConfig
import com.amazity.foyer.auth.FoyerAccountCoordinator
import com.amazity.foyer.model.VaultNote
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Assume.assumeTrue
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class NotesServerIntegrationDeviceTest {
    @Test
    fun offlineCapableWritesRoundTripThroughFoyerServer() = runBlocking {
        assumeTrue(
            "This smoke test only writes to the local emulator server",
            BuildConfig.FOYER_API_BASE_URL.startsWith("http://10.0.2.2") ||
                BuildConfig.FOYER_API_BASE_URL.startsWith("http://127.0.0.1"),
        )
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val coordinator = FoyerAccountCoordinator(context)
        if (coordinator.developmentAuthAvailable()) {
            coordinator.useDevelopmentSession()
        }
        val repository = FoyerRepository(context)
        repository.refreshNotes()
        val folder = repository.createFolder("Device folder ${System.currentTimeMillis()}")
        var serverNote: VaultNote? = null

        try {
            serverNote = repository.createNote(
                title = "Emulator repository verification",
                body = "# Keep\n\n<script>alert(1)</script> lossless.",
                folderId = folder.id,
            )
            repository.refreshNotes()
            val cached = repository.notes.first { catalog -> catalog.note(serverNote!!.id) != null }
                .note(serverNote.id)
            assertEquals("Emulator repository verification", cached?.title)
            assertTrue(cached?.body.orEmpty().contains("<script>alert(1)</script>"))

            val bodyOnlyUpdate = "# Device update\n\n**body-only**\n"
            serverNote = repository.updateNote(
                note = cached!!,
                title = cached.title,
                body = bodyOnlyUpdate,
                folderId = folder.id,
            )
            repository.refreshNotes()
            assertEquals(
                bodyOnlyUpdate,
                repository.notes
                    .first { catalog ->
                        catalog.status.pendingUploads == 0 &&
                            catalog.note(serverNote.id)?.body == bodyOnlyUpdate
                    }
                    .note(serverNote.id)
                    ?.body,
            )
        } finally {
            serverNote?.let { note ->
                runCatching { repository.deleteNote(note) }
                withTimeout(30_000) {
                    repository.notes.first { catalog -> catalog.note(note.id) == null }
                }
            }
            runCatching { repository.deleteFolder(folder) }
            withTimeout(30_000) {
                repository.notes.first { catalog ->
                    catalog.folder(folder.id) == null && catalog.status.pendingUploads == 0
                }
            }
        }

        repository.refreshNotes()
        serverNote?.let { note ->
            assertFalse(
                repository.notes
                    .first { catalog -> catalog.status.pendingUploads == 0 }
                    .notes
                .any { it.id == note.id },
            )
        }
        Unit
    }
}
