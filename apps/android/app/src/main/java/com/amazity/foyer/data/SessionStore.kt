package com.amazity.foyer.data

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/**
 * Encrypted cache for leftover non-key secrets from earlier builds.
 *
 * This is not the device signing key. Android Keystore holds that private
 * key under [com.amazity.foyer.auth.AndroidDeviceKeyStore.KEY_ALIAS], and
 * short-lived Foyer access tokens stay in memory on [com.amazity.foyer.auth.AuthSessionCoordinator].
 */
class SessionStore(context: Context) {
    private val preferences = context.applicationContext.getSharedPreferences("foyer_session", Context.MODE_PRIVATE)

    fun readToken(): String? {
        val encoded = preferences.getString(TOKEN_KEY, null) ?: return null
        return runCatching {
            val bytes = Base64.decode(encoded, Base64.NO_WRAP)
            val iv = bytes.copyOfRange(0, IV_SIZE)
            val ciphertext = bytes.copyOfRange(IV_SIZE, bytes.size)
            Cipher.getInstance(TRANSFORMATION).run {
                init(Cipher.DECRYPT_MODE, secretKey(), GCMParameterSpec(128, iv))
                doFinal(ciphertext).decodeToString()
            }
        }.getOrNull()
    }

    fun writeToken(token: String) {
        val encrypted = Cipher.getInstance(TRANSFORMATION).run {
            init(Cipher.ENCRYPT_MODE, secretKey())
            iv + doFinal(token.encodeToByteArray())
        }
        preferences.edit().putString(TOKEN_KEY, Base64.encodeToString(encrypted, Base64.NO_WRAP)).apply()
    }

    fun clear() {
        preferences.edit().clear().apply()
    }

    private fun secretKey(): SecretKey {
        val keyStore = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        (keyStore.getKey(KEY_ALIAS, null) as? SecretKey)?.let { return it }
        return KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore").run {
            init(
                KeyGenParameterSpec.Builder(
                    KEY_ALIAS,
                    KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT,
                )
                    .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                    .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                    .build(),
            )
            generateKey()
        }
    }

    private companion object {
        const val KEY_ALIAS = "foyer-session-v1"
        const val TOKEN_KEY = "bearer_token"
        const val TRANSFORMATION = "AES/GCM/NoPadding"
        const val IV_SIZE = 12
    }
}
