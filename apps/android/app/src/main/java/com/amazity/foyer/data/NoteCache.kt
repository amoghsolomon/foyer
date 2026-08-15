package com.amazity.foyer.data

import com.amazity.foyer.model.NotesCatalog
import com.amazity.foyer.model.VaultFolder
import com.amazity.foyer.model.VaultNote
import java.time.Duration
import java.time.Instant
import org.json.JSONArray
import org.json.JSONObject

internal suspend fun FoyerDao.replaceNotesFrom(response: JSONObject) {
    val folders = response.optJSONArray("folders").objects().map(::cachedFolder)
    val notes = response.optJSONArray("notes").objects().map(::cachedNote)
    replaceNoteCache(folders, notes)
}

internal fun cachedFolder(value: JSONObject) = CachedNoteFolder(
    id = value.getString("id"),
    name = value.getString("name"),
    position = value.optInt("position"),
    updatedAt = value.optString("updatedAt"),
)

internal fun cachedNote(value: JSONObject) = CachedNote(
    id = value.getString("id"),
    folderId = value.getString("folderId"),
    title = value.getString("title"),
    body = value.getString("body"),
    summary = value.optString("summary"),
    tagsJson = value.optJSONArray("tags")?.toString() ?: "[]",
    linkedFromJson = value.optJSONArray("linkedFrom")?.toString() ?: "[]",
    version = value.optLong("version", 1L),
    createdAt = value.optString("createdAt"),
    updatedAt = value.optString("updatedAt"),
)

internal fun notesCatalog(
    folders: List<CachedNoteFolder>,
    notes: List<CachedNote>,
) = NotesCatalog(
    folders = folders.map { VaultFolder(it.id, it.name) },
    notes = notes.map(::vaultNote),
    recentNoteIds = notes.take(20).map(CachedNote::id),
)

internal fun vaultNote(note: CachedNote) = VaultNote(
    id = note.id,
    folderId = note.folderId,
    title = note.title,
    summary = note.summary,
    updatedLabel = updatedLabel(note.updatedAt),
    tags = jsonStrings(note.tagsJson),
    body = note.body,
    linkedFrom = jsonStrings(note.linkedFromJson),
    version = note.version,
    createdAt = note.createdAt,
    updatedAt = note.updatedAt,
)

private fun JSONArray?.objects(): List<JSONObject> = buildList {
    val array = this@objects ?: return@buildList
    for (index in 0 until array.length()) array.optJSONObject(index)?.let(::add)
}

private fun jsonStrings(value: String): List<String> = runCatching {
    val array = JSONArray(value)
    buildList {
        for (index in 0 until array.length()) {
            array.optString(index).takeIf(String::isNotBlank)?.let(::add)
        }
    }
}.getOrDefault(emptyList())

private fun updatedLabel(value: String): String {
    val instant = runCatching { Instant.parse(value) }.getOrElse {
        runCatching { Instant.parse(value.replace(' ', 'T') + "Z") }.getOrNull()
    } ?: return "Updated on server"
    val age = Duration.between(instant, Instant.now()).coerceAtLeast(Duration.ZERO)
    return when {
        age.toMinutes() < 1 -> "Updated just now"
        age.toHours() < 1 -> "Updated ${age.toMinutes()}m ago"
        age.toDays() < 1 -> "Updated ${age.toHours()}h ago"
        else -> "Updated ${age.toDays()}d ago"
    }
}
