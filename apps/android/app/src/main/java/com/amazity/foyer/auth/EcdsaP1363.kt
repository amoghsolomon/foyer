package com.amazity.foyer.auth

/**
 * Strict conversion from the DER ECDSA signature returned by Android's
 * `SHA256withECDSA` to the IEEE P1363 64-byte `r||s` encoding required on the
 * Foyer wire.
 */
object EcdsaP1363 {
    private const val COORDINATE_SIZE = 32
    private const val SEQUENCE = 0x30
    private const val INTEGER = 0x02

    fun derToIeeeP1363(der: ByteArray): ByteArray {
        require(der.isNotEmpty()) { "DER signature is empty" }
        val reader = DerReader(der)
        reader.expectTag(SEQUENCE)
        val sequence = reader.readValue()
        reader.expectExhausted()

        val contents = DerReader(sequence)
        val r = contents.readInteger()
        val s = contents.readInteger()
        contents.expectExhausted()

        return fixedCoordinate(r) + fixedCoordinate(s)
    }

    private fun fixedCoordinate(value: ByteArray): ByteArray {
        val unsigned = stripMinimalInteger(value)
        require(unsigned.size <= COORDINATE_SIZE) { "ECDSA coordinate exceeds P-256 size" }
        require(unsigned.any { it != 0.toByte() }) { "ECDSA coordinate must be non-zero" }
        return ByteArray(COORDINATE_SIZE).also { dest ->
            unsigned.copyInto(dest, dest.size - unsigned.size)
        }
    }

    private fun stripMinimalInteger(value: ByteArray): ByteArray {
        require(value.isNotEmpty()) { "DER INTEGER is empty" }
        require(value[0].toInt() and 0x80 == 0) { "ECDSA coordinate must be positive" }
        if (value.size > 1 && value[0] == 0.toByte()) {
            require(value[1].toInt() and 0x80 != 0) { "DER INTEGER is not minimally encoded" }
            return value.copyOfRange(1, value.size)
        }
        return value
    }

    private class DerReader(private val bytes: ByteArray) {
        private var offset = 0

        fun expectTag(tag: Int) {
            require(offset < bytes.size) { "truncated DER tag" }
            val actual = bytes[offset].toInt() and 0xff
            require(actual == tag) { "unexpected DER tag" }
            offset += 1
        }

        fun readInteger(): ByteArray {
            expectTag(INTEGER)
            return readValue()
        }

        fun readValue(): ByteArray {
            require(offset < bytes.size) { "truncated DER length" }
            val first = bytes[offset].toInt() and 0xff
            offset += 1
            val length = if (first < 0x80) {
                first
            } else {
                val count = first and 0x7f
                require(count in 1..2) { "unsupported DER length" }
                require(offset + count <= bytes.size) { "truncated DER length" }
                var value = 0
                repeat(count) {
                    value = (value shl 8) or (bytes[offset].toInt() and 0xff)
                    offset += 1
                }
                require(value >= 0x80) { "DER length is not minimally encoded" }
                value
            }
            require(length > 0) { "DER value is empty" }
            require(offset + length <= bytes.size) { "truncated DER value" }
            val value = bytes.copyOfRange(offset, offset + length)
            offset += length
            return value
        }

        fun expectExhausted() {
            require(offset == bytes.size) { "DER encoding has trailing bytes" }
        }
    }
}
