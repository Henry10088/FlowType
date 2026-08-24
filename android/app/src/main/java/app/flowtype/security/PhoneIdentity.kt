package app.flowtype.security

import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import java.security.KeyPairGenerator
import java.security.KeyStore
import java.security.Signature

class PhoneIdentity {
    private val keyStore = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }

    fun publicKey(pcId: String): ByteArray = keyPair(pcId).certificate.publicKey.encoded

    fun sign(pcId: String, payload: ByteArray): ByteArray {
        val entry = keyPair(pcId)
        return Signature.getInstance("SHA256withECDSA").run {
            initSign(entry.privateKey)
            update(payload)
            sign()
        }
    }

    fun delete(pcId: String) {
        keyStore.deleteEntry(alias(pcId))
    }

    private fun keyPair(pcId: String): KeyStore.PrivateKeyEntry {
        val alias = alias(pcId)
        (keyStore.getEntry(alias, null) as? KeyStore.PrivateKeyEntry)?.let { return it }
        KeyPairGenerator.getInstance(KeyProperties.KEY_ALGORITHM_EC, "AndroidKeyStore").apply {
            initialize(
                KeyGenParameterSpec.Builder(alias, KeyProperties.PURPOSE_SIGN)
                    .setAlgorithmParameterSpec(java.security.spec.ECGenParameterSpec("secp256r1"))
                    .setDigests(KeyProperties.DIGEST_SHA256)
                    .setUserAuthenticationRequired(false)
                    .build(),
            )
            generateKeyPair()
        }
        return keyStore.getEntry(alias, null) as KeyStore.PrivateKeyEntry
    }

    private fun alias(pcId: String) = "flowtype-phone-v1-$pcId"
}
