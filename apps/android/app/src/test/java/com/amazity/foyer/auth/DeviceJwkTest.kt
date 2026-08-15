package com.amazity.foyer.auth

import java.io.File
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class DeviceJwkTest {
    @Test
    fun rfc7638ExampleMatchesCanonicalThumbprint() {
        val jwk = DevicePublicJwk(
            x = "MKBCTNIcKUSDii11ySs3526iDZ8AiTo7Tu6KPAqv7D4",
            y = "4Etl6SRW2YiLUrN5vfvVHuhp7x8PxltmWWlbbM4IFyM",
        )
        assertEquals(
            """{"crv":"P-256","kty":"EC","x":"MKBCTNIcKUSDii11ySs3526iDZ8AiTo7Tu6KPAqv7D4","y":"4Etl6SRW2YiLUrN5vfvVHuhp7x8PxltmWWlbbM4IFyM"}""",
            jwk.canonicalJson(),
        )
        assertEquals("cn-I_WNMClehiVp51i_0VpOENW1upEerA8sEam5hn-s", jwk.deviceKeyId())
    }

    @Test
    fun contractFixturesMatchWhenPresent() {
        val publicJwk = AuthContractFixtures.json("fixtures/rfc7517-public.jwk.json")
            ?: AuthContractFixtures.json("jwk-thumbprint.json")
            ?: return
        val jwkObject = publicJwk.optJSONObject("jwk") ?: publicJwk
        val jwk = DevicePublicJwk(
            x = jwkObject.getString("x"),
            y = jwkObject.getString("y"),
        )
        val expectedId = AuthContractFixtures.text("fixtures/thumbprint.txt")
            ?: publicJwk.optString("deviceKeyId").ifBlank { publicJwk.optString("thumbprint") }
        if (!expectedId.isNullOrBlank()) {
            assertEquals(expectedId.trim(), jwk.deviceKeyId())
        }
        val canonical = AuthContractFixtures.text("fixtures/canonical-jwk.json")
            ?: publicJwk.optString("canonicalJson").ifBlank { jwk.canonicalJson() }
        assertEquals(canonical.trim(), jwk.canonicalJson())
    }

    @Test
    fun rejectsPaddedOrMalformedCoordinates() {
        runCatching {
            DevicePublicJwk(
                x = "MKBCTNIcKUSDii11ySs3526iDZ8AiTo7Tu6KPAqv7D4=",
                y = "4Etl6SRW2YiLUrN5vfvVHuhp7x8PxltmWWlbbM4IFyM",
            )
        }.exceptionOrNull().let { assertTrue(it is IllegalArgumentException) }
        runCatching {
            DevicePublicJwk(x = "+++", y = "---")
        }.exceptionOrNull().let { assertTrue(it is IllegalArgumentException) }
    }

    @Test
    fun enrollmentJsonIsPublicOnly() {
        val material = DevicePublicMaterial(
            DevicePublicJwk(
                x = "MKBCTNIcKUSDii11ySs3526iDZ8AiTo7Tu6KPAqv7D4",
                y = "4Etl6SRW2YiLUrN5vfvVHuhp7x8PxltmWWlbbM4IFyM",
            ),
        )
        val json = material.enrollmentJson()
        assertTrue(json.contains("\"kty\": \"EC\""))
        assertTrue(json.contains("\"deviceKeyId\""))
        assertFalse(json.contains("\"d\""))
        assertFalse(json.contains("private", ignoreCase = true))
    }
}

internal object AuthContractFixtures {
    fun json(name: String): JSONObject? {
        val text = text(name) ?: return null
        return JSONObject(text)
    }

    fun text(name: String): String? = file(name)?.takeIf { it.isFile }?.readText()

    fun file(name: String): File? {
        val roots = listOf(
            File("/home/user/Projects/amazity/foyer/contracts/auth/v1"),
            File("../../../../contracts/auth/v1"),
            File("../../../contracts/auth/v1"),
            File("../../contracts/auth/v1"),
            File("contracts/auth/v1"),
        )
        return roots.asSequence()
            .map { File(it, name) }
            .firstOrNull(File::isFile)
    }
}
