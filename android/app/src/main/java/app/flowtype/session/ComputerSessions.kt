package app.flowtype.session

class ComputerSessions(
    private val phoneId: String,
    private val sessionIdFactory: () -> String,
    private val load: (String) -> ParkedSession?,
    private val save: (String, ParkedSession) -> Unit,
    private val clear: (String) -> Unit,
) {
    data class ParkedSession(
        val state: InputSession.State,
        val remoteStarted: Boolean,
    )

    var activePcId: String? = null
        private set
    var current: InputSession = newSession()
        private set

    fun activate(pcId: String): ParkedSession? {
        if (activePcId == pcId) {
            return load(pcId)
        }
        val stored = load(pcId)
        val carryUnassignedDraft = activePcId == null && stored == null && current.currentText.isNotEmpty()
        if (!carryUnassignedDraft) {
            current = newSession().also { session -> stored?.let { session.restore(it.state) } }
        }
        activePcId = pcId
        return stored
    }

    fun saveCurrent(remoteStarted: Boolean) {
        val pcId = activePcId ?: return
        val parked = ParkedSession(current.state(), remoteStarted)
        if (parked.state.sessionId == null && parked.state.text.isEmpty()) {
            clear(pcId)
        } else {
            save(pcId, parked)
        }
    }

    fun clearCurrent() {
        activePcId?.let(clear)
        current.reset()
    }

    fun remove(pcId: String) {
        clear(pcId)
        if (activePcId == pcId) {
            activePcId = null
            current = newSession()
        }
    }

    private fun newSession() = InputSession(phoneId, sessionIdFactory)
}
