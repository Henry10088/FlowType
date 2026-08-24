package app.flowtype.security

import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

class SecureTextCipher(private val keyAlias: String) {
    fun encrypt(text: String): ByteArray {
        val cipher = Cipher.getInstance(TRANSFORMATION).apply {
            init(Cipher.ENCRYPT_MODE, key())
        }
        return cipher.iv + cipher.doFinal(text.toByteArray(Charsets.UTF_8))
    }

    fun decrypt(encrypted: ByteArray): String {
        require(encrypted.size > IV_BYTES)
        val cipher = Cipher.getInstance(TRANSFORMATION).apply {
            init(
                Cipher.DECRYPT_MODE,
                key(),
                GCMParameterSpec(128, encrypted.copyOfRange(0, IV_BYTES)),
            )
        }
        return String(cipher.doFinal(encrypted.copyOfRange(IV_BYTES, encrypted.size)), Charsets.UTF_8)
    }

    fun destroyKey() {
        KeyStore.getInstance("AndroidKeyStore").apply { load(null) }.deleteEntry(keyAlias)
    }

    private fun key(): SecretKey {
        val keyStore = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        (keyStore.getKey(keyAlias, null) as? SecretKey)?.let { return it }
        return KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore").run {
            init(
                KeyGenParameterSpec.Builder(
                    keyAlias,
                    KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT,
                )
                    .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                    .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                    .setUserAuthenticationRequired(false)
                    .build(),
            )
            generateKey()
        }
    }

    companion object {
        private const val TRANSFORMATION = "AES/GCM/NoPadding"
        private const val IV_BYTES = 12
    }
}
