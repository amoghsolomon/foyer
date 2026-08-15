package com.amazity.foyer.auth

import com.amazity.foyer.network.ApiException
import java.net.ConnectException
import java.net.SocketTimeoutException
import java.net.UnknownHostException

data class DeviceEnrollmentPresentation(
    val deviceKeyId: String,
    val publicJwk: DevicePublicJwk,
) {
    val fingerprint: String = deviceKeyId
    val shortFingerprint: String = deviceKeyId.take(12)
    val enrollmentJson: String = DevicePublicMaterial(publicJwk, deviceKeyId).enrollmentJson()
}

fun deviceAuthErrorMessage(error: Throwable): String = when {
    error is ApiException && error.status in UNAUTHORIZED_STATUSES ->
        "This device is not enrolled yet. Ask the operator to add the public key, then try again."
    error is ApiException && error.status >= 500 ->
        "Foyer is unavailable. Try again shortly."
    error is UnknownHostException || error is ConnectException || error is SocketTimeoutException ->
        "Couldn't reach Foyer. Check the connection and try again."
    error.cause is UnknownHostException || error.cause is ConnectException ||
        error.cause is SocketTimeoutException ->
        "Couldn't reach Foyer. Check the connection and try again."
    else ->
        "Couldn't authenticate this device. Try again after the operator adds the public key."
}

private val UNAUTHORIZED_STATUSES = setOf(401, 403, 404)
