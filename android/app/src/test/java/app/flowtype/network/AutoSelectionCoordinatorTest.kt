package app.flowtype.network

import app.flowtype.pairing.ComputerBinding
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Test

class AutoSelectionCoordinatorTest {
    @Test
    fun singleCandidateDoesNotOpenProbeSockets() {
        var probed = false
        val candidate = binding("one")
        val coordinator = AutoSelectionCoordinator { _, _ -> probed = true }
        var selected: ComputerBinding? = null

        coordinator.choose(listOf(candidate), null) { selected = it }

        assertEquals(candidate, selected)
        assertFalse(probed)
    }

    @Test
    fun ignoresAProbeResultAfterCancellation() {
        var callback: ((List<TargetProbeResult>) -> Unit)? = null
        val candidates = listOf(binding("one"), binding("two"))
        val coordinator = AutoSelectionCoordinator { _, result -> callback = result }
        var completed = false

        coordinator.choose(candidates, null) { completed = true }
        coordinator.cancel()
        callback?.invoke(
            listOf(TargetProbeResult(candidates.first(), ProbeOutcome.READY, activityAgeMs = 1)),
        )

        assertFalse(completed)
    }

    private fun binding(id: String) = ComputerBinding(
        pcId = id,
        pcName = id,
        endpoint = "wss://127.0.0.1:32187",
        tlsSpkiSha256 = "hash",
        pairingToken = null,
        endpoints = emptyList(),
    )
}
