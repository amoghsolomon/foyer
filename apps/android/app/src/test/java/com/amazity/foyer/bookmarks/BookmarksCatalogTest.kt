package com.amazity.foyer.bookmarks

import com.amazity.foyer.model.BookmarkFolder
import com.amazity.foyer.model.BookmarkItem
import com.amazity.foyer.model.BookmarksCatalog
import com.amazity.foyer.model.BookmarksFilter
import com.amazity.foyer.model.BookmarksStatus
import com.amazity.foyer.model.BookmarksSyncBanner
import com.amazity.foyer.model.bookmarksSyncBanner
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class BookmarksCatalogTest {
    @Test
    fun `child folders are ordered by position then name`() {
        val catalog = BookmarksCatalog(
            folders = listOf(
                BookmarkFolder("b", "Beta", parentId = "root", position = 1),
                BookmarkFolder("a", "Alpha", parentId = "root", position = 0),
                BookmarkFolder("root", "Root", parentId = null, position = 0),
                BookmarkFolder("z", "Other", parentId = null, position = 1),
            ),
            bookmarks = emptyList(),
            recentBookmarkIds = emptyList(),
        )
        assertEquals(listOf("a", "b"), catalog.childFolders("root").map(BookmarkFolder::id))
        assertEquals(listOf("root", "z"), catalog.childFolders(null).map(BookmarkFolder::id))
    }

    @Test
    fun `bookmarks stay in their folder after a move`() {
        val bookmark = sampleBookmark("b1", "inbox", "Moved")
        val catalog = BookmarksCatalog(
            folders = listOf(BookmarkFolder("inbox", "Inbox"), BookmarkFolder("later", "Later")),
            bookmarks = listOf(bookmark.copy(folderId = "later")),
            recentBookmarkIds = listOf("b1"),
        )
        assertEquals(0, catalog.bookmarksIn("inbox").size)
        assertEquals("Moved", catalog.bookmarksIn("later").single().title)
    }

    @Test
    fun `folder path walks parents without looping`() {
        val catalog = nestedCatalog()
        assertEquals(listOf("root", "child", "leaf"), catalog.folderPath("leaf").map(BookmarkFolder::id))
        assertEquals("Root / Child / Leaf", catalog.folderPathLabel("leaf"))
    }

    @Test
    fun `folder move targets exclude self and descendants`() {
        val catalog = nestedCatalog()
        val child = catalog.folder("child")!!
        assertEquals(listOf("other", "root"), catalog.validFolderMoveTargets(child).map(BookmarkFolder::id))
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
            "Folder is not empty. Move or delete its bookmarks and folders first.",
            catalog.validateFolderDelete(catalog.folder("child")!!),
        )
        assertNull(catalog.validateFolderDelete(catalog.folder("other")!!))
    }

    @Test
    fun `search and filters hide archived unless requested`() {
        val catalog = BookmarksCatalog(
            folders = listOf(BookmarkFolder("inbox", "Inbox")),
            bookmarks = listOf(
                sampleBookmark("live", "inbox", "Rust docs", url = "https://doc.rust-lang.org", tags = listOf("rust")),
                sampleBookmark("fav", "inbox", "Favorite", favorite = true, tags = listOf("work")),
                sampleBookmark("old", "inbox", "Archived rust", archived = true, tags = listOf("rust")),
            ),
            recentBookmarkIds = listOf("live", "fav", "old"),
        )
        assertEquals(listOf("fav", "live"), catalog.visibleBookmarks().map(BookmarkItem::id))
        assertEquals(listOf("fav"), catalog.visibleBookmarks(filter = BookmarksFilter.Favorites).map(BookmarkItem::id))
        assertEquals(listOf("old"), catalog.visibleBookmarks(filter = BookmarksFilter.Archived).map(BookmarkItem::id))
        assertEquals(listOf("live"), catalog.visibleBookmarks(query = "doc.rust").map(BookmarkItem::id))
        assertEquals(listOf("fav"), catalog.visibleBookmarks(tag = "work").map(BookmarkItem::id))
        assertEquals(listOf("rust", "work"), catalog.allTags())
    }

    @Test
    fun `sync banner prefers stale revision over offline and pending`() {
        assertEquals(
            BookmarksSyncBanner.StaleRevision("The expected revision does not match the current revision."),
            bookmarksSyncBanner(
                BookmarksStatus(
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
            BookmarksSyncBanner.Offline(3),
            bookmarksSyncBanner(BookmarksStatus(loading = false, offline = true, pendingUploads = 3)),
        )
        assertEquals(
            BookmarksSyncBanner.Pending(1),
            bookmarksSyncBanner(BookmarksStatus(loading = false, connected = true, pendingUploads = 1)),
        )
        assertTrue(
            bookmarksSyncBanner(BookmarksStatus(loading = false, connected = true, lastError = "upload failed"))
                is BookmarksSyncBanner.Error,
        )
        assertNull(bookmarksSyncBanner(BookmarksStatus(loading = false, connected = true)))
    }

    @Test
    fun `replica mapping preserves lossless description and tags`() {
        val record = BookmarkRecord(
            id = "b1",
            userId = "dev-user",
            folderId = "inbox",
            url = "https://example.com/path",
            title = "Example",
            description = "Keep <script>alert(1)</script>\n",
            tags = listOf("work", "docs"),
            favorite = true,
            archived = false,
            position = 3,
            revision = 4,
            createdAt = "2026-08-15T00:00:00Z",
            updatedAt = "2026-08-15T00:00:00Z",
            deletedAt = null,
        )
        val item = vaultBookmark(record)
        assertEquals("Keep <script>alert(1)</script>\n", item.description)
        assertEquals(listOf("work", "docs"), item.tags)
        assertTrue(item.favorite)
        assertFalse(item.archived)
        assertEquals("example.com", item.host)
    }

    @Test
    fun `tag encoding survives the replica column`() {
        val encoded = encodeTags(listOf("work", "docs"))
        assertEquals(listOf("work", "docs"), decodeTags(encoded))
        assertEquals(emptyList<String>(), decodeTags(null))
        assertTrue(flagValue(1, null))
        assertTrue(flagValue(null, "true"))
        assertFalse(flagValue(0, "false"))
    }

    private fun nestedCatalog() = BookmarksCatalog(
        folders = listOf(
            BookmarkFolder("root", "Root"),
            BookmarkFolder("child", "Child", parentId = "root"),
            BookmarkFolder("leaf", "Leaf", parentId = "child"),
            BookmarkFolder("other", "Other"),
        ),
        bookmarks = listOf(sampleBookmark("b1", "child", "Nested")),
        recentBookmarkIds = listOf("b1"),
    )

    private fun sampleBookmark(
        id: String,
        folderId: String,
        title: String,
        url: String = "https://example.com/$id",
        description: String = "body",
        tags: List<String> = emptyList(),
        favorite: Boolean = false,
        archived: Boolean = false,
    ) = BookmarkItem(
        id = id,
        folderId = folderId,
        url = url,
        title = title,
        description = description,
        tags = tags,
        favorite = favorite,
        archived = archived,
        revision = 1,
        updatedAt = "2026-08-15T00:00:00Z",
    )
}
