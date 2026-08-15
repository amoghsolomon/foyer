package com.amazity.foyer.auth

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import java.security.KeyStore
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class AndroidDeviceKeyStoreTest {
    @Test
    fun generatesNonExportableP256KeyAndRetainsPublicMaterial() {
        val first = AndroidDeviceKeyStore()
        val material = first.material()
        assertEquals("EC", material.jwk.kty)
        assertEquals("P-256", material.jwk.crv)
        assertEquals(material.jwk.deviceKeyId(), material.deviceKeyId)

        val again = AndroidDeviceKeyStore()
        assertEquals(material.deviceKeyId, again.material().deviceKeyId)

        val signature = first.signSha256("foyer-android-keystore".toByteArray())
        assertEquals(64, signature.size)

        val stored = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        val privateKey = stored.getKey(AndroidDeviceKeyStore.KEY_ALIAS, null)
        assertNotNull(privateKey)
        assertTrue(runCatching { privateKey.encoded }.getOrNull() == null)

        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val enrollment = DeviceEnrollmentStore(context)
        enrollment.writePublicMaterial(material)
        val file = enrollment.enrollmentFile()
        assertTrue(file.isFile)
        val text = file.readText()
        assertTrue(text.contains(material.deviceKeyId))
        assertTrue(text.contains("\"kty\": \"EC\""))
        assertFalse(text.contains("\"d\""))
    }
}
