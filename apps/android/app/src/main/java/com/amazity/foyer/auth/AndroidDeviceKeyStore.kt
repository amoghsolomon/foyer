package com.amazity.foyer.auth

import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import java.security.KeyPairGenerator
import java.security.KeyStore
import java.security.Signature
import java.security.interfaces.ECPublicKey
import java.security.spec.ECGenParameterSpec

class AndroidDeviceKeyStore : DeviceSigner {
    @Volatile private var cached: DevicePublicMaterial? = null

    override fun material(): DevicePublicMaterial {
        cached?.let { return it }
        synchronized(LOCK) {
            cached?.let { return it }
            val keyStore = loadedKeyStore()
            if (keyStore.containsAlias(KEY_ALIAS).not()) {
                generateKey()
                keyStore.load(null)
            }
            val publicKey = keyStore.getCertificate(KEY_ALIAS).publicKey as ECPublicKey
            return DevicePublicMaterial(DevicePublicJwk.fromEcPublicKey(publicKey)).also { cached = it }
        }
    }

    override fun signSha256(payload: ByteArray): ByteArray {
        material()
        val keyStore = loadedKeyStore()
        val privateKey = keyStore.getKey(KEY_ALIAS, null)
            ?: error("Foyer device signing key is missing")
        val der = Signature.getInstance("SHA256withECDSA").run {
            initSign(privateKey as java.security.PrivateKey)
            update(payload)
            sign()
        }
        return EcdsaP1363.derToIeeeP1363(der)
    }

    private fun generateKey() {
        KeyPairGenerator.getInstance(KeyProperties.KEY_ALGORITHM_EC, ANDROID_KEYSTORE).run {
            initialize(
                KeyGenParameterSpec.Builder(KEY_ALIAS, KeyProperties.PURPOSE_SIGN)
                    .setAlgorithmParameterSpec(ECGenParameterSpec("secp256r1"))
                    .setDigests(KeyProperties.DIGEST_SHA256)
                    .setUserAuthenticationRequired(false)
                    .build(),
            )
            generateKeyPair()
        }
    }

    private fun loadedKeyStore(): KeyStore =
        KeyStore.getInstance(ANDROID_KEYSTORE).apply { load(null) }

    companion object {
        const val KEY_ALIAS = "foyer-device-signing-v1"
        private const val ANDROID_KEYSTORE = "AndroidKeyStore"
        private val LOCK = Any()
    }
}
