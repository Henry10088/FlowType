package app.flowtype.network

import app.flowtype.protocol.AckMessage
import app.flowtype.protocol.SnapshotMessage
import app.flowtype.protocol.SnapshotType

data class OutboundAction(
    val snapshot: SnapshotMessage,
    val resume: Boolean,
)

class OutboundQueue {
    private var connected = false
    private var sessionStarted = false
    private var startAttempted = false
    private var inFlight: SnapshotMessage? = null
    private var pending: SnapshotMessage? = null
    private var latest: SnapshotMessage? = null
    var lastAckSequence: Long = 0
        private set

    fun offer(snapshot: SnapshotMessage): OutboundAction? {
        latest = snapshot
        if (!connected || inFlight != null) {
            pending = snapshot
            return null
        }
        inFlight = snapshot
        if (snapshot.type == SnapshotType.START) startAttempted = true
        return OutboundAction(snapshot, resume = false)
    }

    fun onConnected(): OutboundAction? {
        connected = true
        val latestSnapshot = latest ?: pending ?: inFlight ?: return null
        if (!sessionStarted && !startAttempted) return null
        // If START was sent but its target/session confirmation was lost, restart
        // the remote session with the latest complete text instead of sending UPDATE.
        val snapshot = if (!sessionStarted) {
            latestSnapshot.copy(type = SnapshotType.START)
        } else {
            latestSnapshot
        }
        inFlight = snapshot
        pending = null
        return OutboundAction(snapshot, resume = sessionStarted)
    }

    fun onDisconnected() {
        connected = false
    }

    fun markSessionStarted() {
        sessionStarted = true
    }

    fun restore(snapshot: SnapshotMessage, acknowledgedSequence: Long, remoteStarted: Boolean) {
        require(acknowledgedSequence in 0..snapshot.sequence)
        latest = snapshot
        pending = snapshot
        lastAckSequence = acknowledgedSequence
        sessionStarted = remoteStarted
        startAttempted = remoteStarted
    }

    fun startOfflineDraft(): OutboundAction? {
        if (!connected || sessionStarted || startAttempted) return null
        val snapshot = latest ?: pending ?: return null
        val start = snapshot.copy(type = SnapshotType.START)
        inFlight = start
        pending = null
        startAttempted = true
        return OutboundAction(start, resume = false)
    }

    fun requireExplicitStart() {
        sessionStarted = false
        startAttempted = false
        inFlight = null
        pending = latest
    }

    fun requiresExplicitStart(): Boolean =
        connected && latest != null && !sessionStarted && !startAttempted

    fun remoteStarted(): Boolean = sessionStarted

    fun acknowledge(ack: AckMessage): OutboundAction? {
        if (!acceptsSession(ack.sessionId)) return null
        if (ack.appliedSequence < lastAckSequence) return null
        lastAckSequence = ack.appliedSequence
        if (ack.finished) {
            reset()
            return null
        }
        sessionStarted = true
        if (inFlight?.let { ack.appliedSequence >= it.sequence } == true) {
            inFlight = null
        }
        val next = pending ?: return null
        pending = null
        inFlight = next
        return connected.thenAction(next)
    }

    fun retry(): OutboundAction? {
        if (!connected) return null
        val snapshot = inFlight ?: return null
        val retry = if (sessionStarted && snapshot.type == SnapshotType.START) {
            snapshot.copy(type = SnapshotType.UPDATE)
        } else {
            snapshot
        }
        return OutboundAction(retry, resume = false)
    }

    fun currentInFlight(): SnapshotMessage? = inFlight

    fun acceptsSession(sessionId: String): Boolean =
        latest?.sessionId == sessionId || pending?.sessionId == sessionId || inFlight?.sessionId == sessionId

    fun abandonSession() {
        reset()
    }

    private fun reset() {
        sessionStarted = false
        startAttempted = false
        inFlight = null
        pending = null
        latest = null
        lastAckSequence = 0
    }

    private fun Boolean.thenAction(snapshot: SnapshotMessage): OutboundAction? =
        if (this) OutboundAction(snapshot, resume = false) else null
}
