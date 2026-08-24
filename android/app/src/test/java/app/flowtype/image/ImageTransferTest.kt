package app.flowtype.image

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Test

class ImageTransferTest {
    @Test
    fun encodesImageMetadataAndDigest() {
        val image = PreparedImage(
            transferId = "transfer-1",
            bytes = byteArrayOf(1, 2, 3),
            mimeType = "image/jpeg",
            width = 100,
            height = 50,
            original = false,
        )

        val header = JSONObject(image.header("phone-1"))
        assertEquals("image_start", header.getString("type"))
        assertEquals("transfer-1", header.getString("transfer_id"))
        assertEquals(3, header.getInt("byte_length"))
        assertEquals(
            "039058c6f2c0cb492c533b0a4d14ef77cc0f78abccced5287d84a1a2011cfb81",
            header.getString("sha256"),
        )
    }

    @Test
    fun decodesImageReplies() {
        val reply = ImageTransferReply.decode(
            JSONObject("""{"type":"image_ack","transfer_id":"transfer-1"}"""),
        )
        assertEquals(ImageTransferReply.Ack("transfer-1"), reply)
    }
}
