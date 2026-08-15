package com.amazity.foyer.auth

import java.time.Instant

data class AuthChallenge(
    val challengeId: String,
    val signingPayload: String,
    val expiresAt: Instant,
)

data class AuthSession(
    val accessToken: String,
    val tokenType: String,
    val expiresAt: Instant,
    val userId: String,
    val deviceKeyId: String,
)

interface DeviceSigner {
    fun material(): DevicePublicMaterial
    fun signSha256(payload: ByteArray): ByteArray
}

interface DeviceAuthTransport {
    suspend fun createChallenge(deviceKeyId: String): AuthChallenge
    suspend fun createSession(challengeId: String, signature: String): AuthSession
    suspend fun validateAccessToken(accessToken: String)
}

interface AuthSessionPersistence {
    fun lastAuthenticatedAt(): Instant?
    fun markAuthenticated(at: Instant)
    fun usedDevelopmentAuth(): Boolean
    fun markDevelopmentAuth(used: Boolean)
    fun clearSessionState()
    fun writePublicMaterial(material: DevicePublicMaterial)
}

interface AccessTokenProvider {
    suspend fun bearerToken(forceRefresh: Boolean = false): String
    fun invalidateAccessToken()
    fun hasPreviousAccess(): Boolean
}
