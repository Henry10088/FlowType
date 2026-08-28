package app.flowtype.session

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class ComputerSessionsTest {
    @Test
    fun switchingAtoBtoARestoresIndependentSessionAndSequence() {
        val stored = mutableMapOf<String, ComputerSessions.ParkedSession>()
        var nextId = 0
        val sessions = ComputerSessions(
            phoneId = "phone",
            sessionIdFactory = { "session-${++nextId}" },
            load = stored::get,
            save = stored::set,
            clear = stored::remove,
        )

        sessions.activate("a")
        sessions.current.onTextChanged("A 初稿")
        sessions.current.onTextChanged("A 修正稿")
        val aState = sessions.current.state()
        sessions.saveCurrent(remoteStarted = true)

        sessions.activate("b")
        assertEquals("", sessions.current.currentText)
        sessions.current.onTextChanged("B 正文")
        val bSessionId = sessions.current.sessionId
        sessions.saveCurrent(remoteStarted = true)

        val restored = sessions.activate("a")
        assertTrue(restored?.remoteStarted == true)
        assertEquals(aState, sessions.current.state())
        assertNotEquals(bSessionId, sessions.current.sessionId)
    }

    @Test
    fun removingOneComputerDoesNotDeleteAnotherDraft() {
        val stored = mutableMapOf<String, ComputerSessions.ParkedSession>()
        val sessions = ComputerSessions(
            phoneId = "phone",
            sessionIdFactory = { "session" },
            load = stored::get,
            save = stored::set,
            clear = stored::remove,
        )
        sessions.activate("a")
        sessions.current.onTextChanged("A")
        sessions.saveCurrent(true)
        sessions.activate("b")
        sessions.current.onTextChanged("B")
        sessions.saveCurrent(true)

        sessions.remove("a")

        assertNull(stored["a"])
        assertEquals("B", stored["b"]?.state?.text)
    }
}
