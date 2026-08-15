package com.amazity.foyer.contacts

import com.amazity.foyer.model.Contact
import com.amazity.foyer.model.ContactEmail
import com.amazity.foyer.model.ContactPhone
import com.amazity.foyer.model.ContactPostalAddress
import com.amazity.foyer.model.StructuredContactName

const val MAX_DISPLAY_NAME = 200
const val MAX_NAME_PART = 100
const val MAX_ORGANIZATION = 200
const val MAX_JOB_TITLE = 200
const val MAX_EMAIL = 254
const val MAX_PHONE = 64
const val MAX_ADDRESS_LINE = 200
const val MAX_NOTE_CHARS = 16_384
const val MAX_EMAILS = 16
const val MAX_PHONES = 16
const val MAX_ADDRESSES = 8
const val MAX_BOOK_NAME = 80

data class ContactDraft(
    val displayName: String = "",
    val name: StructuredContactName = StructuredContactName(),
    val emails: List<ContactEmail> = emptyList(),
    val phones: List<ContactPhone> = emptyList(),
    val organization: String = "",
    val jobTitle: String = "",
    val addresses: List<ContactPostalAddress> = emptyList(),
    val birthday: String? = null,
    val notes: String = "",
    val addressBookId: String = "",
)

fun Contact.toDraft(): ContactDraft = ContactDraft(
    displayName = displayName,
    name = name,
    emails = emails.ifEmpty { listOf(ContactEmail("")) },
    phones = phones.ifEmpty { listOf(ContactPhone("")) },
    organization = organization,
    jobTitle = jobTitle,
    addresses = addresses.ifEmpty { listOf(ContactPostalAddress()) },
    birthday = birthday,
    notes = notes,
    addressBookId = addressBookId,
)

fun ContactDraft.normalized(): ContactDraft {
    val cleanedEmails = emails.map { it.copy(value = it.value.trim()) }.filter { it.value.isNotEmpty() }
    val cleanedPhones = phones.map { it.copy(value = it.value.trim()) }.filter { it.value.isNotEmpty() }
    val cleanedAddresses = addresses.filterNot { it.isBlank() }
    val derivedName = displayName.trim().ifEmpty { name.formatted() }
        .ifEmpty { cleanedEmails.firstOrNull()?.value }
        .orEmpty()
        .ifEmpty { cleanedPhones.firstOrNull()?.value }
        .orEmpty()
        .ifEmpty { organization.trim() }
        .ifEmpty { "Unnamed contact" }
    return copy(
        displayName = derivedName,
        emails = cleanedEmails,
        phones = cleanedPhones,
        addresses = cleanedAddresses,
        organization = organization.trim(),
        jobTitle = jobTitle.trim(),
        birthday = birthday?.trim()?.takeIf { it.isNotEmpty() },
    )
}

fun validateContactDraft(draft: ContactDraft): String? {
    val value = draft.normalized()
    if (value.addressBookId.isBlank()) return "Choose an address book."
    if (value.displayName.length > MAX_DISPLAY_NAME) return "Display name is too long."
    if (value.organization.length > MAX_ORGANIZATION) return "Organization is too long."
    if (value.jobTitle.length > MAX_JOB_TITLE) return "Job title is too long."
    if (value.notes.length > MAX_NOTE_CHARS) return "Notes are too long."
    if (value.notes.contains('\u0000')) return "Notes cannot contain NUL bytes."
    listOf(
        value.name.givenName,
        value.name.familyName,
        value.name.additionalNames,
        value.name.honorificPrefix,
        value.name.honorificSuffix,
    ).forEach { part ->
        if (part.length > MAX_NAME_PART) return "A name part is too long."
    }
    if (value.emails.size > MAX_EMAILS) return "Too many email addresses."
    if (value.phones.size > MAX_PHONES) return "Too many phone numbers."
    if (value.addresses.size > MAX_ADDRESSES) return "Too many postal addresses."
    value.emails.forEach { email ->
        if (email.value.length > MAX_EMAIL || '@' !in email.value || ' ' in email.value) {
            return "Enter a valid email address."
        }
    }
    value.phones.forEach { phone ->
        if (phone.value.length > MAX_PHONE || phone.value.any { ch ->
                ch !in "0123456789 +-()./xX"
            }
        ) {
            return "Enter a valid phone number."
        }
    }
    value.addresses.forEach { address ->
        if (listOf(
                address.poBox,
                address.extended,
                address.street,
                address.locality,
                address.region,
                address.postalCode,
                address.country,
            ).any { it.length > MAX_ADDRESS_LINE }
        ) {
            return "An address line is too long."
        }
    }
    value.birthday?.let { birthday ->
        val compact = birthday.replace("-", "")
        if (compact.length != 8 || compact.any { !it.isDigit() }) {
            return "Birthday must be YYYY-MM-DD."
        }
    }
    val hasIdentity = value.displayName.isNotBlank() ||
        !value.name.isBlank() ||
        value.emails.isNotEmpty() ||
        value.phones.isNotEmpty() ||
        value.organization.isNotBlank()
    if (!hasIdentity) return "Add a name, email, phone, or organization."
    return null
}

fun validateBookName(name: String): String? {
    val trimmed = name.trim()
    if (trimmed.isEmpty()) return "Address book name is required."
    if (trimmed.length > MAX_BOOK_NAME) return "Address book name is too long."
    if (trimmed.any { it.isISOControl() }) return "Address book name cannot contain control characters."
    return null
}

fun emailsJson(emails: List<ContactEmail>): String =
    jsonArray(emails) { email ->
        jsonObject(
            "value" to email.value,
            "type" to email.type,
            "pref" to email.pref,
        )
    }

fun phonesJson(phones: List<ContactPhone>): String =
    jsonArray(phones) { phone ->
        jsonObject(
            "value" to phone.value,
            "type" to phone.type,
            "pref" to phone.pref,
        )
    }

fun addressesJson(addresses: List<ContactPostalAddress>): String =
    jsonArray(addresses) { address ->
        jsonObject(
            "poBox" to address.poBox,
            "extended" to address.extended,
            "street" to address.street,
            "locality" to address.locality,
            "region" to address.region,
            "postalCode" to address.postalCode,
            "country" to address.country,
            "type" to address.type,
            "pref" to address.pref,
        )
    }

fun parseEmails(raw: String?): List<ContactEmail> = parseObjectArray(raw).map { obj ->
    ContactEmail(
        value = obj["value"].orEmpty(),
        type = obj["type"].orEmpty().ifBlank { "other" },
        pref = obj["pref"].equals("true", ignoreCase = true),
    )
}

fun parsePhones(raw: String?): List<ContactPhone> = parseObjectArray(raw).map { obj ->
    ContactPhone(
        value = obj["value"].orEmpty(),
        type = obj["type"].orEmpty().ifBlank { "other" },
        pref = obj["pref"].equals("true", ignoreCase = true),
    )
}

fun parseAddresses(raw: String?): List<ContactPostalAddress> = parseObjectArray(raw).map { obj ->
    ContactPostalAddress(
        poBox = obj["poBox"].orEmpty(),
        extended = obj["extended"].orEmpty(),
        street = obj["street"].orEmpty(),
        locality = obj["locality"].orEmpty(),
        region = obj["region"].orEmpty(),
        postalCode = obj["postalCode"].orEmpty(),
        country = obj["country"].orEmpty(),
        type = obj["type"].orEmpty().ifBlank { "other" },
        pref = obj["pref"].equals("true", ignoreCase = true),
    )
}

private fun <T> jsonArray(items: List<T>, encode: (T) -> String): String =
    items.joinToString(prefix = "[", postfix = "]") { encode(it) }

private fun jsonObject(vararg pairs: Pair<String, Any?>): String =
    pairs.joinToString(prefix = "{", postfix = "}") { (key, value) ->
        "\"$key\":${jsonValue(value)}"
    }

private fun jsonValue(value: Any?): String = when (value) {
    null -> "null"
    is Boolean -> value.toString()
    is Number -> value.toString()
    else -> "\"${escapeJson(value.toString())}\""
}

private fun escapeJson(value: String): String = buildString {
    value.forEach { ch ->
        when (ch) {
            '\\' -> append("\\\\")
            '"' -> append("\\\"")
            '\n' -> append("\\n")
            '\r' -> append("\\r")
            '\t' -> append("\\t")
            else -> append(ch)
        }
    }
}

internal fun parseObjectArray(raw: String?): List<Map<String, String>> {
    if (raw.isNullOrBlank() || raw == "null") return emptyList()
    val body = raw.trim().removePrefix("[").removeSuffix("]")
    if (body.isBlank()) return emptyList()
    return splitTopLevel(body, ',').mapNotNull { item ->
        val trimmed = item.trim()
        if (!trimmed.startsWith("{")) return@mapNotNull null
        val inner = trimmed.removePrefix("{").removeSuffix("}")
        splitTopLevel(inner, ',').mapNotNull { pair ->
            val colon = pair.indexOf(':')
            if (colon < 0) return@mapNotNull null
            val key = unquote(pair.substring(0, colon).trim())
            val value = unquote(pair.substring(colon + 1).trim())
            key to value
        }.toMap()
    }
}

private fun splitTopLevel(value: String, delimiter: Char): List<String> {
    val parts = ArrayList<String>()
    val current = StringBuilder()
    var depth = 0
    var quoted = false
    var escaped = false
    value.forEach { ch ->
        when {
            escaped -> {
                current.append(ch)
                escaped = false
            }
            ch == '\\' -> {
                current.append(ch)
                escaped = true
            }
            ch == '"' -> {
                quoted = !quoted
                current.append(ch)
            }
            !quoted && (ch == '{' || ch == '[') -> {
                depth += 1
                current.append(ch)
            }
            !quoted && (ch == '}' || ch == ']') -> {
                depth -= 1
                current.append(ch)
            }
            !quoted && depth == 0 && ch == delimiter -> {
                parts.add(current.toString())
                current.clear()
            }
            else -> current.append(ch)
        }
    }
    if (current.isNotEmpty()) parts.add(current.toString())
    return parts
}

private fun unquote(value: String): String {
    val trimmed = value.trim()
    val unquoted = if (trimmed.length >= 2 && trimmed.startsWith("\"") && trimmed.endsWith("\"")) {
        trimmed.substring(1, trimmed.length - 1)
    } else {
        trimmed
    }
    return unquoted
        .replace("\\n", "\n")
        .replace("\\r", "\r")
        .replace("\\t", "\t")
        .replace("\\\"", "\"")
        .replace("\\\\", "\\")
}

fun typeLabel(type: String): String = when (type.lowercase()) {
    "work" -> "Work"
    "home" -> "Home"
    "cell", "mobile" -> "Mobile"
    "fax" -> "Fax"
    "voice" -> "Voice"
    else -> type.replaceFirstChar { if (it.isLowerCase()) it.titlecase() else it.toString() }
}

val contactTypes = listOf("work", "home", "cell", "other")
val addressTypes = listOf("home", "work", "other")
