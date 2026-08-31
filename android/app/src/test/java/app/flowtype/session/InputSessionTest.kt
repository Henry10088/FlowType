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
    @Test
    fun failedDraftChangesStayLocalUntilExplicitlyStarted() {
        val session = InputSession("phone") { "session-1" }

        assertNull(session.onTextChanged("语音草稿", startIfNeeded = false))
        assertNull(session.onTextChanged("语音修正稿", startIfNeeded = false))
        assertNull(session.sessionId)
        assertEquals("语音修正稿", session.currentText)
        assertEquals(0L, session.latestSequence)

        val start = session.startLocalDraft()!!
        assertEquals(SnapshotType.START, start.type)
        assertEquals("session-1", start.sessionId)
        assertEquals("语音修正稿", start.fullText)
    }

    private fun session() = InputSession("phone") { "session-1" }

    @Test
    fun firstNonEmptyChangeStartsSession() {
        val session = session()
        assertNull(session.onTextChanged(""))
        val message = session.onTextChanged("你好")!!
        assertEquals(SnapshotType.START, message.type)
        assertEquals(1L, message.sequence)
        assertEquals("你好", message.fullText)
        assertFalse(message.attachExisting)
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
    fun clearingActiveSessionSendsEmptyUpdateWithoutEndingSession() {
        val session = session()
        session.onTextChanged("待清空")

        val clear = session.onTextChanged("")!!

        assertEquals(SnapshotType.UPDATE, clear.type)
        assertEquals(2L, clear.sequence)
        assertEquals("", clear.fullText)
        assertEquals("session-1", session.sessionId)
        assertFalse(session.finishing)
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
    fun completedSessionCanStartNextVoiceSessionWithoutReusingText() {
        val session = session()
        session.onTextChanged("第一")
        val submitted = session.onTextChanged("第一句\n")!!
        val finish = session.finish()!!
        assertEquals(2L, submitted.sequence)
        assertEquals(submitted.sequence, finish.sequence)

        session.acknowledge(
            AckMessage(finish.sessionId, finish.sequence, ServerSessionState.FINISHED),
        )
        assertTrue(session.finished)

        session.reset()
        val next = session.onTextChanged("第二句")!!
        assertEquals(SnapshotType.START, next.type)
        assertEquals(1L, next.sequence)
        assertEquals("第二句", next.fullText)
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
        var nextId = 0
        val session = InputSession("phone") { "session-${++nextId}" }
        session.onTextChanged("保留的完整文本")
        session.finish()

        val messages = session.restartAtCurrentCursor()

        assertEquals(2, messages.size)
        assertEquals(SnapshotType.START, messages[0].type)
        assertEquals(SnapshotType.FINISH, messages[1].type)
        assertEquals("保留的完整文本", messages[0].fullText)
        assertEquals("session-1", messages[0].replacesSessionId)
        assertEquals("session-2", messages[0].sessionId)
        assertTrue(messages[0].attachExisting)
        assertEquals(messages[0].sessionId, messages[1].sessionId)
        assertTrue(session.finishing)
    }

    @Test
    fun activeSessionCanMoveToCurrentCursorAndRemainEditable() {
        var nextId = 0
        val session = InputSession("phone") { "session-${++nextId}" }
        session.onTextChanged("移动后继续输入")

        val messages = session.restartAtCurrentCursor()

        assertEquals(1, messages.size)
        assertEquals(SnapshotType.START, messages.single().type)
        assertEquals("移动后继续输入", messages.single().fullText)
        assertEquals("session-1", messages.single().replacesSessionId)
        assertEquals("session-2", messages.single().sessionId)
        assertEquals(1L, messages.single().sequence)
        assertTrue(messages.single().attachExisting)
        assertFalse(session.finishing)
        val update = session.onTextChanged("移动后继续输入。")!!
        assertEquals(SnapshotType.UPDATE, update.type)
        assertEquals("session-1", update.replacesSessionId)

        session.acknowledge(AckMessage("session-2", 2, ServerSessionState.ACTIVE))
        assertNull(session.replacementSessionId)
    }

    @Test
    fun pendingReplacementSurvivesStatePersistenceBeforeStartAck() {
        var nextId = 0
        val original = InputSession("phone") { "session-${++nextId}" }
        original.onTextChanged("原位置")
        original.restartAtCurrentCursor()

        val restored = InputSession("phone") { "unused" }
        restored.restore(original.state())

        val recovery = restored.recoverySnapshot()!!
        assertEquals("session-1", recovery.replacesSessionId)
        assertEquals("session-2", recovery.sessionId)
        assertTrue(recovery.attachExisting)
    }

    @Test
    fun explicitLocalDraftStartChecksOnlyTheNewCursor() {
        val session = session()
        session.replaceLocalDraft("已有草稿")

        val start = session.startLocalDraft(attachExistingAtCursor = true)!!
        val update = session.onTextChanged("已有草稿修正")!!

        assertTrue(start.attachExisting)
        assertTrue(update.attachExisting)
        session.acknowledge(AckMessage("session-1", 2, ServerSessionState.ACTIVE))
        assertFalse(session.state().attachExistingAtCursor)
    }
}
