package app.flowtype.protocol

import org.junit.Assert.assertThrows
import org.junit.Assert.assertEquals
import org.junit.Test
import org.json.JSONArray

class ProtocolTest {
    @Test
    fun parsesLanguageNeutralContractFixtures() {
        val input = checkNotNull(javaClass.classLoader?.getResourceAsStream("valid-messages.json"))
        val fixtures = JSONArray(input.bufferedReader().use { it.readText() })

        for (index in 0..2) {
            val expected = fixtures.getJSONObject(index)
            val message = ProtocolCodec.decodeSnapshot(expected.toString())
            assertEquals(expected.getString("type"), message.type.wireName)
            assertEquals(expected.getString("full_text"), message.fullText)
        }

        val ack = ProtocolCodec.decodeServer(fixtures.getJSONObject(3).toString())
        require(ack is ServerMessage.Ack)
        assertEquals(ServerSessionState.FINISHED, ack.value.sessionState)
    }

    @Test
    fun validatesACompleteSnapshot() {
        SnapshotMessage(
            type = SnapshotType.START,
            phoneId = "phone",
            sessionId = "session",
            sequence = 1L,
            fullText = "你好\nWindows",
        ).validate()
    }

    @Test
    fun rejectsNonPositiveSequence() {
        assertThrows(IllegalArgumentException::class.java) {
            SnapshotMessage(
                type = SnapshotType.UPDATE,
                phoneId = "phone",
                sessionId = "session",
            sequence = 0L,
                fullText = "",
            ).validate()
        }
    }

    @Test
    fun encodesSessionCancellation() {
        val json = ProtocolCodec.encode(CancelMessage("phone", "session"))
        val value = org.json.JSONObject(json)

        assertEquals("cancel", value.getString("type"))
        assertEquals("phone", value.getString("phone_id"))
        assertEquals("session", value.getString("session_id"))
    }

    @Test
    fun encodesAndDecodesTargetProbeResult() {
        val json = ProtocolCodec.encode(ProbeMessage("phone"))
        assertEquals("probe", org.json.JSONObject(json).getString("type"))

        val message = ProtocolCodec.decodeServer(
            """{"protocol_version":1,"type":"probe_result","target_state":"ready","target_name":"VS Code","activity_age_ms":42}""",
        )
        require(message is ServerMessage.ProbeResult)
        assertEquals(ProbeState.READY, message.value.targetState)
        assertEquals(42L, message.value.activityAgeMs)
    }

    @Test
    fun decodesSwitchToCurrentComputer() {
        val message = ProtocolCodec.decodeServer(
            """{"protocol_version":1,"type":"switch_computer","pc_id":"pc","pc_name":"办公室电脑","request_id":"request-1"}""",
        )
        require(message is ServerMessage.SwitchComputer)
        assertEquals("pc", message.value.pcId)
        assertEquals("办公室电脑", message.value.pcName)
        assertEquals("request-1", message.value.requestId)
    }

    @Test
    fun encodesHealthCheckAndSwitchAcknowledgement() {
        val health = ProtocolCodec.encode(HealthCheckMessage("phone"))
        assertEquals("health_check", org.json.JSONObject(health).getString("type"))
        require(
            ProtocolCodec.decodeServer("""{"protocol_version":1,"type":"health_ack"}""")
                is ServerMessage.HealthAck,
        )

        val ack = org.json.JSONObject(
            ProtocolCodec.encode(SwitchAckMessage("request-1", "pc", accepted = true)),
        )
        assertEquals("switch_ack", ack.getString("type"))
        assertEquals("request-1", ack.getString("request_id"))
        assertEquals(true, ack.getBoolean("accepted"))
    }

    @Test
    fun decodesInputServiceRecoveryRequired() {
        val message = ProtocolCodec.decodeServer(
            """{"protocol_version":1,"type":"error","code":"RECOVERY_REQUIRED","session_id":"voice"}""",
        )

        require(message is ServerMessage.Error)
        assertEquals(ErrorCode.RECOVERY_REQUIRED, message.value.code)
        assertEquals("voice", message.value.sessionId)
    }

    @Test
    fun decodesTargetSubmitted() {
        val message = ProtocolCodec.decodeServer(
            """{"protocol_version":1,"type":"error","code":"TARGET_SUBMITTED","session_id":"voice"}""",
        )

        require(message is ServerMessage.Error)
        assertEquals(ErrorCode.TARGET_SUBMITTED, message.value.code)
        assertEquals("voice", message.value.sessionId)
    }
}
