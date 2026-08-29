package app.flowtype.network

import app.flowtype.pairing.ComputerBinding
import app.flowtype.security.PhoneIdentity

class ControlClientPool(
    private val phoneId: String,
    private val onSwitchComputer: (String, (Boolean) -> Unit) -> Unit,
    private val identityFactory: () -> PhoneIdentity = ::PhoneIdentity,
) {
    private val clients = mutableMapOf<String, ControlClient>()

    fun update(bindings: List<ComputerBinding>, activePcId: String?) {
        val targets = bindings
            .filter { it.pairingToken == null && it.pcId != activePcId }
            .associateBy(ComputerBinding::pcId)

        clients.keys.toList()
            .filter { it !in targets }
            .forEach { pcId -> clients.remove(pcId)?.shutdown() }

        targets.values.forEach { binding ->
            clients.getOrPut(binding.pcId) {
                ControlClient(phoneId, identityFactory(), object : ControlClient.Listener {
                    override fun onSwitchComputer(
                        pcId: String,
                        completion: (Boolean) -> Unit,
                    ) = this@ControlClientPool.onSwitchComputer(pcId, completion)
                })
            }.update(binding)
        }
    }

    fun ensureConnected() {
        clients.values.forEach(ControlClient::ensureConnected)
    }

    fun shutdown() {
        clients.values.forEach(ControlClient::shutdown)
        clients.clear()
    }
}
