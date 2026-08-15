package com.amazity.foyer.calendar

import android.net.Uri
import com.amazity.foyer.model.EventDraft
import com.amazity.foyer.model.FoyerCalendar
import com.amazity.foyer.model.FoyerEvent
import com.amazity.foyer.network.ApiException
import com.amazity.foyer.network.FoyerApiClient
import java.util.UUID
import org.json.JSONArray
import org.json.JSONObject

class CalendarConflictException(
    val code: String,
    val detail: String,
) : IllegalStateException(detail) {
    fun publicMessage(): String = when (code) {
        "stale_etag", "stale_revision" ->
            "Someone else changed this item. The server copy will replace the rejected edit."
        "gone" -> "This item was deleted on the server and cannot be restored."
        "conflict" -> detail
        else -> detail
    }
}

data class CalendarSyncCredentials(
    val endpoint: String,
    val token: String,
    val userId: String,
)

class CalendarApi(private val api: FoyerApiClient) {
    suspend fun syncCredentials(): CalendarSyncCredentials {
        val body = api.request("/v1/sync/credentials").requireJson()
        return CalendarSyncCredentials(
            endpoint = body.getString("endpoint"),
            token = body.getString("token"),
            userId = body.getString("userId"),
        )
    }

    suspend fun calendars(): List<FoyerCalendar> {
        val body = api.request("/v1/calendars").requireJson()
        return body.optJSONArray("calendars").objects().map(::calendarRecord)
    }

    suspend fun events(calendarId: String? = null): List<FoyerEvent> {
        val path = if (calendarId.isNullOrBlank()) {
            "/v1/events"
        } else {
            "/v1/events?calendarId=${Uri.encode(calendarId)}"
        }
        val body = api.request(path).requireJson()
        return body.optJSONArray("events").objects().map(::eventRecord)
    }

    suspend fun createCalendar(
        id: String = UUID.randomUUID().toString(),
        operationId: String = UUID.randomUUID().toString(),
        displayName: String,
        description: String = "",
        color: String? = null,
    ): FoyerCalendar = mutate(
        "/v1/calendars",
        JSONObject()
            .put("operationId", operationId)
            .put("id", id)
            .put("displayName", displayName)
            .put("description", description)
            .put("color", color ?: JSONObject.NULL),
    ).let(::calendarRecord)

    suspend fun renameCalendar(
        id: String,
        expectedRevision: Long,
        expectedEtag: String?,
        displayName: String,
        operationId: String = UUID.randomUUID().toString(),
    ): FoyerCalendar = mutate(
        "/v1/calendars/${Uri.encode(id)}/rename",
        JSONObject()
            .put("operationId", operationId)
            .put("expectedRevision", expectedRevision)
            .put("expectedEtag", expectedEtag ?: JSONObject.NULL)
            .put("displayName", displayName),
    ).let(::calendarRecord)

    suspend fun deleteCalendar(
        id: String,
        expectedRevision: Long,
        expectedEtag: String?,
        operationId: String = UUID.randomUUID().toString(),
    ): FoyerCalendar = mutate(
        "/v1/calendars/${Uri.encode(id)}/delete",
        JSONObject()
            .put("operationId", operationId)
            .put("expectedRevision", expectedRevision)
            .put("expectedEtag", expectedEtag ?: JSONObject.NULL),
    ).let(::calendarRecord)

    suspend fun createEvent(
        id: String = UUID.randomUUID().toString(),
        operationId: String = UUID.randomUUID().toString(),
        uid: String? = null,
        draft: EventDraft,
    ): FoyerEvent = mutate("/v1/events", eventPayload(operationId, id, uid, draft, null, null))
        .let(::eventRecord)

    suspend fun updateEvent(
        id: String,
        expectedRevision: Long,
        expectedEtag: String?,
        draft: EventDraft,
        operationId: String = UUID.randomUUID().toString(),
    ): FoyerEvent = mutate(
        "/v1/events/${Uri.encode(id)}/update",
        eventPayload(operationId, id, null, draft, expectedRevision, expectedEtag),
    ).let(::eventRecord)

    suspend fun moveEvent(
        id: String,
        expectedRevision: Long,
        expectedEtag: String?,
        calendarId: String,
        operationId: String = UUID.randomUUID().toString(),
    ): FoyerEvent = mutate(
        "/v1/events/${Uri.encode(id)}/move",
        JSONObject()
            .put("operationId", operationId)
            .put("expectedRevision", expectedRevision)
            .put("expectedEtag", expectedEtag ?: JSONObject.NULL)
            .put("calendarId", calendarId),
    ).let(::eventRecord)

    suspend fun deleteEvent(
        id: String,
        expectedRevision: Long,
        expectedEtag: String?,
        operationId: String = UUID.randomUUID().toString(),
    ): FoyerEvent = mutate(
        "/v1/events/${Uri.encode(id)}/delete",
        JSONObject()
            .put("operationId", operationId)
            .put("expectedRevision", expectedRevision)
            .put("expectedEtag", expectedEtag ?: JSONObject.NULL),
    ).let(::eventRecord)

    private suspend fun mutate(path: String, payload: JSONObject): JSONObject {
        val response = api.request(path, "POST", payload)
        if (!response.successful) {
            val code = response.body?.optJSONObject("error")?.optString("code").orEmpty()
            val message = response.body?.optJSONObject("error")?.optString("message")
                ?: response.body?.toString()
                ?: "Foyer calendar request failed (${response.status})"
            if (code == "stale_revision" || code == "stale_etag" || code == "conflict" || code == "gone") {
                throw CalendarConflictException(code, message)
            }
            throw ApiException(response.status, message)
        }
        return response.body ?: JSONObject()
    }
}

private fun eventPayload(
    operationId: String,
    id: String,
    uid: String?,
    draft: EventDraft,
    expectedRevision: Long?,
    expectedEtag: String?,
): JSONObject = JSONObject()
    .put("operationId", operationId)
    .put("id", id)
    .put("calendarId", draft.calendarId)
    .put("uid", uid ?: JSONObject.NULL)
    .put("summary", draft.summary)
    .put("description", draft.description)
    .put("location", draft.location)
    .put("allDay", draft.allDay)
    .put("dtstart", draft.dtstart)
    .put("dtend", draft.dtend ?: JSONObject.NULL)
    .put("tzid", draft.tzid ?: JSONObject.NULL)
    .put("rrule", draft.rrule ?: JSONObject.NULL)
    .put("exdates", JSONArray(draft.exdates))
    .apply {
        if (expectedRevision != null) put("expectedRevision", expectedRevision)
        if (expectedEtag != null) put("expectedEtag", expectedEtag)
    }

private fun calendarRecord(value: JSONObject) = FoyerCalendar(
    id = value.getString("id"),
    uid = value.optString("uid").ifBlank { value.getString("id") },
    href = value.optString("href"),
    etag = value.optString("etag"),
    displayName = value.optString("displayName", value.optString("display_name")),
    description = value.optString("description"),
    color = value.optional("color"),
    revision = value.optLong("revision", 1L),
    createdAt = value.optString("createdAt", value.optString("created_at")),
    updatedAt = value.optString("updatedAt", value.optString("updated_at")),
)

private fun eventRecord(value: JSONObject) = FoyerEvent(
    id = value.getString("id"),
    calendarId = value.optString("calendarId", value.optString("calendar_id")),
    uid = value.optString("uid"),
    href = value.optString("href"),
    etag = value.optString("etag"),
    summary = value.optString("summary"),
    description = value.optString("description"),
    location = value.optString("location"),
    allDay = value.optBoolean("allDay", value.optInt("all_day") == 1),
    dtstart = value.optString("dtstart"),
    dtend = value.optional("dtend"),
    tzid = value.optional("tzid"),
    rrule = value.optional("rrule"),
    exdates = value.optString("exdates", "[]").ifBlank { "[]" },
    revision = value.optLong("revision", 1L),
    createdAt = value.optString("createdAt", value.optString("created_at")),
    updatedAt = value.optString("updatedAt", value.optString("updated_at")),
)

private fun JSONObject.optional(key: String): String? =
    if (isNull(key)) null else optString(key).takeIf(String::isNotBlank)

private fun JSONArray?.objects(): List<JSONObject> = buildList {
    val array = this@objects ?: return@buildList
    for (index in 0 until array.length()) array.optJSONObject(index)?.let(::add)
}

private fun com.amazity.foyer.network.ApiResponse.requireJson(): JSONObject {
    if (!successful) throw ApiException(status, body?.toString())
    return body ?: JSONObject()
}
