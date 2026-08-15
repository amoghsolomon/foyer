package com.amazity.foyer.auth

import com.amazity.foyer.BuildConfig
import com.amazity.foyer.data.SessionStore
import com.amazity.foyer.network.ApiException
import java.time.Duration
import java.time.Instant
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock

class AuthSessionCoordinator(
    private val developmentToken: String?,
    private val signer: DeviceSigner,
    private val transport: DeviceAuthTransport,
    private val persistence: AuthSessionPersistence,
    private val clock: () -> Instant = Instant::now,
    private val refreshSkew: Duration = Duration.ofSeconds(45),
) : AccessTokenProvider {
    private val lock = Mutex()
    @Volatile private var cached: AuthSession? = null
    @Volatile private var developmentActive = false
    private var inflight: CompletableDeferred<AuthSession>? = null

    init {
        persistence.writePublicMaterial(signer.material())
        if (developmentToken == null) {
            persistence.markDevelopmentAuth(false)
        } else if (persistence.usedDevelopmentAuth()) {
            developmentActive = true
        }
    }

    fun developmentAuthAvailable(): Boolean = !developmentToken.isNullOrBlank()

    fun enrollment(): DeviceEnrollmentPresentation {
        val material = signer.material()
        persistence.writePublicMaterial(material)
        return DeviceEnrollmentPresentation(
            deviceKeyId = material.deviceKeyId,
            publicJwk = material.jwk,
        )
    }

    override fun hasPreviousAccess(): Boolean =
        persistence.lastAuthenticatedAt() != null ||
            (developmentAuthAvailable() && persistence.usedDevelopmentAuth())

    suspend fun restoreSession(): Boolean {
        enrollment()
        if (developmentActive && developmentToken != null) {
            return runCatching { useDevelopmentSession() }.isSuccess || hasPreviousAccess()
        }
        if (!hasPreviousAccess()) return false
        return authenticateAllowingOffline()
    }

    suspend fun authenticate(): Boolean = try {
        bearerToken(forceRefresh = true)
        true
    } catch (error: ApiException) {
        if (error.status in setOf(401, 403, 404)) {
            persistence.clearSessionState()
            developmentActive = false
            false
        } else {
            throw error
        }
    }

    suspend fun useDevelopmentSession() {
        val token = developmentToken
        require(!token.isNullOrBlank()) { "Development authentication is disabled in this build." }
        transport.validateAccessToken(token)
        developmentActive = true
        cached = null
        persistence.markDevelopmentAuth(true)
        persistence.markAuthenticated(clock())
    }

    fun signOut() {
        cached = null
        developmentActive = false
        persistence.clearSessionState()
        persistPublicOnly()
    }

    override fun invalidateAccessToken() {
        cached = null
    }

    override suspend fun bearerToken(forceRefresh: Boolean): String {
        if (developmentActive) {
            val token = developmentToken
            require(!token.isNullOrBlank()) { "Development authentication is disabled in this build." }
            return token
        }
        if (!forceRefresh) {
            cached?.takeIf { it.isFresh(clock(), refreshSkew) }?.let { return it.accessToken }
        }
        return refreshSession(forceRefresh).accessToken
    }

    private suspend fun authenticateAllowingOffline(): Boolean = try {
        bearerToken(forceRefresh = cached == null)
        true
    } catch (error: ApiException) {
        if (error.status in setOf(401, 403, 404)) {
            persistence.clearSessionState()
            developmentActive = false
            false
        } else {
            hasPreviousAccess()
        }
    } catch (_: Exception) {
        hasPreviousAccess()
    }

    private suspend fun refreshSession(forceRefresh: Boolean): AuthSession {
        val waiter: CompletableDeferred<AuthSession>
        val leader: Boolean
        lock.withLock {
            if (!forceRefresh) {
                cached?.takeIf { it.isFresh(clock(), refreshSkew) }?.let { return it }
            }
            val existing = inflight
            if (existing != null) {
                waiter = existing
                leader = false
            } else {
                waiter = CompletableDeferred()
                inflight = waiter
                leader = true
            }
        }
        if (!leader) return waiter.await()
        return try {
            val session = exchangeChallenge()
            cached = session
            persistence.markAuthenticated(clock())
            waiter.complete(session)
            session
        } catch (error: Throwable) {
            waiter.completeExceptionally(error)
            throw error
        } finally {
            lock.withLock { if (inflight === waiter) inflight = null }
        }
    }

    private suspend fun exchangeChallenge(): AuthSession {
        val material = signer.material()
        persistence.writePublicMaterial(material)
        val challenge = transport.createChallenge(material.deviceKeyId)
        val payload = Base64Url.decode(challenge.signingPayload)
        val signature = Base64Url.encode(signer.signSha256(payload))
        return transport.createSession(challenge.challengeId, signature)
    }

    private fun persistPublicOnly() {
        persistence.writePublicMaterial(signer.material())
    }

    companion object {
        fun create(
            context: android.content.Context,
            transport: DeviceAuthTransport = DeviceAuthHttp(),
            clock: () -> Instant = Instant::now,
        ): AuthSessionCoordinator {
            val app = context.applicationContext
            // Drop leftover encrypted short-lived tokens from earlier builds so they
            // cannot be mistaken for the Keystore device signing key.
            SessionStore(app).clear()
            val keys = AndroidDeviceKeyStore()
            val enrollment = DeviceEnrollmentStore(app)
            return AuthSessionCoordinator(
                developmentToken = developmentTokenOrNull(),
                signer = keys,
                transport = transport,
                persistence = enrollment,
                clock = clock,
            )
        }

        internal fun developmentTokenOrNull(): String? =
            if (BuildConfig.FOYER_DEVELOPMENT_AUTH) {
                BuildConfig.FOYER_DEV_TOKEN.takeIf(String::isNotBlank)
            } else {
                null
            }
    }
}

private fun AuthSession.isFresh(now: Instant, skew: Duration): Boolean =
    expiresAt.minus(skew).isAfter(now)
