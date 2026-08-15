package com.amazity.foyer.network

import android.net.Uri
import com.amazity.foyer.BuildConfig
import com.amazity.foyer.auth.AccessTokenProvider
import com.amazity.foyer.data.PendingMutation
import com.amazity.foyer.data.PendingNotification
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.json.JSONArray
import org.json.JSONObject

data class ApiResponse(
    val status: Int,
    val body: JSONObject?,
    val headers: Map<String, List<String>>,
) {
    val successful: Boolean get() = status in 200..299
}

class FoyerApiClient(
    private val tokens: AccessTokenProvider,
    private val baseUrl: String = BuildConfig.FOYER_API_BASE_URL,
    private val transport: FoyerHttpTransport = UrlConnectionTransport,
) {
    suspend fun changes(cursor: Long): JSONObject = request(
        path = "/api/sync/changes?cursor=$cursor",
    ).requireJson()

    suspend fun agenda(from: String, to: String): JSONObject = request(
        path = "/api/agenda?from=${Uri.encode(from)}&to=${Uri.encode(to)}",
    ).requireJson()

    suspend fun homeBriefing(timezone: String): JSONObject = request(
        "/api/home-briefing?timezone=${Uri.encode(timezone)}",
    ).requireJson()

    suspend fun settings(): JSONObject = request("/api/settings").requireJson()

    suspend fun profile(): JSONObject = request("/api/profile").requireJson()

    suspend fun memories(limit: Int = 30, cursor: String? = null): JSONObject = request(
        buildString {
            append("/api/memories?limit=")
            append(limit.coerceIn(1, 100))
            cursor?.takeIf(String::isNotBlank)?.let {
                append("&cursor=")
                append(Uri.encode(it))
            }
        },
    ).requireJson()

    suspend fun deleteMemory(id: String) {
        val response = request("/api/memories/${Uri.encode(id)}", "DELETE")
        if (!response.successful) throw ApiException(response.status, response.body?.toString())
    }

    suspend fun updateSettings(
        timezone: String? = null,
        notificationWhitelist: List<String>? = null,
    ): JSONObject {
        val payload = JSONObject()
        timezone?.let { payload.put("timezone", it) }
        notificationWhitelist?.let { payload.put("notificationWhitelist", JSONArray(it)) }
        return request("/api/settings", "PUT", payload).requireJson()
    }

    suspend fun sendMutations(mutations: List<PendingMutation>): JSONObject {
        val payload = JSONObject().put(
            "mutations",
            JSONArray().apply {
                mutations.forEach { mutation ->
                    put(
                        JSONObject()
                            .put("mutationId", mutation.mutationId)
                            .put("deviceId", mutation.deviceId)
                            .put("entityType", mutation.entityType)
                            .put("entityId", mutation.entityId)
                            .put("operation", mutation.operation)
                            .put(
                                "payload",
                                mutation.payloadJson?.let(::JSONObject) ?: JSONObject.NULL,
                            ),
                    )
                }
            },
        )
        return request("/api/sync/mutations", "POST", payload).requireJson()
    }

    suspend fun sendNotifications(notifications: List<PendingNotification>): JSONObject {
        val payload = JSONArray().apply {
            notifications.forEach { notification ->
                put(
                    JSONObject()
                        .put("id", notification.id)
                        .put("appPackage", notification.appPackage)
                        .put("title", notification.title ?: JSONObject.NULL)
                        .put("body", notification.body)
                        .put("postedAt", notification.postedAt)
                        .put("redacted", notification.redacted),
                )
            }
        }
        return request(
            path = "/api/notifications/batch",
            method = "POST",
            jsonArray = payload,
        ).requireJson()
    }

    suspend fun notes(): JSONObject = request("/api/notes").requireJson()

    suspend fun createNote(
        title: String,
        body: String,
        folderId: String,
        tags: List<String> = emptyList(),
    ): JSONObject = request(
        "/api/notes",
        "POST",
        JSONObject()
            .put("title", title)
            .put("body", body)
            .put("folderId", folderId)
            .put("tags", JSONArray(tags)),
    ).requireJson().getJSONObject("note")

    suspend fun updateNote(
        id: String,
        version: Long,
        title: String,
        body: String,
        folderId: String,
        tags: List<String>,
    ): JSONObject = request(
        "/api/notes/${Uri.encode(id)}",
        "PATCH",
        JSONObject()
            .put("version", version)
            .put("title", title)
            .put("body", body)
            .put("folderId", folderId)
            .put("tags", JSONArray(tags)),
    ).requireJson().getJSONObject("note")

    suspend fun deleteNote(id: String, version: Long) {
        val response = request("/api/notes/${Uri.encode(id)}?version=$version", "DELETE")
        if (!response.successful) throw ApiException(response.status, response.body?.toString())
    }

    suspend fun sendChat(message: String): JSONObject = request(
        "/api/chat",
        "POST",
        JSONObject().put("message", message),
    ).requireJson()

    suspend fun activities(query: String = ""): JSONObject = request(
        "/api/activities" + query.trim().takeIf(String::isNotEmpty)?.let {
            "?q=${Uri.encode(it)}"
        }.orEmpty(),
    ).requireJson()

    suspend fun activity(id: String): JSONObject = request(
        "/api/activities/${Uri.encode(id)}",
    ).requireJson()

    suspend fun createActivity(
        message: String,
        timezone: String = "UTC",
    ): JSONObject = request(
        "/api/activities",
        "POST",
        JSONObject().put("message", message).put("timezone", timezone),
    ).requireJson()

    suspend fun renameActivity(id: String, title: String): JSONObject = request(
        "/api/activities/${Uri.encode(id)}",
        "PATCH",
        JSONObject().put("title", title),
    ).requireJson()

    suspend fun archiveActivity(id: String) {
        val response = request("/api/activities/${Uri.encode(id)}/archive", "POST", JSONObject())
        if (!response.successful) throw ApiException(response.status, response.body?.toString())
    }

    suspend fun deleteActivity(id: String) {
        val response = request("/api/activities/${Uri.encode(id)}", "DELETE")
        if (!response.successful) throw ApiException(response.status, response.body?.toString())
    }

    suspend fun sendActivityMessage(
        id: String,
        message: String,
        timezone: String = java.time.ZoneId.systemDefault().id,
    ): JSONObject = request(
        "/api/activities/${Uri.encode(id)}/messages",
        "POST",
        JSONObject().put("message", message).put("timezone", timezone),
    ).requireJson()

    suspend fun scheduleActivity(
        id: String,
        runAt: String,
        frequency: String,
        interval: Int,
        timezone: String,
    ): JSONObject = request(
        "/api/activities/${Uri.encode(id)}/schedule",
        "PUT",
        JSONObject()
            .put("runAt", runAt)
            .put("frequency", frequency)
            .put("interval", interval)
            .put("timezone", timezone),
    ).requireJson()

    suspend fun cancelActivitySchedule(id: String) {
        val response = request("/api/activities/${Uri.encode(id)}/schedule", "DELETE")
        if (!response.successful) throw ApiException(response.status, response.body?.toString())
    }

    suspend fun runActivityNow(
        id: String,
        timezone: String = java.time.ZoneId.systemDefault().id,
    ): JSONObject = request(
        "/api/activities/${Uri.encode(id)}/run",
        "POST",
        JSONObject().put("timezone", timezone),
    ).requireJson()

    suspend fun retryActivityRun(id: String, runId: String): JSONObject = request(
        "/api/activities/${Uri.encode(id)}/runs/${Uri.encode(runId)}/retry",
        "POST",
        JSONObject(),
    ).requireJson()

    suspend fun sendBackgroundTask(message: String): JSONObject = request(
        "/api/chat",
        "POST",
        JSONObject().put(
            "message",
            "BACKGROUND TASK — execute fully using cloud tools and do not emit " +
                "client-action directives:\n$message",
        ),
    ).requireJson()

    suspend fun assistantTurn(message: String): JSONObject {
        val principal = request("/api/session").requireJson().getJSONObject("user")
        val userId = principal.getString("userId")
        return request(
            path = "/api/flue/agents/foyer/${Uri.encode(userId)}?wait=result",
            method = "POST",
            json = JSONObject().put("message", message),
            readTimeoutMillis = 120_000,
        ).requireJson()
    }

    suspend fun validateSession(): JSONObject = request("/v1/session").requireJson()

    suspend fun grokStatus(): JSONObject = request(
        "/api/integrations/grok",
    ).requireJson()

    suspend fun startGrokDeviceLogin(): JSONObject = request(
        "/api/integrations/grok/device/start",
        "POST",
        JSONObject(),
    ).requireJson()

    suspend fun pollGrokDeviceLogin(flowId: String): JSONObject = request(
        "/api/integrations/grok/device/poll",
        "POST",
        JSONObject().put("flowId", flowId),
    ).requireJson()

    suspend fun request(
        path: String,
        method: String = "GET",
        json: JSONObject? = null,
        jsonArray: JSONArray? = null,
        authenticated: Boolean = true,
        readTimeoutMillis: Int = 45_000,
    ): ApiResponse = withContext(Dispatchers.IO) {
        val requestBody = json?.toString() ?: jsonArray?.toString()
        var attemptedRefresh = false
        while (true) {
            val headers = linkedMapOf(
                "accept" to "application/json",
                "origin" to baseUrl.trimEnd('/'),
            )
            if (authenticated) {
                headers["authorization"] = "Bearer ${tokens.bearerToken(forceRefresh = attemptedRefresh)}"
            }
            if (requestBody != null) {
                headers["content-type"] = "application/json"
            }
            val response = transport.exchange(
                FoyerHttpRequest(
                    url = baseUrl.trimEnd('/') + path,
                    method = method,
                    headers = headers,
                    body = requestBody?.encodeToByteArray(),
                    readTimeoutMillis = readTimeoutMillis,
                ),
            )
            if (authenticated && response.status == 401 && !attemptedRefresh) {
                tokens.invalidateAccessToken()
                attemptedRefresh = true
                continue
            }
            return@withContext response
        }
        @Suppress("UNREACHABLE_CODE")
        error("unreachable")
    }
}

class ApiException(val status: Int, detail: String?) :
    IllegalStateException("Foyer API request failed ($status): ${detail.orEmpty()}")

private fun ApiResponse.requireJson(): JSONObject {
    if (!successful) throw ApiException(status, body?.toString())
    return body ?: JSONObject()
}
