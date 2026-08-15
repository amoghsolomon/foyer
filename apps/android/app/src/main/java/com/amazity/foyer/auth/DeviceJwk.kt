package com.amazity.foyer.auth

import java.math.BigInteger
import java.security.MessageDigest
import java.security.interfaces.ECPublicKey

data class DevicePublicJwk(
    val kty: String = "EC",
    val crv: String = "P-256",
    val x: String,
    val y: String,
) {
    init {
        require(kty == "EC") { "only EC public keys are supported" }
        require(crv == "P-256") { "only P-256 public keys are supported" }
        require(x.isNotEmpty() && y.isNotEmpty()) { "JWK coordinates are required" }
        require(COORDINATE.matches(x) && COORDINATE.matches(y)) { "JWK coordinates must be unpadded base64url" }
    }

    fun canonicalJson(): String =
        """{"crv":"$crv","kty":"$kty","x":"$x","y":"$y"}"""

    fun deviceKeyId(): String =
        Base64Url.encode(MessageDigest.getInstance("SHA-256").digest(canonicalJson().toByteArray(Charsets.UTF_8)))

    fun normalized(): DevicePublicJwk = DevicePublicJwk(kty = "EC", crv = "P-256", x = x, y = y)

    companion object {
        private val COORDINATE = Regex("^[A-Za-z0-9_-]+$")

        fun fromEcPublicKey(publicKey: ECPublicKey): DevicePublicJwk {
            val point = publicKey.w
            return DevicePublicJwk(
                x = Base64Url.encode(unsignedFixed(point.affineX)),
                y = Base64Url.encode(unsignedFixed(point.affineY)),
            )
        }

        fun unsignedFixed(value: BigInteger, size: Int = 32): ByteArray {
            require(value.signum() >= 0) { "coordinate must be non-negative" }
            val raw = value.toByteArray()
            val start = if (raw.size > 1 && raw[0] == 0.toByte()) 1 else 0
            val unsigned = raw.copyOfRange(start, raw.size)
            require(unsigned.size <= size) { "coordinate exceeds P-256 field size" }
            return ByteArray(size).also { dest ->
                unsigned.copyInto(dest, dest.size - unsigned.size)
            }
        }
    }
}

data class DevicePublicMaterial(
    val jwk: DevicePublicJwk,
    val deviceKeyId: String = jwk.deviceKeyId(),
) {
    fun enrollmentJson(): String = buildString {
        append("{\n")
        append("  \"algorithm\": \"ES256\",\n")
        append("  \"crv\": \"").append(jwk.crv).append("\",\n")
        append("  \"deviceKeyId\": \"").append(deviceKeyId).append("\",\n")
        append("  \"kty\": \"").append(jwk.kty).append("\",\n")
        append("  \"x\": \"").append(jwk.x).append("\",\n")
        append("  \"y\": \"").append(jwk.y).append("\"\n")
        append("}")
    }
}
