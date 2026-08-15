package com.amazity.foyer.auth

import com.amazity.foyer.network.ApiException
import com.amazity.foyer.network.ApiResponse
import com.amazity.foyer.network.FoyerHttpRequest
import com.amazity.foyer.network.FoyerHttpTransport
import java.time.Instant
import java.util.concurrent.atomic.AtomicInteger
import kotlinx.coroutines.runBlocking
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class DeviceAuthHttpTest {
    @Test
    fun postsCamelCaseChallengeAndParsesResponse() = runBlocking {
        val transport = RecordingTransport(
            ApiResponse(
                200,
                JSONObject()
                    .put("challengeId", "challenge-1")
                    .put("signingPayload", "cGF5bG9hZA")
                    .put("expiresAt", "2026-08-15T12:01:00Z"),
                emptyMap(),
            ),
        )
        val http = DeviceAuthHttp(baseUrl = "https://foyer.test", transport = transport)
        val challenge = http.createChallenge("cn-I_WNMClehiVp51i_0VpOENW1upEerA8sEam5hn-s")
        assertEquals("challenge-1", challenge.challengeId)
        assertEquals("cGF5bG9hZA", challenge.signingPayload)
        assertEquals(Instant.parse("2026-08-15T12:01:00Z"), challenge.expiresAt)
        val request = transport.requests.single()
        assertEquals("https://foyer.test/v1/auth/challenges", request.url)
        assertEquals("POST", request.method)
        val body = JSONObject(request.body!!.decodeToString())
        assertEquals("cn-I_WNMClehiVp51i_0VpOENW1upEerA8sEam5hn-s", body.getString("deviceKeyId"))
        assertFalse(request.headers.containsKey("authorization"))
    }

    @Test
    fun postsSessionSignatureAndParsesBearerResponse() = runBlocking {
        val transport = RecordingTransport(
            ApiResponse(
                200,
                JSONObject()
                    .put("accessToken", "access-token")
                    .put("tokenType", "Bearer")
                    .put("expiresAt", "2026-08-15T12:05:00Z")
                    .put("userId", "user-1")
                    .put("deviceKeyId", "cn-I_WNMClehiVp51i_0VpOENW1upEerA8sEam5hn-s"),
                emptyMap(),
            ),
        )
        val http = DeviceAuthHttp(baseUrl = "https://foyer.test/", transport = transport)
        val session = http.createSession("challenge-1", "signature-p1363")
        assertEquals("access-token", session.accessToken)
        assertEquals("Bearer", session.tokenType)
        assertEquals("user-1", session.userId)
        assertEquals("cn-I_WNMClehiVp51i_0VpOENW1upEerA8sEam5hn-s", session.deviceKeyId)
        val request = transport.requests.single()
        assertEquals("https://foyer.test/v1/auth/sessions", request.url)
        val body = JSONObject(request.body!!.decodeToString())
        assertEquals("challenge-1", body.getString("challengeId"))
        assertEquals("signature-p1363", body.getString("signature"))
    }

    @Test
    fun failedChallengeExposesOnlySafeDetail() = runBlocking {
        val transport = RecordingTransport(
            ApiResponse(
                401,
                JSONObject().put(
                    "error",
                    JSONObject().put("code", "unknown_device").put("message", "secret-token-value"),
                ),
                emptyMap(),
            ),
        )
        val http = DeviceAuthHttp(baseUrl = "https://foyer.test", transport = transport)
        val error = runCatching { http.createChallenge("device") }.exceptionOrNull() as ApiException
        assertEquals(401, error.status)
        assertTrue(error.message!!.contains("unknown_device"))
        assertFalse(error.message!!.contains("secret-token-value"))
    }
}

private class RecordingTransport(private vararg val responses: ApiResponse) : FoyerHttpTransport {
    val requests = mutableListOf<FoyerHttpRequest>()
    private val index = AtomicInteger(0)

    override fun exchange(request: FoyerHttpRequest): ApiResponse {
        requests += request
        return responses[index.getAndIncrement().coerceAtMost(responses.lastIndex)]
    }
}
