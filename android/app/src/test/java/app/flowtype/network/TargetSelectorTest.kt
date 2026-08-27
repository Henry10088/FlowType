package app.flowtype.network

import app.flowtype.pairing.ComputerBinding
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class TargetSelectorTest {
    @Test
    fun onlyRotatesCandidateEndpointsDuringPairing() {
        val first = ComputerBinding(
            pcId = "pc",
            pcName = "Office",
            endpoint = "wss://100.0.0.13:32187/v1/sync",
            tlsSpkiSha256 = "hash",
            pairingToken = "one-time-token",
            endpoints = listOf(
                "wss://100.0.0.13:32187/v1/sync",
                "wss://192.168.1.20:32187/v1/sync",
            ),
        )

        val second = first.nextPairingEndpoint()
        assertEquals("pc", second.pcId)
        assertEquals("wss://192.168.1.20:32187/v1/sync", second.endpoint)
        assertEquals(first.endpoint, second.nextPairingEndpoint().endpoint)

        val paired = second.pairedAtCurrentEndpoint()
        assertEquals(second.endpoint, paired.nextPairingEndpoint().endpoint)
        assertEquals(listOf(second.endpoint), paired.endpoints)
    }

    @Test
    fun keepsTheCurrentComputerWhenActivityIsClose() {
        val current = result("current", 8_000)
        val other = result("other", 9_500)

        assertEquals("current", TargetSelector.choose(listOf(other, current), "current")?.binding?.pcId)
    }

    @Test
    fun choosesAClearlyMoreRecentlyUsedComputer() {
        val current = result("current", 20_000)
        val other = result("other", 2_000)

        assertEquals("other", TargetSelector.choose(listOf(current, other), "current")?.binding?.pcId)
    }

    @Test
    fun refusesAnAmbiguousPair() {
        val first = result("first", 8_000)
        val second = result("second", 8_500)

        assertNull(TargetSelector.choose(listOf(first, second), null))
    }

    @Test
    fun oneReadyComputerIsEnough() {
        assertEquals("only", TargetSelector.choose(listOf(result("only", null)), null)?.binding?.pcId)
    }

    private fun result(pcId: String, age: Long?) = TargetProbeResult(
        binding = ComputerBinding(pcId, pcId, "wss://$pcId/v1/sync", "hash", null),
        outcome = ProbeOutcome.READY,
        activityAgeMs = age,
    )
}
