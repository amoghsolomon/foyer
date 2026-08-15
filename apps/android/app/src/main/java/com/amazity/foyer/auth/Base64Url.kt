package com.amazity.foyer.auth

import java.util.Base64

internal object Base64Url {
    private val encoder = Base64.getUrlEncoder().withoutPadding()
    private val decoder = Base64.getUrlDecoder()

    fun encode(bytes: ByteArray): String = encoder.encodeToString(bytes)

    fun decode(value: String): ByteArray {
        val normalized = value.trim()
        require(normalized.isNotEmpty()) { "base64url value is empty" }
        return decoder.decode(normalized)
    }
}
