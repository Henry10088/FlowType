package app.flowtype.image

import app.flowtype.protocol.PROTOCOL_VERSION
import org.json.JSONObject
import java.security.MessageDigest
import java.util.UUID

data class PreparedImage(
    val transferId: String = UUID.randomUUID().toString(),
    val bytes: ByteArray,
    val mimeType: String,
    val width: Int,
    val height: Int,
    val original: Boolean,
) {
    val sha256: String = MessageDigest.getInstance("SHA-256")
        .digest(bytes)
        .joinToString("") { "%02x".format(it) }

    fun header(phoneId: String): String = JSONObject()
        .put("protocol_version", PROTOCOL_VERSION)
        .put("type", "image_start")
        .put("transfer_id", transferId)
        .put("phone_id", phoneId)
        .put("mime_type", mimeType)
        .put("width", width)
        .put("height", height)
        .put("byte_length", bytes.size)
        .put("sha256", sha256)
        .put("original", original)
        .toString()
}

sealed interface ImageTransferReply {
    val transferId: String

    data class Ack(override val transferId: String) : ImageTransferReply
    data class Error(override val transferId: String, val code: String) : ImageTransferReply

    companion object {
        fun decode(json: JSONObject): ImageTransferReply = when (json.getString("type")) {
            "image_ack" -> Ack(json.getString("transfer_id"))
            "image_error" -> Error(json.getString("transfer_id"), json.optString("code", "unknown"))
            else -> error("not an image reply")
        }
    }
}
