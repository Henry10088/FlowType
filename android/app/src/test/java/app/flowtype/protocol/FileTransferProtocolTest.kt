package app.flowtype.protocol

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

class FileTransferProtocolTest {
    @Test
    fun offerOnlyContainsABoundedSummary() {
        val offer = FileOfferMessage(
            phoneId = "phone-1",
            batchId = "batch-1",
            direction = TransferDirection.PHONE_TO_COMPUTER,
            fileCount = 2,
            directoryCount = 1,
            totalBytes = Long.MAX_VALUE,
            roots = listOf("资料"),
        )
        offer.validate("phone-1")

        val encoded = FileTransferCodec.encodeOffer(offer)
        val decoded = FileTransferCodec.decodeOffer(encoded)
        assertEquals(offer, decoded)
        assertFalse(org.json.JSONObject(encoded).has("entries"))
    }

    @Test
    fun parsesSharedContractFixtures() {
        val input = checkNotNull(javaClass.classLoader?.getResourceAsStream("file-transfer-messages.json"))
        val fixtures = org.json.JSONArray(input.bufferedReader().use { it.readText() })

        val offer = FileTransferCodec.decodeOffer(fixtures.getJSONObject(0).toString())
        offer.validate("phone-1")
        assertEquals(TransferDirection.PHONE_TO_COMPUTER, offer.direction)

        val ticket = FileTransferCodec.decodeTicket(fixtures.getJSONObject(1).toString())
        ticket.validate(1_700_000_000)
        assertEquals(32188, ticket.dataPort)

        val entry = FileTransferCodec.decodeManifestEntry(fixtures.getJSONObject(2).toString())
        assertEquals("资料/说明.txt", entry.relativePath)
    }

    @Test
    fun rejectsTraversalAndWindowsSeparators() {
        for (path in listOf("../secret", "a/../secret", "/root", "a\\b", "a//b", "a/")) {
            assertThrows(IllegalArgumentException::class.java) {
                validateRelativeTransferPath(path)
            }
        }
    }

    @Test
    fun validatesResumeOffsetsAndTerminalStates() {
        TransferResumeEntry("entry-1", 16, TransferEntryState.WAITING_NETWORK).validate(42)
        assertThrows(IllegalArgumentException::class.java) {
            TransferResumeEntry("entry-1", 41, TransferEntryState.COMPLETED).validate(42)
        }
        assertTrue(TransferBatchState.QUEUED.canTransitionTo(TransferBatchState.PREPARING))
        assertTrue(TransferBatchState.TRANSFERRING.canTransitionTo(TransferBatchState.WAITING_NETWORK))
        assertFalse(TransferBatchState.COMPLETED.canTransitionTo(TransferBatchState.TRANSFERRING))
    }

    @Test
    fun ticketMustBeUnexpiredAndAtLeast256Bits() {
        val ticket = FileTicketMessage(
            batchId = "batch-1",
            dataPort = 32188,
            token = "a".repeat(43),
            expiresAtEpochSeconds = 200,
        )
        ticket.validate(100)
        assertThrows(IllegalArgumentException::class.java) { ticket.validate(200) }
    }
}
