package app.flowtype.session

import app.flowtype.protocol.AckMessage
import app.flowtype.protocol.SnapshotMessage
import app.flowtype.protocol.SnapshotType

class InputSession(
    private val phoneId: String,
    private val sessionIdFactory: () -> String,
) {
    data class State(
        val sessionId: String?,
        val text: String,
        val latestSequence: Long,
        val acknowledgedSequence: Long,
        val finishing: Boolean,
        val replacementSessionId: String? = null,
        val attachExistingAtCursor: Boolean = false,
    )

    var sessionId: String? = null
        private set
    var currentText: String = ""
        private set
    var latestSequence: Long = 0
        private set
    var acknowledgedSequence: Long = 0
        private set
    var finishing: Boolean = false
        private set
    var finished: Boolean = false
        private set
    var replacementSessionId: String? = null
        private set
    private var attachExistingAtCursor: Boolean = false

    fun onTextChanged(text: String, startIfNeeded: Boolean = true): SnapshotMessage? {
        check(!finishing && !finished) { "session is not editable" }
        if (text == currentText) return null
        if (sessionId == null && (text.isEmpty() || !startIfNeeded)) {
            currentText = text
            return null
        }

        val type = if (sessionId == null) SnapshotType.START else SnapshotType.UPDATE
        if (sessionId == null) {
            sessionId = sessionIdFactory()
            attachExistingAtCursor = false
        }
        currentText = text
        latestSequence += 1
        return snapshot(type)
    }

    fun finish(): SnapshotMessage? {
        val id = sessionId ?: return null
        check(!finishing && !finished) { "session already finishing" }
        finishing = true
        if (latestSequence == 0L) latestSequence = 1
        return SnapshotMessage(
            type = SnapshotType.FINISH,
            phoneId = phoneId,
            sessionId = id,
            sequence = latestSequence,
            fullText = currentText,
        )
    }

    fun restartAtCurrentCursor(): List<SnapshotMessage> {
        check(sessionId != null && !finished) { "session cannot be restarted" }
        val finishAfterRestart = finishing
        val text = currentText
        resetForReplacement(text)
        val start = startLocalDraft(attachExistingAtCursor = true) ?: return emptyList()
        return if (finishAfterRestart) listOf(start, checkNotNull(finish())) else listOf(start)
    }

    fun resetForReplacement(text: String = currentText): String? {
        val replaced = sessionId ?: replacementSessionId
        reset()
        replacementSessionId = replaced
        currentText = text
        return replaced
    }

    fun replaceLocalDraft(text: String) {
        check(sessionId == null && !finishing && !finished) { "active session cannot be replaced" }
        currentText = text
    }

    fun startLocalDraft(attachExistingAtCursor: Boolean = false): SnapshotMessage? {
        if (sessionId != null || currentText.isEmpty()) return null
        sessionId = sessionIdFactory()
        this.attachExistingAtCursor = attachExistingAtCursor
        latestSequence = 1
        return snapshot(SnapshotType.START)
    }

    fun acknowledge(ack: AckMessage) {
        if (ack.sessionId != sessionId || ack.appliedSequence < acknowledgedSequence) return
        acknowledgedSequence = minOf(ack.appliedSequence, latestSequence)
        if (acknowledgedSequence > 0) {
            replacementSessionId = null
            attachExistingAtCursor = false
        }
        if (ack.finished && finishing && acknowledgedSequence == latestSequence) {
            finished = true
        }
    }

    fun reset() {
        sessionId = null
        currentText = ""
        latestSequence = 0
        acknowledgedSequence = 0
        finishing = false
        finished = false
        replacementSessionId = null
        attachExistingAtCursor = false
    }

    fun state(): State = State(
        sessionId = sessionId,
        text = currentText,
        latestSequence = latestSequence,
        acknowledgedSequence = acknowledgedSequence,
        finishing = finishing,
        replacementSessionId = replacementSessionId,
        attachExistingAtCursor = attachExistingAtCursor,
    )

    fun restore(state: State) {
        require(state.latestSequence >= state.acknowledgedSequence && state.acknowledgedSequence >= 0)
        require((state.sessionId == null) == (state.latestSequence == 0L))
        sessionId = state.sessionId
        currentText = state.text
        latestSequence = state.latestSequence
        acknowledgedSequence = state.acknowledgedSequence
        finishing = state.finishing
        finished = false
        replacementSessionId = state.replacementSessionId
        attachExistingAtCursor = state.attachExistingAtCursor
    }

    fun recoverySnapshot(): SnapshotMessage? {
        val id = sessionId ?: return null
        return SnapshotMessage(
            type = if (finishing) SnapshotType.FINISH else SnapshotType.UPDATE,
            phoneId = phoneId,
            sessionId = id,
            sequence = latestSequence,
            fullText = currentText,
            replacesSessionId = replacementSessionId,
            attachExisting = attachExistingAtCursor,
        )
    }

    private fun snapshot(type: SnapshotType): SnapshotMessage = SnapshotMessage(
        type = type,
        phoneId = phoneId,
        sessionId = checkNotNull(sessionId),
        sequence = latestSequence,
        fullText = currentText,
        replacesSessionId = replacementSessionId,
        attachExisting = attachExistingAtCursor,
    )
}
