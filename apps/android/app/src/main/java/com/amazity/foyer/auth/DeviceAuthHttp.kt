package com.amazity.foyer.auth

import com.amazity.foyer.BuildConfig
import com.amazity.foyer.network.ApiException
import com.amazity.foyer.network.ApiResponse
import com.amazity.foyer.network.FoyerHttpRequest
import com.amazity.foyer.network.FoyerHttpTransport
import com.amazity.foyer.network.UrlConnectionTransport
import java.time.Instant
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.json.JSONObject

class DeviceAuthHttp(
    private val baseUrl: String = BuildConfig.FOYER_API_BASE_URL,
    private val transport: FoyerHttpTransport = UrlConnectionTransport,
) : DeviceAuthTransport {
    override suspend fun createChallenge(deviceKeyId: String): AuthChallenge {
        val body = post(
            "/v1/auth/challenges",
            JSONObject().put("deviceKeyId", deviceKeyId),
        )
        return AuthChallenge(
            challengeId = body.getString("challengeId"),
            signingPayload = body.getString("signingPayload"),
            expiresAt = parseInstant(body.getString("expiresAt")),
        )
    }

    override suspend fun createSession(challengeId: String, signature: String): AuthSession {
        val body = post(
            "/v1/auth/sessions",
            JSONObject()
                .put("challengeId", challengeId)
                .put("signature", signature),
        )
        return AuthSession(
            accessToken = body.getString("accessToken"),
            tokenType = body.optString("tokenType", "Bearer"),
            expiresAt = parseInstant(body.getString("expiresAt")),
            userId = body.getString("userId"),
            deviceKeyId = body.getString("deviceKeyId"),
        )
    }

    override suspend fun validateAccessToken(accessToken: String) {
        val response = withContext(Dispatchers.IO) {
            transport.exchange(
                FoyerHttpRequest(
                    url = url("/v1/session"),
                    method = "GET",
                    headers = mapOf(
                        "accept" to "application/json",
                        "authorization" to "Bearer $accessToken",
                    ),
                ),
            )
        }
        if (!response.successful) {
            throw ApiException(response.status, safeDetail(response))
        }
    }

    private suspend fun post(path: String, json: JSONObject): JSONObject {
        val response = withContext(Dispatchers.IO) {
            transport.exchange(
                FoyerHttpRequest(
                    url = url(path),
                    method = "POST",
                    headers = mapOf(
                        "accept" to "application/json",
                        "content-type" to "application/json",
                    ),
                    body = json.toString().encodeToByteArray(),
                ),
            )
        }
        if (!response.successful) {
            throw ApiException(response.status, safeDetail(response))
        }
        return response.body ?: JSONObject()
    }

    private fun url(path: String): String = baseUrl.trimEnd('/') + path

    private fun parseInstant(value: String): Instant = Instant.parse(value)

    private fun safeDetail(response: ApiResponse): String {
        val code = response.body?.optJSONObject("error")?.optString("code").orEmpty()
        return code.ifBlank { "request failed" }
    }
}
