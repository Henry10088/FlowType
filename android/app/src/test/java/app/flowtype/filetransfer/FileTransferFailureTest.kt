package app.flowtype.filetransfer

import app.flowtype.protocol.TransferDirection
import app.flowtype.protocol.TransferBatchState
import app.flowtype.network.fileTransferBaseUrl
import app.flowtype.network.FILE_IO_IDLE_TIMEOUT_SECONDS
import java.io.FileNotFoundException
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class FileTransferFailureTest {
    @Test
    fun stalledDataConnectionHasAFiniteRecoveryTimeout() {
        assertEquals(30L, FILE_IO_IDLE_TIMEOUT_SECONDS)
    }

    @Test
    fun processRecoveryDoesNotMoveAnActiveBatchBackwards() {
        assertEquals(true, shouldEnterPreparing(TransferBatchState.QUEUED))
        assertEquals(true, shouldEnterPreparing(TransferBatchState.WAITING_NETWORK))
        assertEquals(false, shouldEnterPreparing(TransferBatchState.PREPARING))
        assertEquals(false, shouldEnterPreparing(TransferBatchState.TRANSFERRING))
    }

    @Test
    fun dataChannelConvertsWebSocketSchemesBeforeHttpParsing() {
        assertEquals(
            "https://10.0.2.2:32187/v1/sync",
            fileTransferBaseUrl("wss://10.0.2.2:32187/v1/sync"),
        )
    }

    @Test
    fun onlyPermanentPerFileErrorsAreSkipped() {
        assertEquals(
            "source_unavailable",
            permanentEntryFailureCode(
                FileNotFoundException("gone"),
                TransferDirection.PHONE_TO_COMPUTER,
            ),
        )
        assertEquals(
            "source_changed",
            permanentEntryFailureCode(
                IllegalStateException("file download failed: source_changed"),
                TransferDirection.COMPUTER_TO_PHONE,
            ),
        )
        assertEquals(
            "integrity_failed",
            permanentEntryFailureCode(
                IllegalStateException("file integrity check failed"),
                TransferDirection.PHONE_TO_COMPUTER,
            ),
        )
        assertNull(
            permanentEntryFailureCode(
                IllegalStateException("connection reset"),
                TransferDirection.PHONE_TO_COMPUTER,
            ),
        )
        assertNull(
            permanentEntryFailureCode(
                SecurityException("target permission lost"),
                TransferDirection.COMPUTER_TO_PHONE,
            ),
        )
    }
}
