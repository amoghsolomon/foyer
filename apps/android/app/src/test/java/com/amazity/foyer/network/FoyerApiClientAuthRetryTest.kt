package com.amazity.foyer.network

import com.amazity.foyer.auth.AccessTokenProvider
import java.util.concurrent.atomic.AtomicInteger
import kotlinx.coroutines.runBlocking
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Test

class FoyerApiClientAuthRetryTest {
    @Test
    fun retriesUnauthorizedRequestOnceWithFreshToken() = runBlocking {
        val tokens = FakeTokens(listOf("stale", "fresh"))
        val transport = ScriptedTransport(
            ApiResponse(401, JSONObject().put("error", JSONObject().put("code", "unauthenticated")), emptyMap()),
            ApiResponse(200, JSONObject().put("ok", true), emptyMap()),
        )
        val client = FoyerApiClient(tokens, baseUrl = "https://foyer.test", transport = transport)
        val body = client.request("/v1/notes").body
        assertEquals(true, body?.optBoolean("ok"))
        assertEquals(listOf("stale", "fresh"), tokens.issued)
        assertEquals(2, transport.calls.get())
        assertEquals(1, tokens.invalidations.get())
    }

    @Test
    fun doesNotRetryASecondUnauthorizedResponse() = runBlocking {
        val tokens = FakeTokens(listOf("a", "b", "c"))
        val transport = ScriptedTransport(
            ApiResponse(401, JSONObject(), emptyMap()),
            ApiResponse(401, JSONObject(), emptyMap()),
        )
        val client = FoyerApiClient(tokens, baseUrl = "https://foyer.test", transport = transport)
        val response = client.request("/v1/notes")
        assertEquals(401, response.status)
        assertEquals(2, transport.calls.get())
        assertEquals(1, tokens.invalidations.get())
    }
}

private class FakeTokens(private val values: List<String>) : AccessTokenProvider {
    val issued = mutableListOf<String>()
    val invalidations = AtomicInteger(0)
    private var index = 0
    override suspend fun bearerToken(forceRefresh: Boolean): String {
        val token = values[index.coerceAtMost(values.lastIndex)]
        if (index < values.lastIndex) index += 1
        issued += token
        return token
    }
    override fun invalidateAccessToken() {
        invalidations.incrementAndGet()
    }
    override fun hasPreviousAccess(): Boolean = true
}

private class ScriptedTransport(private vararg val responses: ApiResponse) : FoyerHttpTransport {
    val calls = AtomicInteger(0)
    override fun exchange(request: FoyerHttpRequest): ApiResponse {
        val index = calls.getAndIncrement().coerceAtMost(responses.lastIndex)
        return responses[index]
    }
}
