package app.flowtype.filetransfer

import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean
import org.junit.Assert.*
import org.junit.Test

class TransferWorkersTest {
    @Test fun failureCancelsOtherIoAndJoinsWritersBeforeReturning() {
        val writing = CountDownLatch(1)
        val ioCanceled = CountDownLatch(1)
        val writerStopped = AtomicBoolean(false)
        val failure = assertThrows(IllegalStateException::class.java) {
            runTransferWorkers(listOf(0, 1), 2, { ioCanceled.countDown() }) { index ->
                if (index == 0) {
                    writing.countDown()
                    try { ioCanceled.await(5, TimeUnit.SECONDS) }
                    finally { writerStopped.set(true) }
                } else {
                    assertTrue(writing.await(5, TimeUnit.SECONDS))
                    error("disk full")
                }
            }
        }
        assertEquals("disk full", failure.message)
        assertTrue(writerStopped.get())
        assertEquals(0L, ioCanceled.count)
    }
}
