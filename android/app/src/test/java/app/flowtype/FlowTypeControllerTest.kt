package app.flowtype

import org.junit.Assert.assertFalse
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
}
