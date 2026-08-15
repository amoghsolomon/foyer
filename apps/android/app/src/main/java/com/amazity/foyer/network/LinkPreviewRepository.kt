package com.amazity.foyer.network

import android.content.Context
import com.amazity.foyer.data.CachedLinkPreview
import com.amazity.foyer.data.FoyerDatabase
import java.io.ByteArrayOutputStream
import java.net.HttpURLConnection
import java.net.URL
import java.util.concurrent.TimeUnit
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

class LinkPreviewRepository private constructor(context: Context) {
    private val dao = FoyerDatabase.get(context.applicationContext).foyerDao()

    suspend fun load(url: String): CachedLinkPreview? = withContext(Dispatchers.IO) {
        val cached = dao.linkPreview(url)
        if (cached != null && System.currentTimeMillis() - cached.fetchedAt < CACHE_MAX_AGE) {
            return@withContext cached.takeUnless(CachedLinkPreview::failed)
        }
        val fetched = runCatching { fetch(url) }.getOrElse {
            CachedLinkPreview(url, null, null, null, true, System.currentTimeMillis())
        }
        dao.upsertLinkPreview(fetched)
        fetched.takeUnless(CachedLinkPreview::failed)
    }

    private fun fetch(url: String): CachedLinkPreview {
        val requested = URL(url)
        require(requested.protocol.equals("https", ignoreCase = true))
        val page = open(requested, PAGE_LIMIT, "text/html")
        require(page.finalUrl.protocol.equals("https", ignoreCase = true))
        val metadata = OpenGraphParser.parse(page.bytes.toString(Charsets.UTF_8))
        require(!metadata.title.isNullOrBlank() || !metadata.description.isNullOrBlank())
        val imageBytes = metadata.imageUrl
            ?.let { runCatching { URL(page.finalUrl, it) }.getOrNull() }
            ?.takeIf { image ->
                image.protocol.equals("https", ignoreCase = true) &&
                    image.host.equals(page.finalUrl.host, ignoreCase = true)
            }
            ?.let { image -> runCatching { open(image, IMAGE_LIMIT, "image/").bytes }.getOrNull() }
        return CachedLinkPreview(
            url = url,
            title = metadata.title?.take(MAX_TITLE_LENGTH),
            description = metadata.description?.take(MAX_DESCRIPTION_LENGTH),
            imageBytes = imageBytes,
            failed = false,
            fetchedAt = System.currentTimeMillis(),
        )
    }

    private fun open(url: URL, limit: Int, expectedContentType: String): Download {
        val connection = url.openConnection() as HttpURLConnection
        return try {
            connection.instanceFollowRedirects = true
            connection.connectTimeout = TIMEOUT_MILLIS
            connection.readTimeout = TIMEOUT_MILLIS
            connection.setRequestProperty("user-agent", "Foyer Android link preview")
            connection.connect()
            require(connection.responseCode in 200..299)
            require(connection.contentType.orEmpty().lowercase().startsWith(expectedContentType))
            val bytes = connection.inputStream.use { it.readAtMost(limit) }
            Download(connection.url, bytes)
        } finally {
            connection.disconnect()
        }
    }

    private fun java.io.InputStream.readAtMost(limit: Int): ByteArray {
        val output = ByteArrayOutputStream()
        val buffer = ByteArray(8_192)
        var remaining = limit
        while (remaining > 0) {
            val count = read(buffer, 0, minOf(buffer.size, remaining))
            if (count < 0) break
            output.write(buffer, 0, count)
            remaining -= count
        }
        return output.toByteArray()
    }

    private data class Download(val finalUrl: URL, val bytes: ByteArray)

    companion object {
        private val CACHE_MAX_AGE = TimeUnit.DAYS.toMillis(7)
        private const val TIMEOUT_MILLIS = 4_000
        private const val PAGE_LIMIT = 512 * 1_024
        private const val IMAGE_LIMIT = 512 * 1_024
        private const val MAX_TITLE_LENGTH = 300
        private const val MAX_DESCRIPTION_LENGTH = 600

        @Volatile private var instance: LinkPreviewRepository? = null

        fun get(context: Context): LinkPreviewRepository = instance ?: synchronized(this) {
            instance ?: LinkPreviewRepository(context).also { instance = it }
        }
    }
}

data class OpenGraphMetadata(
    val title: String?,
    val description: String?,
    val imageUrl: String?,
)

object OpenGraphParser {
    private val metaPattern = Regex("<meta\\b[^>]*>", RegexOption.IGNORE_CASE)
    private val attributePattern = Regex(
        "([a-zA-Z_:][-a-zA-Z0-9_:.]*)\\s*=\\s*([\"'])(.*?)\\2",
        setOf(RegexOption.IGNORE_CASE, RegexOption.DOT_MATCHES_ALL),
    )
    private val titlePattern = Regex(
        "<title\\b[^>]*>(.*?)</title>",
        setOf(RegexOption.IGNORE_CASE, RegexOption.DOT_MATCHES_ALL),
    )

    fun parse(html: String): OpenGraphMetadata {
        val values = buildMap {
            metaPattern.findAll(html).forEach { tag ->
                val attributes = attributePattern.findAll(tag.value).associate {
                    it.groupValues[1].lowercase() to decode(it.groupValues[3])
                }
                val key = attributes["property"]?.lowercase() ?: attributes["name"]?.lowercase()
                val content = attributes["content"]?.trim()?.takeIf(String::isNotEmpty)
                if (key != null && content != null && key !in this) put(key, content)
            }
        }
        val fallbackTitle = titlePattern.find(html)?.groupValues?.getOrNull(1)
            ?.replace(Regex("<[^>]+>"), " ")
            ?.let(::decode)
            ?.trim()
            ?.takeIf(String::isNotEmpty)
        return OpenGraphMetadata(
            title = values["og:title"] ?: fallbackTitle,
            description = values["og:description"] ?: values["description"],
            imageUrl = values["og:image"],
        )
    }

    private fun decode(value: String): String = value
        .replace("&amp;", "&", ignoreCase = true)
        .replace("&quot;", "\"", ignoreCase = true)
        .replace("&#39;", "'", ignoreCase = true)
        .replace("&lt;", "<", ignoreCase = true)
        .replace("&gt;", ">", ignoreCase = true)
}
