package com.amazity.foyer.bookmarks

const val MAX_FOLDER_NAME = 80
const val MAX_BOOKMARK_TITLE = 200
const val MAX_DESCRIPTION_BYTES = 64 * 1024
const val MAX_URL_BYTES = 2048
const val MAX_TAGS = 16
const val MAX_TAG_LENGTH = 32

fun requiredFolderName(value: String): String {
    val trimmed = value.trim()
    require(trimmed.isNotEmpty()) { "Folder name is required" }
    require(trimmed.length <= MAX_FOLDER_NAME) { "Folder name is too long" }
    require('\u0000' !in trimmed && trimmed.none(Char::isISOControl)) { "Folder name is invalid" }
    return trimmed
}

fun requiredBookmarkTitle(value: String): String {
    val trimmed = value.trim()
    require(trimmed.isNotEmpty()) { "Title is required" }
    require(trimmed.length <= MAX_BOOKMARK_TITLE) { "Title is too long" }
    require('\u0000' !in trimmed && trimmed.none(Char::isISOControl)) { "Title is invalid" }
    return trimmed
}

fun losslessDescription(value: String): String {
    require(value.toByteArray(Charsets.UTF_8).size <= MAX_DESCRIPTION_BYTES) { "Description is too long" }
    require('\u0000' !in value) { "Description cannot contain NUL bytes" }
    return value
}

fun validateBookmarkUrl(value: String): String {
    val trimmed = value.trim()
    require(trimmed.isNotEmpty()) { "URL is required" }
    require(trimmed.toByteArray(Charsets.UTF_8).size <= MAX_URL_BYTES) { "URL is too long" }
    require('\u0000' !in trimmed && trimmed.none { it.isISOControl() || it.isWhitespace() }) {
        "URL cannot contain whitespace or control characters"
    }
    val rest = when {
        trimmed.startsWith("https://", ignoreCase = true) -> "https://" to trimmed.substring(8)
        trimmed.startsWith("http://", ignoreCase = true) -> "http://" to trimmed.substring(7)
        else -> error("Only HTTP and HTTPS URLs are accepted")
    }
    val afterScheme = rest.second
    require(afterScheme.isNotEmpty()) { "URL must include a host" }
    val authorityEnd = afterScheme.indexOfFirst { it == '/' || it == '?' || it == '#' }
        .takeIf { it >= 0 } ?: afterScheme.length
    val authority = afterScheme.substring(0, authorityEnd)
    require(authority.isNotEmpty()) { "URL must include a host" }
    val hostport = authority.substringAfterLast('@')
    require(hostport.isNotEmpty()) { "URL must include a host" }
    val host = if (hostport.startsWith('[')) {
        val end = hostport.indexOf(']')
        require(end > 1) { "URL host is invalid" }
        hostport.substring(1, end)
    } else {
        hostport.substringBefore(':')
    }
    require(host.isNotEmpty() && host != "." && host != "..") { "URL must include a host" }
    return rest.first + afterScheme
}

fun normalizeBookmarkTags(values: Collection<String>): List<String> {
    val tags = ArrayList<String>()
    values.forEach { raw ->
        val tag = normalizeBookmarkTag(raw)
        if (tag !in tags) tags.add(tag)
    }
    require(tags.size <= MAX_TAGS) { "A bookmark may have at most $MAX_TAGS tags" }
    return tags
}

fun normalizeBookmarkTag(value: String): String {
    val collapsed = value.trim().lowercase().split(Regex("\\s+")).filter { it.isNotEmpty() }.joinToString(" ")
    require(collapsed.isNotEmpty()) { "Tags cannot be empty" }
    require(collapsed.length <= MAX_TAG_LENGTH) { "Each tag must be at most $MAX_TAG_LENGTH characters" }
    require('\u0000' !in collapsed && collapsed.none(Char::isISOControl)) { "Tags cannot contain control characters" }
    return collapsed
}

fun parseTagInput(value: String): List<String> =
    normalizeBookmarkTags(value.split(',', '#', '\n').map { it.trim() }.filter { it.isNotEmpty() })
