package app.flowtype

import app.flowtype.pairing.ComputerBinding
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class FlowTypeControllerTest {
    @Test
    fun automaticSelectionRunsOnlyForAnIdleImplicitSession() {
        assertTrue(shouldBeginAutoSelection(true, hasActiveSession = false, explicitStartRequired = false))
        assertFalse(shouldBeginAutoSelection(false, hasActiveSession = false, explicitStartRequired = false))
        assertFalse(shouldBeginAutoSelection(true, hasActiveSession = true, explicitStartRequired = false))
        assertFalse(shouldBeginAutoSelection(true, hasActiveSession = false, explicitStartRequired = true))
    }

    @Test
    fun restoredLocalDraftRequiresAnExplicitStart() {
        assertTrue(restoredDraftRequiresExplicitStart(hasActiveSession = false, hasText = true))
        assertFalse(restoredDraftRequiresExplicitStart(hasActiveSession = true, hasText = true))
        assertFalse(restoredDraftRequiresExplicitStart(hasActiveSession = false, hasText = false))
    }

    @Test
    fun recoverableTransferSelectsItsComputerBeforeThePreviouslySelectedOne() {
        val selected = binding("pc-selected")
        val recovery = binding("pc-recovery")

        assertEquals(recovery, recoveryBinding(listOf(selected, recovery), recovery.pcId))
        assertEquals(selected, recoveryBinding(listOf(selected, recovery), "missing"))
        assertEquals(selected, recoveryBinding(listOf(selected, recovery), null))
        assertNull(recoveryBinding(emptyList(), recovery.pcId))
    }

    private fun binding(pcId: String) = ComputerBinding(
        pcId = pcId,
        pcName = pcId,
        endpoint = "127.0.0.1:32187",
        tlsSpkiSha256 = "sha256",
        pairingToken = null,
    )
}
