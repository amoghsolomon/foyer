package com.amazity.foyer.auth

import com.amazity.foyer.network.ApiException
import java.time.Duration
import java.time.Instant
import java.util.concurrent.atomic.AtomicInteger
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.async
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class AuthSessionCoordinatorTest {
    @Test
    fun refreshesShortlyBeforeExpiryAndReusesFreshToken() = runBlocking {
        val transport = FakeTransport()
        val clock = MutableClock(Instant.parse("2026-08-15T12:00:00Z"))
        val coordinator = coordinator(transport, clock)
        val first = coordinator.bearerToken()
        assertEquals("token-1", first)
        assertEquals(1, transport.challenges.get())
        clock.now = Instant.parse("2026-08-15T12:02:00Z")
        assertEquals("token-1", coordinator.bearerToken())
        assertEquals(1, transport.challenges.get())
        clock.now = Instant.parse("2026-08-15T12:04:20Z")
        assertEquals("token-2", coordinator.bearerToken())
        assertEquals(2, transport.challenges.get())
    }

    @Test
    fun coalescesConcurrentRefresh() = runBlocking {
        val gate = CompletableDeferred<Unit>()
        val transport = FakeTransport(beforeChallenge = { gate.await() })
        val coordinator = coordinator(transport)
        val first = async { coordinator.bearerToken() }
        val second = async { coordinator.bearerToken() }
        while (transport.started.get() == 0) {
            kotlinx.coroutines.yield()
        }
        assertEquals(1, transport.started.get())
        gate.complete(Unit)
        assertEquals("token-1", first.await())
        assertEquals("token-1", second.await())
        assertEquals(1, transport.challenges.get())
    }

    @Test
    fun forceRefreshAfterInvalidateIssuesANewChallenge() = runBlocking {
        val transport = FakeTransport()
        val coordinator = coordinator(transport)
        assertEquals("token-1", coordinator.bearerToken())
        coordinator.invalidateAccessToken()
        assertEquals("token-2", coordinator.bearerToken(forceRefresh = true))
        assertEquals(2, transport.challenges.get())
    }

    @Test
    fun unauthorizedChallengeClearsPreviousAccess() = runBlocking {
        val transport = FakeTransport(challengeStatus = 401)
        val persistence = MemoryPersistence().apply {
            markAuthenticated(Instant.parse("2026-08-15T11:00:00Z"))
        }
        val coordinator = coordinator(transport, persistence = persistence)
        assertTrue(coordinator.hasPreviousAccess())
        assertFalse(coordinator.authenticate())
        assertFalse(coordinator.hasPreviousAccess())
    }

    @Test
    fun developmentTokenIsRefusedWhenDisabled() {
        val coordinator = coordinator(FakeTransport(), developmentToken = null)
        val error = runCatching { runBlocking { coordinator.useDevelopmentSession() } }.exceptionOrNull()
        assertTrue(error is IllegalArgumentException)
    }

    @Test
    fun developmentTokenSkipsChallengeExchange() = runBlocking {
        val transport = FakeTransport()
        val persistence = MemoryPersistence()
        val coordinator = coordinator(transport, persistence = persistence, developmentToken = "dev-token")
        coordinator.useDevelopmentSession()
        assertEquals("dev-token", coordinator.bearerToken())
        assertEquals("dev-token", coordinator.bearerToken(forceRefresh = true))
        assertEquals(0, transport.challenges.get())
        assertTrue(coordinator.hasPreviousAccess())
        assertTrue(persistence.usedDevelopmentAuth())
        assertTrue(persistence.written.isNotEmpty())
        assertFalse(persistence.written.any { it.enrollmentJson().contains("dev-token") })
    }

    @Test
    fun signsDecodedPayloadAndSendsUnpaddedP1363() = runBlocking {
        val transport = FakeTransport()
        val signer = RecordingSigner()
        val coordinator = AuthSessionCoordinator(
            developmentToken = null,
            signer = signer,
            transport = transport,
            persistence = MemoryPersistence(),
            clock = { Instant.parse("2026-08-15T12:00:00Z") },
            refreshSkew = Duration.ofSeconds(60),
        )
        assertEquals("token-1", coordinator.bearerToken())
        assertEquals(1, signer.payloads.size)
        assertEquals("payload-1", signer.payloads.single().decodeToString())
        assertEquals(Base64Url.encode(ByteArray(64) { 1 }), transport.signatures.single())
        assertFalse(transport.signatures.single().contains("="))
    }

    @Test
    fun doesNotGiveAccessTokenToPersistence() = runBlocking {
        val persistence = MemoryPersistence()
        val coordinator = coordinator(FakeTransport(), persistence = persistence)
        coordinator.bearerToken()
        assertTrue(persistence.written.isNotEmpty())
        assertFalse(persistence.written.joinToString { it.enrollmentJson() }.contains("token-1"))
        assertFalse(persistence.storedTokens)
    }

    @Test
    fun restoreAllowsOfflineWhenPreviouslyAuthenticated() = runBlocking {
        val transport = FakeTransport(challengeStatus = 503)
        val persistence = MemoryPersistence().apply {
            markAuthenticated(Instant.parse("2026-08-15T11:00:00Z"))
        }
        val coordinator = coordinator(transport, persistence = persistence)
        assertTrue(coordinator.restoreSession())
    }

    private fun coordinator(
        transport: FakeTransport,
        clock: MutableClock = MutableClock(Instant.parse("2026-08-15T12:00:00Z")),
        persistence: MemoryPersistence = MemoryPersistence(),
        developmentToken: String? = null,
    ) = AuthSessionCoordinator(
        developmentToken = developmentToken,
        signer = FakeSigner(),
        transport = transport,
        persistence = persistence,
        clock = { clock.now },
        refreshSkew = Duration.ofSeconds(60),
    )
}

private class MutableClock(var now: Instant)

private class FakeSigner : DeviceSigner {
    private val jwk = DevicePublicJwk(
        x = "MKBCTNIcKUSDii11ySs3526iDZ8AiTo7Tu6KPAqv7D4",
        y = "4Etl6SRW2YiLUrN5vfvVHuhp7x8PxltmWWlbbM4IFyM",
    )

    override fun material(): DevicePublicMaterial = DevicePublicMaterial(jwk)
    override fun signSha256(payload: ByteArray): ByteArray = ByteArray(64) { 1 }
}

private class RecordingSigner : DeviceSigner {
    val payloads = mutableListOf<ByteArray>()
    private val delegate = FakeSigner()
    override fun material(): DevicePublicMaterial = delegate.material()
    override fun signSha256(payload: ByteArray): ByteArray {
        payloads += payload.copyOf()
        return delegate.signSha256(payload)
    }
}

private class MemoryPersistence : AuthSessionPersistence {
    private var lastAuth: Instant? = null
    private var development = false
    val written = mutableListOf<DevicePublicMaterial>()
    var storedTokens = false
    override fun lastAuthenticatedAt(): Instant? = lastAuth
    override fun markAuthenticated(at: Instant) { lastAuth = at }
    override fun usedDevelopmentAuth(): Boolean = development
    override fun markDevelopmentAuth(used: Boolean) { development = used }
    override fun clearSessionState() {
        lastAuth = null
        development = false
    }
    override fun writePublicMaterial(material: DevicePublicMaterial) {
        written += material
    }
}

private class FakeTransport(
    private val challengeStatus: Int = 200,
    private val beforeChallenge: suspend () -> Unit = {},
) : DeviceAuthTransport {
    val challenges = AtomicInteger(0)
    val started = AtomicInteger(0)
    val signatures = mutableListOf<String>()
    private val issued = AtomicInteger(0)

    override suspend fun createChallenge(deviceKeyId: String): AuthChallenge {
        started.incrementAndGet()
        beforeChallenge()
        if (challengeStatus != 200) throw ApiException(challengeStatus, "request failed")
        challenges.incrementAndGet()
        return AuthChallenge(
            challengeId = "challenge-${challenges.get()}",
            signingPayload = Base64Url.encode("payload-${challenges.get()}".toByteArray()),
            expiresAt = Instant.parse("2026-08-15T12:01:00Z"),
        )
    }

    override suspend fun createSession(challengeId: String, signature: String): AuthSession {
        signatures += signature
        val n = issued.incrementAndGet()
        return AuthSession(
            accessToken = "token-$n",
            tokenType = "Bearer",
            expiresAt = Instant.parse("2026-08-15T12:05:00Z"),
            userId = "user-1",
            deviceKeyId = "cn-I_WNMClehiVp51i_0VpOENW1upEerA8sEam5hn-s",
        )
    }

    override suspend fun validateAccessToken(accessToken: String) = Unit
}
