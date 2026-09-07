package app.flowtype.filetransfer

import app.flowtype.protocol.TransferDirection
import java.io.FileNotFoundException
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class FileTransferFailureTest {
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
