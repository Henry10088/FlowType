package app.flowtype.connectivity

internal class ConnectivityDemand {
    private var visibleActivities = 0
    private var floatingInputVisible = false

    val required: Boolean
        get() = visibleActivities > 0 || floatingInputVisible

    fun activityStarted() {
        visibleActivities += 1
    }

    fun activityStopped() {
        visibleActivities = (visibleActivities - 1).coerceAtLeast(0)
    }

    fun setFloatingInputVisible(visible: Boolean) {
        floatingInputVisible = visible
    }
}
