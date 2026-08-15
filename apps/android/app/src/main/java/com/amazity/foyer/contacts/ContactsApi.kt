package com.amazity.foyer.contacts

import android.net.Uri
import com.amazity.foyer.model.AddressBook
import com.amazity.foyer.model.Contact
import com.amazity.foyer.model.ContactEmail
import com.amazity.foyer.model.ContactPhone
import com.amazity.foyer.model.ContactPostalAddress
import com.amazity.foyer.model.StructuredContactName
import com.amazity.foyer.network.ApiException
import com.amazity.foyer.network.FoyerApiClient
import java.util.UUID
import org.json.JSONArray
import org.json.JSONObject

data class ContactsSyncCredentials(
    val endpoint: String,
    val token: String,
    val userId: String,
    val expiresAt: String,
)

class ContactsConflictException(
    val code: String,
    val detail: String,
) : IllegalStateException(detail) {
    fun publicMessage(): String = when (code) {
        "stale_etag", "stale_revision" ->
            "This contact changed on another device. The server copy will replace the rejected edit."
        "address_book_not_empty" ->
            "Address book is not empty. Move or delete its contacts first."
        "invalid_parent" -> "That address book destination is not valid."
        "gone" -> "This item was deleted on the server and cannot be restored."
        else -> detail
    }
}

class ContactsApi(private val api: FoyerApiClient) {
    suspend fun syncCredentials(): ContactsSyncCredentials {
        val body = api.request("/v1/sync/credentials").requireJson()
        return ContactsSyncCredentials(
            endpoint = body.getString("endpoint"),
            token = body.getString("token"),
            userId = body.optString("userId"),
            expiresAt = body.optString("expiresAt"),
        )
    }

    suspend fun addressBooks(): List<AddressBook> {
        val body = api.request("/v1/address-books").requireJson()
        return body.optJSONArray("addressBooks").objects().map(::addressBook)
    }

    suspend fun contacts(addressBookId: String? = null): List<Contact> {
        val path = if (addressBookId.isNullOrBlank()) {
            "/v1/contacts"
        } else {
            "/v1/contacts?addressBookId=${Uri.encode(addressBookId)}"
        }
        val body = api.request(path).requireJson()
        return body.optJSONArray("contacts").objects().map(::contact)
    }

    suspend fun createAddressBook(
        id: String = UUID.randomUUID().toString(),
        operationId: String = UUID.randomUUID().toString(),
        displayName: String,
        description: String? = null,
    ): AddressBook = mutate(
        "/v1/address-books",
        JSONObject()
            .put("operationId", operationId)
            .put("id", id)
            .put("displayName", displayName)
            .put("description", description ?: JSONObject.NULL),
    ).let(::addressBook)

    suspend fun updateAddressBook(
        id: String,
        expectedEtag: String?,
        expectedRevision: Long,
        displayName: String,
        operationId: String = UUID.randomUUID().toString(),
    ): AddressBook = mutate(
        "/v1/address-books/${Uri.encode(id)}/update",
        JSONObject()
            .put("operationId", operationId)
            .put("expectedEtag", expectedEtag ?: JSONObject.NULL)
            .put("expectedRevision", expectedRevision)
            .put("displayName", displayName),
    ).let(::addressBook)

    suspend fun deleteAddressBook(
        id: String,
        expectedEtag: String?,
        expectedRevision: Long,
        operationId: String = UUID.randomUUID().toString(),
    ): AddressBook = mutate(
        "/v1/address-books/${Uri.encode(id)}/delete",
        JSONObject()
            .put("operationId", operationId)
            .put("expectedEtag", expectedEtag ?: JSONObject.NULL)
            .put("expectedRevision", expectedRevision),
    ).let(::addressBook)

    suspend fun createContact(
        id: String = UUID.randomUUID().toString(),
        operationId: String = UUID.randomUUID().toString(),
        addressBookId: String,
        draft: ContactDraft,
    ): Contact = mutate(
        "/v1/contacts",
        contactPayload(operationId, draft).put("id", id).put("addressBookId", addressBookId),
    ).let(::contact)

    suspend fun updateContact(
        id: String,
        expectedEtag: String?,
        expectedRevision: Long,
        draft: ContactDraft,
        operationId: String = UUID.randomUUID().toString(),
    ): Contact = mutate(
        "/v1/contacts/${Uri.encode(id)}/update",
        contactPayload(operationId, draft)
            .put("expectedEtag", expectedEtag ?: JSONObject.NULL)
            .put("expectedRevision", expectedRevision),
    ).let(::contact)

    suspend fun moveContact(
        id: String,
        expectedEtag: String?,
        expectedRevision: Long,
        addressBookId: String,
        operationId: String = UUID.randomUUID().toString(),
    ): Contact = mutate(
        "/v1/contacts/${Uri.encode(id)}/move",
        JSONObject()
            .put("operationId", operationId)
            .put("expectedEtag", expectedEtag ?: JSONObject.NULL)
            .put("expectedRevision", expectedRevision)
            .put("addressBookId", addressBookId),
    ).let(::contact)

    suspend fun deleteContact(
        id: String,
        expectedEtag: String?,
        expectedRevision: Long,
        operationId: String = UUID.randomUUID().toString(),
    ): Contact = mutate(
        "/v1/contacts/${Uri.encode(id)}/delete",
        JSONObject()
            .put("operationId", operationId)
            .put("expectedEtag", expectedEtag ?: JSONObject.NULL)
            .put("expectedRevision", expectedRevision),
    ).let(::contact)

    private suspend fun mutate(path: String, payload: JSONObject): JSONObject {
        val response = api.request(path, "POST", payload)
        if (!response.successful) {
            val code = response.body?.optJSONObject("error")?.optString("code").orEmpty()
            val message = response.body?.optJSONObject("error")?.optString("message")
                ?: response.body?.toString()
                ?: "Foyer contacts request failed (${response.status})"
            if (
                code == "stale_etag" || code == "stale_revision" || code == "conflict" ||
                code == "invalid_parent" || code == "address_book_not_empty" || code == "gone"
            ) {
                throw ContactsConflictException(code, message)
            }
            throw ApiException(response.status, message)
        }
        return response.body ?: JSONObject()
    }
}

private fun contactPayload(operationId: String, draft: ContactDraft): JSONObject {
    val value = draft.normalized()
    return JSONObject()
        .put("operationId", operationId)
        .put("displayName", value.displayName)
        .put(
            "name",
            JSONObject()
                .put("familyName", value.name.familyName)
                .put("givenName", value.name.givenName)
                .put("additionalNames", value.name.additionalNames)
                .put("honorificPrefix", value.name.honorificPrefix)
                .put("honorificSuffix", value.name.honorificSuffix),
        )
        .put("emails", JSONArray(value.emails.map { email ->
            JSONObject().put("value", email.value).put("type", email.type).put("pref", email.pref)
        }))
        .put("phones", JSONArray(value.phones.map { phone ->
            JSONObject().put("value", phone.value).put("type", phone.type).put("pref", phone.pref)
        }))
        .put("organization", value.organization)
        .put("jobTitle", value.jobTitle)
        .put("addresses", JSONArray(value.addresses.map { address ->
            JSONObject()
                .put("poBox", address.poBox)
                .put("extended", address.extended)
                .put("street", address.street)
                .put("locality", address.locality)
                .put("region", address.region)
                .put("postalCode", address.postalCode)
                .put("country", address.country)
                .put("type", address.type)
                .put("pref", address.pref)
        }))
        .put("birthday", value.birthday ?: JSONObject.NULL)
        .put("notes", value.notes)
}

private fun addressBook(value: JSONObject) = AddressBook(
    id = value.getString("id"),
    uid = value.optString("uid").ifBlank { value.getString("id") },
    href = value.optString("href"),
    etag = value.optionalText("etag"),
    displayName = value.getString("displayName"),
    description = value.optString("description"),
    revision = value.optLong("revision", 1L),
)

private fun contact(value: JSONObject) = Contact(
    id = value.getString("id"),
    addressBookId = value.getString("addressBookId"),
    uid = value.optString("uid"),
    href = value.optString("href"),
    etag = value.optString("etag"),
    displayName = value.getString("displayName"),
    name = value.optJSONObject("name")?.let { name ->
        StructuredContactName(
            familyName = name.optString("familyName"),
            givenName = name.optString("givenName"),
            additionalNames = name.optString("additionalNames"),
            honorificPrefix = name.optString("honorificPrefix"),
            honorificSuffix = name.optString("honorificSuffix"),
        )
    } ?: StructuredContactName(),
    emails = value.optJSONArray("emails").objects().map {
        ContactEmail(it.optString("value"), it.optString("type").ifBlank { "other" }, it.optBoolean("pref"))
    },
    phones = value.optJSONArray("phones").objects().map {
        ContactPhone(it.optString("value"), it.optString("type").ifBlank { "other" }, it.optBoolean("pref"))
    },
    organization = value.optString("organization"),
    jobTitle = value.optString("jobTitle"),
    addresses = value.optJSONArray("addresses").objects().map {
        ContactPostalAddress(
            poBox = it.optString("poBox"),
            extended = it.optString("extended"),
            street = it.optString("street"),
            locality = it.optString("locality"),
            region = it.optString("region"),
            postalCode = it.optString("postalCode"),
            country = it.optString("country"),
            type = it.optString("type").ifBlank { "other" },
            pref = it.optBoolean("pref"),
        )
    },
    birthday = value.optionalText("birthday"),
    notes = value.optString("notes"),
    revision = value.optLong("revision", 1L),
    updatedAt = value.optString("updatedAt"),
)

private fun JSONObject.optionalText(key: String): String? =
    if (isNull(key)) null else optString(key).takeIf(String::isNotBlank)

private fun JSONArray?.objects(): List<JSONObject> = buildList {
    val array = this@objects ?: return@buildList
    for (index in 0 until array.length()) array.optJSONObject(index)?.let(::add)
}

private fun com.amazity.foyer.network.ApiResponse.requireJson(): JSONObject {
    if (!successful) throw ApiException(status, body?.toString())
    return body ?: JSONObject()
}
