package com.amazity.foyer.notifications

import android.app.Notification

data class RedactedText(val text: String, val redacted: Boolean)

object NotificationSanitizer {
    private val keywordCode = Regex(
        "(?i)\\b(OTP|verification(?:\\s+code)?|security\\s+code|code\\s+is|2FA)\\b" +
            "(\\s*(?:is\\s*)?[:=\\-]?\\s*)([A-Z0-9][A-Z0-9\\-]{3,11})",
    )
    private val numericCode = Regex("(?<!\\d)\\d{4,8}(?!\\d)")

    fun redact(value: String): RedactedText {
        var changed = false
        var output = keywordCode.replace(value) { match ->
            changed = true
            match.groupValues[1] + match.groupValues[2] + REDACTION_MARKER
        }
        output = numericCode.replace(output) {
            changed = true
            REDACTION_MARKER
        }
        return RedactedText(output.trim(), changed)
    }
}

object NotificationCapturePolicy {
    fun accepts(appPackage: String, whitelist: Set<String>, flags: Int): Boolean =
        appPackage in whitelist &&
            flags and Notification.FLAG_GROUP_SUMMARY == 0 &&
            flags and Notification.FLAG_ONGOING_EVENT == 0 &&
            flags and Notification.FLAG_FOREGROUND_SERVICE == 0
}

private const val REDACTION_MARKER = "[redacted]"
