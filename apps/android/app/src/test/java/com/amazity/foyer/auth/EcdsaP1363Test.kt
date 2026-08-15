package com.amazity.foyer.auth

import java.security.KeyPairGenerator
import java.security.Signature
import java.security.spec.ECGenParameterSpec
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class EcdsaP1363Test {
    @Test
    fun convertsMinimalPositiveIntegers() {
        val der = hex("3006020101020102")
        val p1363 = EcdsaP1363.derToIeeeP1363(der)
        assertEquals(64, p1363.size)
        assertEquals(1, p1363[31].toInt())
        assertEquals(2, p1363[63].toInt())
        assertTrue(p1363.copyOfRange(0, 31).all { it == 0.toByte() })
        assertTrue(p1363.copyOfRange(32, 63).all { it == 0.toByte() })
    }

    @Test
    fun convertsHighBitIntegerWithLeadingZero() {
        val r = ByteArray(33).also {
            it[0] = 0
            it[1] = 0x80.toByte()
        }
        val der = derSequence(derInteger(r), derInteger(byteArrayOf(1)))
        val p1363 = EcdsaP1363.derToIeeeP1363(der)
        assertEquals(0x80.toByte(), p1363[0])
        assertEquals(1, p1363[63].toInt())
    }

    @Test
    fun contractFixturesMatchWhenPresent() {
        val fixture = AuthContractFixtures.json("fixtures/ecdsa-p1363.json")
            ?: AuthContractFixtures.json("ecdsa-p1363.json")
        val cases = fixture?.optJSONArray("cases")
        if (cases != null) {
            for (index in 0 until cases.length()) {
                val item = cases.getJSONObject(index)
                val der = Base64Url.decode(item.getString("der"))
                val expected = Base64Url.decode(item.getString("p1363"))
                assertArrayEquals(expected, EcdsaP1363.derToIeeeP1363(der))
            }
            return
        }
        val derText = AuthContractFixtures.text("fixtures/signature.der.b64")
        val p1363Text = AuthContractFixtures.text("fixtures/signature.b64")
        if (derText != null && p1363Text != null) {
            assertArrayEquals(
                Base64Url.decode(p1363Text.trim()),
                EcdsaP1363.derToIeeeP1363(Base64Url.decode(derText.trim())),
            )
        }
    }

    @Test
    fun rejectsTrailingBytes() {
        val der = hex("300602010102010200")
        runCatching { EcdsaP1363.derToIeeeP1363(der) }.exceptionOrNull().let {
            assertTrue(it is IllegalArgumentException)
        }
    }

    @Test
    fun rejectsHighBitIntegerWithoutLeadingZero() {
        val der = hex("3006020180020101")
        runCatching { EcdsaP1363.derToIeeeP1363(der) }.exceptionOrNull().let {
            assertTrue(it is IllegalArgumentException)
        }
    }

    @Test
    fun rejectsZeroCoordinate() {
        val der = hex("3006020100020101")
        runCatching { EcdsaP1363.derToIeeeP1363(der) }.exceptionOrNull().let {
            assertTrue(it is IllegalArgumentException)
        }
    }

    @Test
    fun rejectsExtraInteger() {
        val der = hex("3009020101020101020103")
        runCatching { EcdsaP1363.derToIeeeP1363(der) }.exceptionOrNull().let {
            assertTrue(it is IllegalArgumentException)
        }
    }

    @Test
    fun convertsRealSha256WithEcdsaDer() {
        val keyPair = KeyPairGenerator.getInstance("EC").run {
            initialize(ECGenParameterSpec("secp256r1"))
            generateKeyPair()
        }
        val payload = "foyer-device-auth-fixture".toByteArray()
        val der = Signature.getInstance("SHA256withECDSA").run {
            initSign(keyPair.private)
            update(payload)
            sign()
        }
        val p1363 = EcdsaP1363.derToIeeeP1363(der)
        assertEquals(64, p1363.size)
        val verified = runCatching {
            Signature.getInstance("SHA256withECDSAinP1363Format").run {
                initVerify(keyPair.public)
                update(payload)
                verify(p1363)
            }
        }.getOrElse {
            Signature.getInstance("SHA256withECDSA").run {
                initVerify(keyPair.public)
                update(payload)
                verify(der)
            }
        }
        assertTrue(verified)
    }

    private fun hex(value: String): ByteArray =
        value.chunked(2).map { it.toInt(16).toByte() }.toByteArray()

    private fun derInteger(value: ByteArray): ByteArray =
        byteArrayOf(0x02, value.size.toByte()) + value

    private fun derSequence(vararg values: ByteArray): ByteArray {
        val body = values.reduce { acc, bytes -> acc + bytes }
        return byteArrayOf(0x30, body.size.toByte()) + body
    }
}
