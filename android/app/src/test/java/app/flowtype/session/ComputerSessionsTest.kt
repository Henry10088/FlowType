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

    @Test
    fun inputEnteredDuringAutomaticSelectionMovesToTheChosenComputer() {
        val stored = mutableMapOf<String, ComputerSessions.ParkedSession>()
        val sessions = ComputerSessions(
            phoneId = "phone",
            sessionIdFactory = { "auto-session" },
            load = stored::get,
            save = stored::set,
            clear = stored::remove,
        )
        sessions.activate("fallback")
        sessions.current.onTextChanged("探测期间输入")
        sessions.current.onTextChanged("探测期间修正后的输入")
        val movingState = sessions.current.state()
        sessions.saveCurrent(remoteStarted = false)

        assertEquals(
            ComputerSessions.AutomaticActivation.Activated(null),
            sessions.activateForAutomaticSelection("winner"),
        )
        sessions.saveCurrent(remoteStarted = false)

        assertEquals("winner", sessions.activePcId)
        assertEquals(movingState, sessions.current.state())
        assertEquals(movingState, stored["winner"]?.state)
        assertNull(stored["fallback"])
    }

    @Test
    fun automaticSelectionCanDetectAConflictingParkedSession() {
        val stored = mutableMapOf<String, ComputerSessions.ParkedSession>()
        stored["winner"] = ComputerSessions.ParkedSession(
            InputSession.State("parked", "原有正文", 1, 1, false),
            remoteStarted = true,
        )
        val sessions = ComputerSessions(
            phoneId = "phone",
            sessionIdFactory = { "new" },
            load = stored::get,
            save = stored::set,
            clear = stored::remove,
        )
        sessions.activate("fallback")
        sessions.current.onTextChanged("新正文")
        sessions.saveCurrent(remoteStarted = false)
        val fallbackBeforeActivation = stored.getValue("fallback")
        val winnerBeforeActivation = stored.getValue("winner")

        assertEquals(
            ComputerSessions.AutomaticActivation.Conflict,
            sessions.activateForAutomaticSelection("winner"),
        )
        assertEquals("fallback", sessions.activePcId)
        assertEquals("新正文", sessions.current.currentText)
        assertEquals(fallbackBeforeActivation, stored["fallback"])
        assertEquals(winnerBeforeActivation, stored["winner"])
    }
}
