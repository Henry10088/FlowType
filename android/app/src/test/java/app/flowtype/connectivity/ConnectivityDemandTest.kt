package app.flowtype.connectivity

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class ConnectivityDemandTest {
    @Test
    fun backgroundWithoutFloatingDoesNotRequireConnectivity() {
        val demand = ConnectivityDemand()

        demand.activityStarted()
        assertTrue(demand.required)

        demand.activityStopped()
        assertFalse(demand.required)
    }

    @Test
    fun floatingBallDoesNotKeepConnectivityAfterActivityStops() {
        val demand = ConnectivityDemand()

        demand.activityStarted()
        demand.activityStopped()

        assertFalse(demand.required)
    }

    @Test
    fun visibleFloatingInputKeepsConnectivityUntilItCloses() {
        val demand = ConnectivityDemand()

        demand.setFloatingInputVisible(true)
        assertTrue(demand.required)

        demand.setFloatingInputVisible(false)
        assertFalse(demand.required)
    }

    @Test
    fun overlappingActivitiesReleaseTheirClaimsIndependently() {
        val demand = ConnectivityDemand()

        demand.activityStarted()
        demand.activityStarted()
        demand.activityStopped()
        assertTrue(demand.required)

        demand.activityStopped()
        assertFalse(demand.required)
    }
}
