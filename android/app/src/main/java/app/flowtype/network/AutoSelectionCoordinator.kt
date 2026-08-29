package app.flowtype.network

import app.flowtype.pairing.ComputerBinding

class AutoSelectionCoordinator(
    private val probe: (List<ComputerBinding>, (List<TargetProbeResult>) -> Unit) -> Unit,
) {
    constructor(client: TargetProbeClient) : this(client::probe)

    private var generation = 0L

    fun choose(
        candidates: List<ComputerBinding>,
        currentPcId: String?,
        completion: (ComputerBinding?) -> Unit,
    ) {
        val requestGeneration = ++generation
        when (candidates.size) {
            0 -> completion(null)
            1 -> completion(candidates.single())
            else -> probe(candidates) { results ->
                if (requestGeneration == generation) {
                    completion(TargetSelector.choose(results, currentPcId)?.binding)
                }
            }
        }
    }

    fun cancel() {
        generation += 1
    }
}
