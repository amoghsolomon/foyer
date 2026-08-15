package com.amazity.foyer.model

/** Read models for the server-owned bookmarks vault and its PowerSync replica. */
data class BookmarkFolder(
    val id: String,
    val name: String,
    val parentId: String? = null,
    val position: Int = 0,
    val revision: Long = 1,
)

data class BookmarkItem(
    val id: String,
    val folderId: String,
    val url: String,
    val title: String,
    val description: String,
    val tags: List<String> = emptyList(),
    val favorite: Boolean = false,
    val archived: Boolean = false,
    val position: Int = 0,
    val revision: Long = 1,
    val createdAt: String = "",
    val updatedAt: String = "",
) {
    val host: String = bookmarkHost(url)
    val summary: String = bookmarkSummary(description, url)
    val updatedLabel: String = bookmarkUpdatedLabel(updatedAt)
}

data class BookmarksStatus(
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
    fun banner(): BookmarksSyncBanner? = bookmarksSyncBanner(this)
}

sealed class BookmarksSyncBanner {
    data class Offline(val pendingUploads: Int) : BookmarksSyncBanner()
    data class Pending(val pendingUploads: Int) : BookmarksSyncBanner()
    data class StaleRevision(val message: String) : BookmarksSyncBanner()
    data class Error(val message: String) : BookmarksSyncBanner()
}

enum class BookmarksFilter {
    All,
    Favorites,
    Archived,
}

fun bookmarksSyncBanner(status: BookmarksStatus): BookmarksSyncBanner? {
    val conflict = status.conflictMessage?.takeIf { it.isNotBlank() }
    if (conflict != null) {
        return if (status.conflictCode == "stale_revision" || conflict.contains("stale revision", ignoreCase = true)) {
            BookmarksSyncBanner.StaleRevision(conflict)
        } else {
            BookmarksSyncBanner.Error(conflict)
        }
    }
    status.lastError?.takeIf { it.isNotBlank() }?.let { return BookmarksSyncBanner.Error(it) }
    if (status.offline) return BookmarksSyncBanner.Offline(status.pendingUploads)
    if (status.pendingUploads > 0) return BookmarksSyncBanner.Pending(status.pendingUploads)
    return null
}

data class BookmarksCatalog(
    val folders: List<BookmarkFolder>,
    val bookmarks: List<BookmarkItem>,
    val recentBookmarkIds: List<String>,
    val status: BookmarksStatus = BookmarksStatus(loading = false),
) {
    fun folder(folderId: String): BookmarkFolder? = folders.firstOrNull { it.id == folderId }

    fun bookmark(bookmarkId: String): BookmarkItem? = bookmarks.firstOrNull { it.id == bookmarkId }

    fun bookmarksIn(folderId: String): List<BookmarkItem> =
        bookmarks.filter { it.folderId == folderId }.sortedWith(
            compareBy<BookmarkItem> { it.archived }
                .thenByDescending { it.favorite }
                .thenBy(BookmarkItem::position)
                .thenBy(BookmarkItem::title)
                .thenBy(BookmarkItem::id),
        )

    fun childFolders(parentId: String?): List<BookmarkFolder> =
        folders.filter { it.parentId == parentId }
            .sortedWith(compareBy(BookmarkFolder::position, BookmarkFolder::name, BookmarkFolder::id))

    fun recentBookmarks(): List<BookmarkItem> = recentBookmarkIds.mapNotNull(::bookmark)

    fun folderPath(folderId: String): List<BookmarkFolder> {
        val path = ArrayList<BookmarkFolder>()
        val seen = HashSet<String>()
        var current = folder(folderId)
        while (current != null && seen.add(current.id)) {
            path.add(0, current)
            current = current.parentId?.let(::folder)
        }
        return path
    }

    fun folderPathLabel(folderId: String): String =
        folderPath(folderId).joinToString(" / ", transform = BookmarkFolder::name)

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
        childFolders(folderId).isEmpty() && bookmarksIn(folderId).isEmpty()

    fun validFolderMoveTargets(folder: BookmarkFolder): List<BookmarkFolder> {
        val blocked = descendantFolderIds(folder.id)
        return folders
            .filter { it.id !in blocked }
            .sortedWith(compareBy({ folderPathLabel(it.id) }, BookmarkFolder::id))
    }

    fun validateFolderMove(folder: BookmarkFolder, parentId: String?): String? {
        if (parentId == null) return null
        if (parentId == folder.id) return "A folder cannot be moved into itself."
        if (parentId in descendantFolderIds(folder.id)) {
            return "A folder cannot be moved into its own descendant."
        }
        if (folder(parentId) == null) return "The destination folder was not found."
        return null
    }

    fun validateFolderDelete(folder: BookmarkFolder): String? =
        if (folderIsEmpty(folder.id)) {
            null
        } else {
            "Folder is not empty. Move or delete its bookmarks and folders first."
        }

    fun allTags(): List<String> =
        bookmarks.flatMap { it.tags }.distinct().sorted()

    fun visibleBookmarks(
        query: String = "",
        filter: BookmarksFilter = BookmarksFilter.All,
        tag: String? = null,
        folderId: String? = null,
    ): List<BookmarkItem> {
        val needle = query.trim().lowercase()
        return bookmarks
            .asSequence()
            .filter { bookmark ->
                when (filter) {
                    BookmarksFilter.All -> !bookmark.archived
                    BookmarksFilter.Favorites -> bookmark.favorite && !bookmark.archived
                    BookmarksFilter.Archived -> bookmark.archived
                }
            }
            .filter { folderId == null || it.folderId == folderId }
            .filter { tag.isNullOrBlank() || tag.lowercase() in it.tags }
            .filter { bookmark ->
                needle.isEmpty() ||
                    bookmark.title.lowercase().contains(needle) ||
                    bookmark.url.lowercase().contains(needle) ||
                    bookmark.description.lowercase().contains(needle) ||
                    bookmark.tags.any { it.contains(needle) } ||
                    bookmark.host.lowercase().contains(needle)
            }
            .sortedWith(
                compareBy<BookmarkItem> { it.archived }
                    .thenByDescending { it.favorite }
                    .thenByDescending { it.updatedAt }
                    .thenBy(BookmarkItem::title)
                    .thenBy(BookmarkItem::id),
            )
            .toList()
    }
}

fun bookmarkHost(url: String): String {
    val withoutScheme = url.substringAfter("://", missingDelimiterValue = url)
    val authority = withoutScheme.substringBefore('/').substringBefore('?').substringBefore('#')
    val hostport = authority.substringAfterLast('@')
    return if (hostport.startsWith('[')) {
        hostport.substringAfter('[').substringBefore(']')
    } else {
        hostport.substringBefore(':')
    }
}

fun bookmarkSummary(description: String, url: String): String {
    val line = description.lineSequence().map { it.trim() }.firstOrNull { it.isNotEmpty() }
    return line?.take(140) ?: url
}

fun bookmarkUpdatedLabel(value: String): String {
    if (value.isBlank()) return "Updated locally"
    val instant = runCatching { java.time.Instant.parse(value) }.getOrNull() ?: return "Updated locally"
    val age = java.time.Duration.between(instant, java.time.Instant.now()).coerceAtLeast(java.time.Duration.ZERO)
    return when {
        age.toMinutes() < 1 -> "Updated just now"
        age.toHours() < 1 -> "Updated ${age.toMinutes()}m ago"
        age.toDays() < 1 -> "Updated ${age.toHours()}h ago"
        else -> "Updated ${age.toDays()}d ago"
    }
}
