package com.amazity.foyer.sync

import org.json.JSONObject

internal fun Map<String, *>.text(key: String): String? =
    this[key]?.toString()?.takeUnless { it == "null" }

internal fun Map<String, *>.requiredText(key: String, domain: String): String =
    text(key) ?: error("Missing $domain upload field: $key")

internal fun Map<String, *>.requiredLong(key: String, domain: String): Long =
    (this[key] as? Number)?.toLong() ?: text(key)?.toLongOrNull()
        ?: error("Missing $domain upload field: $key")

internal fun Map<String, *>.int(key: String): Int? =
    (this[key] as? Number)?.toInt() ?: text(key)?.toIntOrNull()

internal fun Map<String, *>.flag(key: String): Boolean {
    val number = (this[key] as? Number)?.toLong()
    val text = text(key)
    return when {
        number != null -> number != 0L
        text.equals("true", ignoreCase = true) || text == "t" || text == "1" -> true
        else -> false
    }
}

internal fun Map<String, *>.payloadObject(key: String = CLIENT_PAYLOAD): JSONObject? {
    val encoded = text(key) ?: return null
    return runCatching { JSONObject(encoded) }.getOrNull()
}

internal fun JSONObject.optionalText(key: String): String? =
    if (isNull(key)) null else optString(key).takeIf(String::isNotBlank)
