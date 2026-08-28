package app.flowtype.protocol

import org.json.JSONObject

const val PROTOCOL_VERSION: Int = 1
const val MAX_MESSAGE_BYTES: Int = 1024 * 1024

enum class SnapshotType(val wireName: String) {
    START("start"),
    UPDATE("update"),
    FINISH("finish"),
}

data class SnapshotMessage(
    val type: SnapshotType,
    val phoneId: String,
    val sessionId: String,
    val sequence: Long,
    val fullText: String,
    val protocolVersion: Int = PROTOCOL_VERSION,
) {
    fun validate() {
        require(protocolVersion == PROTOCOL_VERSION) { "unsupported protocol" }
        require(phoneId.isNotBlank() && sessionId.isNotBlank()) { "missing identifier" }
        require(sequence > 0) { "sequence must be positive" }
        require(fullText.toByteArray(Charsets.UTF_8).size <= MAX_MESSAGE_BYTES) {
            "message too large"
        }
    }
}

enum class ClientSessionState(val wireName: String) {
    ACTIVE("active"),
    FINISHING("finishing"),
}

data class ResumeMessage(
    val phoneId: String,
    val sessionId: String,
    val lastAckSequence: Long,
    val sequence: Long,
    val fullText: String,
    val sessionState: ClientSessionState,
    val protocolVersion: Int = PROTOCOL_VERSION,
)

data class CancelMessage(
    val phoneId: String,
    val sessionId: String,
    val protocolVersion: Int = PROTOCOL_VERSION,
)

data class ProbeMessage(
    val phoneId: String,
    val protocolVersion: Int = PROTOCOL_VERSION,
)

data class HealthCheckMessage(
    val phoneId: String,
    val protocolVersion: Int = PROTOCOL_VERSION,
)

data class SwitchAckMessage(
    val requestId: String,
    val pcId: String,
    val accepted: Boolean,
    val protocolVersion: Int = PROTOCOL_VERSION,
)

enum class ServerSessionState(val wireName: String) {
    ACTIVE("active"),
    FINISHED("finished"),
}

data class AckMessage(
    val sessionId: String,
    val appliedSequence: Long,
    val sessionState: ServerSessionState,
    val protocolVersion: Int = PROTOCOL_VERSION,
) {
    val finished: Boolean get() = sessionState == ServerSessionState.FINISHED
}

enum class TargetState(val wireName: String) {
    ACTIVE("active"),
    NOT_FOREGROUND("not_foreground"),
    INVALID("invalid"),
}

data class TargetMessage(
    val sessionId: String,
    val targetState: TargetState,
    val targetName: String?,
    val protocolVersion: Int = PROTOCOL_VERSION,
)

data class SwitchComputerMessage(
    val pcId: String,
    val pcName: String,
    val requestId: String?,
    val protocolVersion: Int = PROTOCOL_VERSION,
)

enum class ProbeState(val wireName: String) {
    READY("ready"),
    UNSUPPORTED("unsupported"),
    INVALID("invalid"),
}

data class ProbeResultMessage(
    val targetState: ProbeState,
    val targetName: String?,
    val activityAgeMs: Long?,
    val protocolVersion: Int = PROTOCOL_VERSION,
)

data class HealthAckMessage(
    val protocolVersion: Int = PROTOCOL_VERSION,
)

enum class ErrorCode {
    AUTH_FAILED,
    SESSION_BUSY,
    SESSION_UNKNOWN,
    SESSION_FINISHED,
    SEQUENCE_CONFLICT,
    TEXT_TOO_LARGE,
    TARGET_UNAVAILABLE,
    TARGET_INVALID,
    TARGET_MODIFIED,
    INJECTOR_UNAVAILABLE,
    RECOVERY_REQUIRED,
    INJECTION_UNKNOWN,
    INVALID_MESSAGE,
    UNSUPPORTED_PROTOCOL,
}

data class ErrorMessage(
    val code: ErrorCode,
    val sessionId: String?,
    val protocolVersion: Int = PROTOCOL_VERSION,
)

sealed interface ServerMessage {
    data class Ack(val value: AckMessage) : ServerMessage
    data class Target(val value: TargetMessage) : ServerMessage
    data class SwitchComputer(val value: SwitchComputerMessage) : ServerMessage
    data class ProbeResult(val value: ProbeResultMessage) : ServerMessage
    data class HealthAck(val value: HealthAckMessage) : ServerMessage
    data class Error(val value: ErrorMessage) : ServerMessage
}

object ProtocolCodec {
    fun encode(message: SnapshotMessage): String = JSONObject()
        .put("protocol_version", message.protocolVersion)
        .put("type", message.type.wireName)
        .put("phone_id", message.phoneId)
        .put("session_id", message.sessionId)
        .put("sequence", message.sequence)
        .put("full_text", message.fullText)
        .toString()

    fun encode(message: ResumeMessage): String = JSONObject()
        .put("protocol_version", message.protocolVersion)
        .put("type", "resume")
        .put("phone_id", message.phoneId)
        .put("session_id", message.sessionId)
        .put("last_ack_sequence", message.lastAckSequence)
        .put("sequence", message.sequence)
        .put("full_text", message.fullText)
        .put("session_state", message.sessionState.wireName)
        .toString()

    fun encode(message: CancelMessage): String = JSONObject()
        .put("protocol_version", message.protocolVersion)
        .put("type", "cancel")
        .put("phone_id", message.phoneId)
        .put("session_id", message.sessionId)
        .toString()

    fun encode(message: ProbeMessage): String = JSONObject()
        .put("protocol_version", message.protocolVersion)
        .put("type", "probe")
        .put("phone_id", message.phoneId)
        .toString()

    fun encode(message: HealthCheckMessage): String = JSONObject()
        .put("protocol_version", message.protocolVersion)
        .put("type", "health_check")
        .put("phone_id", message.phoneId)
        .toString()

    fun encode(message: SwitchAckMessage): String = JSONObject()
        .put("protocol_version", message.protocolVersion)
        .put("type", "switch_ack")
        .put("request_id", message.requestId)
        .put("pc_id", message.pcId)
        .put("accepted", message.accepted)
        .toString()

    fun decodeSnapshot(json: String): SnapshotMessage {
        val value = JSONObject(json)
        val type = SnapshotType.entries.single { it.wireName == value.getString("type") }
        return SnapshotMessage(
            type = type,
            phoneId = value.getString("phone_id"),
            sessionId = value.getString("session_id"),
            sequence = value.getLong("sequence"),
            fullText = value.getString("full_text"),
            protocolVersion = value.getInt("protocol_version"),
        ).also(SnapshotMessage::validate)
    }

    fun decodeServer(json: String): ServerMessage {
        val value = JSONObject(json)
        val version = value.getInt("protocol_version")
        require(version == PROTOCOL_VERSION) { "unsupported protocol" }
        return when (value.getString("type")) {
            "ack" -> ServerMessage.Ack(
                AckMessage(
                    sessionId = value.getString("session_id"),
                    appliedSequence = value.getLong("applied_sequence"),
                    sessionState = enumByWireName(value.getString("session_state")),
                    protocolVersion = version,
                ),
            )
            "target" -> ServerMessage.Target(
                TargetMessage(
                    sessionId = value.getString("session_id"),
                    targetState = enumByWireName(value.getString("target_state")),
                    targetName = value.optString("target_name").ifEmpty { null },
                    protocolVersion = version,
                ),
            )
            "switch_computer" -> ServerMessage.SwitchComputer(
                SwitchComputerMessage(
                    pcId = value.getString("pc_id"),
                    pcName = value.getString("pc_name"),
                    requestId = value.optString("request_id").ifEmpty { null },
                    protocolVersion = version,
                ),
            )
            "probe_result" -> ServerMessage.ProbeResult(
                ProbeResultMessage(
                    targetState = enumByWireName(value.getString("target_state")),
                    targetName = value.optString("target_name").ifEmpty { null },
                    activityAgeMs = if (value.has("activity_age_ms")) value.getLong("activity_age_ms") else null,
                    protocolVersion = version,
                ),
            )
            "health_ack" -> ServerMessage.HealthAck(HealthAckMessage(protocolVersion = version))
            "error" -> ServerMessage.Error(
                ErrorMessage(
                    code = ErrorCode.valueOf(value.getString("code")),
                    sessionId = value.optString("session_id").ifEmpty { null },
                    protocolVersion = version,
                ),
            )
            else -> error("unsupported message type")
        }
    }

    private inline fun <reified T : Enum<T>> enumByWireName(name: String): T =
        enumValues<T>().single { (it as? ClientSessionState)?.wireName == name ||
            (it as? ServerSessionState)?.wireName == name ||
            (it as? TargetState)?.wireName == name ||
            (it as? ProbeState)?.wireName == name }
}
