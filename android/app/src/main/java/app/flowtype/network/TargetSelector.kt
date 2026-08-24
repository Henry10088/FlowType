package app.flowtype.network

import kotlin.math.abs

/** Chooses a target only when the foreground/activity signal is strong enough. */
object TargetSelector {
    private const val HYSTERESIS_MS = 2_000L
    private const val FRESH_ACTIVITY_MS = 3_000L

    fun choose(
        results: List<TargetProbeResult>,
        currentPcId: String?,
    ): TargetProbeResult? {
        val ready = results
            .filter { it.outcome == ProbeOutcome.READY }
            .sortedBy(::age)
        val best = ready.firstOrNull() ?: return null
        val current = currentPcId?.let { id -> ready.firstOrNull { it.binding.pcId == id } }
        if (current != null && closeEnough(current, best)) return current

        val second = ready.getOrNull(1)
        return when {
            second == null -> best
            age(best) <= FRESH_ACTIVITY_MS -> best
            separated(best, second) -> best
            else -> null
        }
    }

    private fun age(result: TargetProbeResult): Long =
        result.activityAgeMs ?: Long.MAX_VALUE

    private fun closeEnough(left: TargetProbeResult, right: TargetProbeResult): Boolean {
        val leftAge = age(left)
        val rightAge = age(right)
        return leftAge == Long.MAX_VALUE && rightAge == Long.MAX_VALUE ||
            abs(leftAge - rightAge) <= HYSTERESIS_MS
    }

    private fun separated(best: TargetProbeResult, second: TargetProbeResult): Boolean {
        val bestAge = age(best)
        val secondAge = age(second)
        return bestAge != Long.MAX_VALUE &&
            (secondAge == Long.MAX_VALUE || secondAge - bestAge >= HYSTERESIS_MS)
    }
}
