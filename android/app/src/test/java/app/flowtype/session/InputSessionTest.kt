package app.flowtype.session

import app.flowtype.protocol.AckMessage
import app.flowtype.protocol.ServerSessionState
import app.flowtype.protocol.SnapshotType
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class InputSessionTest {
    private fun session() = InputSession("phone") { "session-1" }

    @Test
    fun firstNonEmptyChangeStartsSession() {
        val session = session()
        assertNull(session.onTextChanged(""))
        val message = session.onTextChanged("你好")!!
        assertEquals(SnapshotType.START, message.type)
        assertEquals(1L, message.sequence)
        assertEquals("你好", message.fullText)
    }

    @Test
    fun laterChangesCarryCompleteState() {
        val session = session()
        session.onTextChanged("你好")
        val update = session.onTextChanged("你好吗")!!
        assertEquals(SnapshotType.UPDATE, update.type)
        assertEquals(2L, update.sequence)
        assertEquals("你好吗", update.fullText)
    }

    @Test
    fun finalAckFreezesSession() {
        val session = session()
        session.onTextChanged("完成内容")
        val finish = session.finish()!!
        assertFalse(session.finished)
        session.acknowledge(
            AckMessage(finish.sessionId, finish.sequence, ServerSessionState.FINISHED),
        )
        assertTrue(session.finished)
    }

    @Test
    fun restoresAnUnfinishedSessionWithoutChangingItsSequence() {
        val session = session()
        session.restore(
            InputSession.State("session-1", "保留正文", 7, 5, finishing = false),
        )
        val snapshot = session.recoverySnapshot()!!
        assertEquals(7L, snapshot.sequence)
        assertEquals("保留正文", snapshot.fullText)
        assertEquals(5L, session.acknowledgedSequence)
    }

    @Test
    fun historyTextBecomesLocalDraftWithoutStartingRemoteSession() {
        val session = session()
        session.replaceLocalDraft("历史正文")
        assertNull(session.sessionId)
        assertEquals("历史正文", session.currentText)
        assertEquals(0L, session.latestSequence)

        val start = session.startLocalDraft()!!
        assertEquals(SnapshotType.START, start.type)
        assertEquals("历史正文", start.fullText)
        assertEquals(1L, start.sequence)
    }

    @Test(expected = IllegalStateException::class)
    fun activeSessionCannotBeReplacedByHistory() {
        val session = session()
        session.onTextChanged("正在输入")
        session.replaceLocalDraft("历史正文")
    }

    @Test
    fun finishingSessionCanRestartAtCurrentCursorWithoutLosingText() {
        val session = session()
        session.onTextChanged("保留的完整文本")
        session.finish()

        val messages = session.restartAtCurrentCursor()

        assertEquals(2, messages.size)
        assertEquals(SnapshotType.START, messages[0].type)
        assertEquals(SnapshotType.FINISH, messages[1].type)
        assertEquals("保留的完整文本", messages[0].fullText)
        assertEquals(messages[0].sessionId, messages[1].sessionId)
        assertTrue(session.finishing)
    }

    @Test
    fun activeSessionCanMoveToCurrentCursorAndRemainEditable() {
        val session = session()
        session.onTextChanged("移动后继续输入")

        val messages = session.restartAtCurrentCursor()

        assertEquals(1, messages.size)
        assertEquals(SnapshotType.START, messages.single().type)
        assertEquals("移动后继续输入", messages.single().fullText)
        assertEquals(1L, messages.single().sequence)
        assertFalse(session.finishing)
        assertEquals(SnapshotType.UPDATE, session.onTextChanged("移动后继续输入。")!!.type)
    }
}
