package com.amazity.foyer.notifications

import android.app.Notification
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class NotificationSanitizerTest {
    @Test
    fun redactsStandaloneFourToEightDigitCodes() {
        val result = NotificationSanitizer.redact("Your login code is 482913. It expires soon.")

        assertEquals("Your login code is [redacted]. It expires soon.", result.text)
        assertTrue(result.redacted)
    }

    @Test
    fun redactsKeywordAlphanumericCodes() {
        val result = NotificationSanitizer.redact("Verification code: A7B9-C2")

        assertEquals("Verification code: [redacted]", result.text)
        assertTrue(result.redacted)
    }

    @Test
    fun leavesOrdinaryTextUntouched() {
        val result = NotificationSanitizer.redact("Maya sent a photo")

        assertEquals("Maya sent a photo", result.text)
        assertFalse(result.redacted)
    }

    @Test
    fun acceptsOnlyWhitelistedNonSummaryNonOngoingNotifications() {
        val whitelist = setOf("com.whatsapp")

        assertTrue(NotificationCapturePolicy.accepts("com.whatsapp", whitelist, 0))
        assertFalse(NotificationCapturePolicy.accepts("com.other", whitelist, 0))
        assertFalse(NotificationCapturePolicy.accepts("com.whatsapp", whitelist, Notification.FLAG_GROUP_SUMMARY))
        assertFalse(NotificationCapturePolicy.accepts("com.whatsapp", whitelist, Notification.FLAG_ONGOING_EVENT))
        assertFalse(NotificationCapturePolicy.accepts("com.whatsapp", whitelist, Notification.FLAG_FOREGROUND_SERVICE))
    }
}
