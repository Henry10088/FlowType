package app.flowtype.connectivity

internal class ConnectivityDemand {
    private var visibleActivities = 0
    private var floatingInputVisible = false
    private var fileTransferActive = false

    val required: Boolean
        get() = visibleActivities > 0 || floatingInputVisible || fileTransferActive

    fun activityStarted() {
        visibleActivities += 1
    }

    fun activityStopped() {
        visibleActivities = (visibleActivities - 1).coerceAtLeast(0)
    }

    fun setFloatingInputVisible(visible: Boolean) {
        floatingInputVisible = visible
    }

    fun setFileTransferActive(active: Boolean) {
        fileTransferActive = active
    }
}
