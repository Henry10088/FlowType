package app.flowtype.pairing

import java.security.MessageDigest
import java.util.UUID

private const val PHONE_ID_NAMESPACE = "flowtype-phone-id-v2\u0000"

/**
 * Returns a stable, non-reversible phone identifier for this Android device.
 *
 * The identifier is not an authentication secret. Authentication still uses
 * the per-computer Keystore key. Hashing keeps the platform Android ID out of
 * the pairing protocol while allowing a reinstall to address the same phone
 * consistently when the platform ID is unchanged.
 */
internal fun phoneIdForAndroidId(androidId: String?): String? {
    val normalized = androidId?.trim().orEmpty()
    if (normalized.isEmpty()) return null

    val digest = MessageDigest.getInstance("SHA-256").digest(
        (PHONE_ID_NAMESPACE + normalized).toByteArray(Charsets.UTF_8),
    )
    return digest.joinToString(separator = "") { byte -> "%02x".format(byte) }
}

internal fun randomPhoneId(): String = UUID.randomUUID().toString()
