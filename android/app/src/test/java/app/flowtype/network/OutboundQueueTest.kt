package app.flowtype.network

import app.flowtype.protocol.AckMessage
import app.flowtype.protocol.ServerSessionState
import app.flowtype.protocol.SnapshotMessage
import app.flowtype.protocol.SnapshotType
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class OutboundQueueTest {
    @Test
    fun keepsOneInFlightAndOnlyTheLatestPendingState() {
        val queue = OutboundQueue()
        queue.onConnected()
        assertEquals(1L, queue.offer(snapshot(1, "一"))!!.snapshot.sequence)
        assertNull(queue.offer(snapshot(2, "二")))
        assertNull(queue.offer(snapshot(3, "三")))

        val next = queue.acknowledge(AckMessage("session", 1, ServerSessionState.ACTIVE))!!
        assertEquals(3L, next.snapshot.sequence)
        assertEquals("三", next.snapshot.fullText)
    }

    @Test
    fun reconnectsWithTheLatestCompleteState() {
        val queue = OutboundQueue()
        queue.onConnected()
        queue.offer(snapshot(1, "旧"))
        queue.markSessionStarted()
        queue.onDisconnected()
        queue.offer(snapshot(4, "最新完整状态"))

        val resumed = queue.onConnected()!!
        assertTrue(resumed.resume)
        assertEquals(4L, resumed.snapshot.sequence)
        assertEquals("最新完整状态", resumed.snapshot.fullText)
    }

    @Test
    fun reconnectsBeforeStartConfirmationWithAStartSnapshot() {
        val queue = OutboundQueue()
        queue.onConnected()
        queue.offer(snapshot(1, "旧"))
        queue.offer(snapshot(2, "最新"))
        queue.onDisconnected()

        val restarted = queue.onConnected()!!
        assertFalse(restarted.resume)
        assertEquals(SnapshotType.START, restarted.snapshot.type)
        assertEquals(2L, restarted.snapshot.sequence)
        assertEquals("最新", restarted.snapshot.fullText)
    }

    @Test
    fun finalAckClearsTheSession() {
        val queue = OutboundQueue()
        queue.onConnected()
        queue.offer(snapshot(1, "完成", SnapshotType.FINISH))
        queue.markSessionStarted()
        queue.acknowledge(AckMessage("session", 1, ServerSessionState.FINISHED))

        assertNull(queue.currentInFlight())
        assertFalse(queue.onConnected()?.resume ?: false)
    }

    @Test
    fun offlineDraftWaitsForAnExplicitFullSync() {
        val queue = OutboundQueue()
        queue.offer(snapshot(1, "离线草稿"))

        assertNull(queue.onConnected())
        assertTrue(queue.requiresExplicitStart())
        val start = queue.startOfflineDraft()!!
        assertEquals(SnapshotType.START, start.snapshot.type)
        assertEquals("离线草稿", start.snapshot.fullText)
    }

    @Test
    fun restoredActiveSessionResumesButRestoredOfflineDraftWaits() {
        val active = OutboundQueue()
        active.restore(snapshot(4, "活跃"), 2, remoteStarted = true)
        assertTrue(active.onConnected()!!.resume)

        val offline = OutboundQueue()
        offline.restore(snapshot(4, "离线"), 0, remoteStarted = false)
        assertNull(offline.onConnected())
        assertTrue(offline.requiresExplicitStart())
    }

    @Test
    fun serverRejectionKeepsTheLatestTextForAnExplicitRestart() {
        val queue = OutboundQueue()
        queue.onConnected()
        queue.offer(snapshot(1, "不会丢失"))

        queue.requireExplicitStart()

        assertTrue(queue.requiresExplicitStart())
        val restarted = queue.startOfflineDraft()!!
        assertEquals(SnapshotType.START, restarted.snapshot.type)
        assertEquals("不会丢失", restarted.snapshot.fullText)
    }

    @Test
    fun abandoningASessionAllowsANewSessionAndIgnoresTheOldAck() {
        val queue = OutboundQueue()
        queue.onConnected()
        queue.offer(snapshot(1, "旧会话", sessionId = "old"))

        queue.abandonSession()
        val next = queue.offer(snapshot(1, "新会话", sessionId = "new"))!!

        assertEquals("new", next.snapshot.sessionId)
        assertNull(queue.acknowledge(AckMessage("old", 9, ServerSessionState.FINISHED)))
        assertEquals("new", queue.currentInFlight()?.sessionId)
        assertTrue(queue.acceptsSession("new"))
        assertFalse(queue.acceptsSession("old"))
    }

    @Test
    fun abandoningWhileDisconnectedDoesNotPretendTheQueueIsConnected() {
        val queue = OutboundQueue()
        queue.offer(snapshot(1, "旧会话", sessionId = "old"))

        queue.abandonSession()

        assertNull(queue.offer(snapshot(1, "新会话", sessionId = "new")))
    }

    @Test
    fun retargetCanStartAndFinishWithTheSameCompleteState() {
        val queue = OutboundQueue()
        queue.onConnected()
        queue.offer(snapshot(4, "旧目标", sessionId = "old"))
        queue.abandonSession()

        val start = snapshot(1, "完整文本", SnapshotType.START, "new")
        val finish = snapshot(1, "完整文本", SnapshotType.FINISH, "new")
        assertEquals(SnapshotType.START, queue.offer(start)!!.snapshot.type)
        assertNull(queue.offer(finish))

        val next = queue.acknowledge(AckMessage("new", 1, ServerSessionState.ACTIVE))!!
        assertEquals(SnapshotType.FINISH, next.snapshot.type)
        assertEquals("完整文本", next.snapshot.fullText)
    }

    private fun snapshot(
        sequence: Long,
        text: String,
        type: SnapshotType = if (sequence == 1L) SnapshotType.START else SnapshotType.UPDATE,
        sessionId: String = "session",
    ) = SnapshotMessage(type, "phone", sessionId, sequence, text)
}
