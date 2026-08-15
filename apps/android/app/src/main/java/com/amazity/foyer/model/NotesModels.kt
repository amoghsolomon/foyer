package com.amazity.foyer.model

/** Read models for the server-owned notes vault and its PowerSync replica. */
data class VaultFolder(
    val id: String,
    val name: String,
    val parentId: String? = null,
    val position: Int = 0,
    val revision: Long = 1,
)

data class VaultNote(
    val id: String,
    val folderId: String,
    val title: String,
    val summary: String,
    val updatedLabel: String,
    val tags: List<String> = emptyList(),
    val body: String,
    val linkedFrom: List<String> = emptyList(),
    val version: Long = 1,
    val createdAt: String = "",
    val updatedAt: String = "",
)

data class NotesStatus(
    val loading: Boolean = true,
    val connected: Boolean = false,
    val offline: Boolean = false,
    val pendingUploads: Int = 0,
    val lastError: String? = null,
    val conflictCode: String? = null,
    val conflictMessage: String? = null,
    val developmentAuth: Boolean = false,
    val usingPowerSync: Boolean = false,
) {
    fun banner(): NotesSyncBanner? = notesSyncBanner(this)
}

sealed class NotesSyncBanner {
    data class Offline(val pendingUploads: Int) : NotesSyncBanner()
    data class Pending(val pendingUploads: Int) : NotesSyncBanner()
    data class StaleRevision(val message: String) : NotesSyncBanner()
    data class Error(val message: String) : NotesSyncBanner()
}

fun notesSyncBanner(status: NotesStatus): NotesSyncBanner? {
    val conflict = status.conflictMessage?.takeIf { it.isNotBlank() }
    if (conflict != null) {
        return if (status.conflictCode == "stale_revision" || conflict.contains("stale revision", ignoreCase = true)) {
            NotesSyncBanner.StaleRevision(conflict)
        } else {
            NotesSyncBanner.Error(conflict)
        }
    }
    status.lastError?.takeIf { it.isNotBlank() }?.let { return NotesSyncBanner.Error(it) }
    if (status.offline) return NotesSyncBanner.Offline(status.pendingUploads)
    if (status.pendingUploads > 0) return NotesSyncBanner.Pending(status.pendingUploads)
    return null
}

data class NotesCatalog(
    val folders: List<VaultFolder>,
    val notes: List<VaultNote>,
    val recentNoteIds: List<String>,
    val status: NotesStatus = NotesStatus(loading = false),
) {
    fun folder(folderId: String): VaultFolder? = folders.firstOrNull { it.id == folderId }

    fun note(noteId: String): VaultNote? = notes.firstOrNull { it.id == noteId }

    fun notesIn(folderId: String): List<VaultNote> = notes.filter { it.folderId == folderId }

    fun childFolders(parentId: String?): List<VaultFolder> =
        folders.filter { it.parentId == parentId }.sortedWith(compareBy(VaultFolder::position, VaultFolder::name, VaultFolder::id))

    fun recentNotes(): List<VaultNote> = recentNoteIds.mapNotNull(::note)

    fun folderPath(folderId: String): List<VaultFolder> {
        val path = ArrayList<VaultFolder>()
        val seen = HashSet<String>()
        var current = folder(folderId)
        while (current != null && seen.add(current.id)) {
            path.add(0, current)
            current = current.parentId?.let(::folder)
        }
        return path
    }

    fun folderPathLabel(folderId: String): String =
        folderPath(folderId).joinToString(" / ", transform = VaultFolder::name)

    fun descendantFolderIds(folderId: String): Set<String> {
        val ids = linkedSetOf<String>()
        val queue = ArrayDeque<String>()
        queue.add(folderId)
        while (queue.isNotEmpty()) {
            val id = queue.removeFirst()
            if (!ids.add(id)) continue
            childFolders(id).forEach { queue.add(it.id) }
        }
        return ids
    }

    fun folderIsEmpty(folderId: String): Boolean =
        childFolders(folderId).isEmpty() && notesIn(folderId).isEmpty()

    fun validFolderMoveTargets(folder: VaultFolder): List<VaultFolder> {
        val blocked = descendantFolderIds(folder.id)
        return folders
            .filter { it.id !in blocked }
            .sortedWith(compareBy({ folderPathLabel(it.id) }, VaultFolder::id))
    }

    fun validateFolderMove(folder: VaultFolder, parentId: String?): String? {
        if (parentId == null) return null
        if (parentId == folder.id) return "A folder cannot be moved into itself."
        if (parentId in descendantFolderIds(folder.id)) {
            return "A folder cannot be moved into its own descendant."
        }
        if (folder(parentId) == null) return "The destination folder was not found."
        return null
    }

    fun validateFolderDelete(folder: VaultFolder): String? =
        if (folderIsEmpty(folder.id)) {
            null
        } else {
            "Folder is not empty. Move or delete its notes and folders first."
        }
}
