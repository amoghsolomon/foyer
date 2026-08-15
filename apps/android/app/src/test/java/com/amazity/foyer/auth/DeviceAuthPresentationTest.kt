package com.amazity.foyer.auth

import com.amazity.foyer.network.ApiException
import java.net.UnknownHostException
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class DeviceAuthPresentationTest {
    private val presentation = DeviceEnrollmentPresentation(
        deviceKeyId = "cn-I_WNMClehiVp51i_0VpOENW1upEerA8sEam5hn-s",
        publicJwk = DevicePublicJwk(
            x = "MKBCTNIcKUSDii11ySs3526iDZ8AiTo7Tu6KPAqv7D4",
            y = "4Etl6SRW2YiLUrN5vfvVHuhp7x8PxltmWWlbbM4IFyM",
        ),
    )

    @Test
    fun enrollmentCopyIsOperatorReadableAndPublic() {
        assertEquals("cn-I_WNMClehiVp51i_0VpOENW1upEerA8sEam5hn-s", presentation.fingerprint)
        assertTrue(presentation.enrollmentJson.contains("MKBCTNIcKUSDii11ySs3526iDZ8AiTo7Tu6KPAqv7D4"))
        assertFalse(presentation.enrollmentJson.contains("private", ignoreCase = true))
    }

    @Test
    fun errorsStayGeneric() {
        assertTrue(deviceAuthErrorMessage(ApiException(401, "unknown device")).contains("not enrolled"))
        assertTrue(deviceAuthErrorMessage(UnknownHostException("dns")).contains("Couldn't reach Foyer"))
        val generic = deviceAuthErrorMessage(IllegalStateException("secret-token-value"))
        assertFalse(generic.contains("secret-token-value"))
        assertTrue(generic.contains("Couldn't authenticate this device"))
    }
}
