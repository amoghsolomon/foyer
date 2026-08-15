package com.amazity.foyer.auth

import android.content.Context

data class GrokDeviceLogin(
    val flowId: String,
    val userCode: String,
    val verificationUri: String,
    val intervalSeconds: Long,
)

sealed interface GrokDevicePoll {
    data class Pending(val retryAfterSeconds: Long) : GrokDevicePoll
    data object Connected : GrokDevicePoll
}

data class GrokConnectionStatus(
    val configured: Boolean,
    val manageable: Boolean,
)

class GrokAccountCoordinator(context: Context) {
    private val api = foyerApiClient(context)

    suspend fun status(): GrokConnectionStatus {
        val body = api.grokStatus()
        return GrokConnectionStatus(
            configured = body.optBoolean("configured"),
            manageable = body.optBoolean("manageable", true),
        )
    }

    suspend fun startDeviceLogin(): GrokDeviceLogin {
        val body = api.startGrokDeviceLogin()
        return GrokDeviceLogin(
            flowId = body.getString("flowId"),
            userCode = body.getString("userCode"),
            verificationUri = body.getString("verificationUri"),
            intervalSeconds = body.optLong("intervalSeconds", 5L).coerceAtLeast(1L),
        )
    }

    suspend fun pollDeviceLogin(flowId: String): GrokDevicePoll {
        val body = api.pollGrokDeviceLogin(flowId)
        return when (body.getString("status")) {
            "connected" -> GrokDevicePoll.Connected
            else -> GrokDevicePoll.Pending(
                body.optLong("retryAfterSeconds", 5L).coerceAtLeast(1L),
            )
        }
    }
}
