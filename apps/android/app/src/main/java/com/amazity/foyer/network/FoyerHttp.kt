package com.amazity.foyer.network

import java.net.HttpURLConnection
import java.net.URL
import org.json.JSONObject

data class FoyerHttpRequest(
    val url: String,
    val method: String,
    val headers: Map<String, String> = emptyMap(),
    val body: ByteArray? = null,
    val connectTimeoutMillis: Int = 15_000,
    val readTimeoutMillis: Int = 45_000,
)

fun interface FoyerHttpTransport {
    fun exchange(request: FoyerHttpRequest): ApiResponse
}

object UrlConnectionTransport : FoyerHttpTransport {
    override fun exchange(request: FoyerHttpRequest): ApiResponse {
        val connection = URL(request.url).openConnection() as HttpURLConnection
        try {
            connection.requestMethod = request.method
            connection.connectTimeout = request.connectTimeoutMillis
            connection.readTimeout = request.readTimeoutMillis
            request.headers.forEach { (name, value) ->
                connection.setRequestProperty(name, value)
            }
            request.body?.let { payload ->
                connection.doOutput = true
                connection.outputStream.use { it.write(payload) }
            }
            val status = connection.responseCode
            val text = (if (status in 200..299) connection.inputStream else connection.errorStream)
                ?.bufferedReader()
                ?.use { it.readText() }
                .orEmpty()
            return ApiResponse(
                status = status,
                body = text.takeIf(String::isNotBlank)?.let { runCatching { JSONObject(it) }.getOrNull() },
                headers = connection.headerFields,
            )
        } finally {
            connection.disconnect()
        }
    }
}
