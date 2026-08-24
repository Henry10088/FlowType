package app.flowtype.network

import android.content.Context
import android.net.Uri
import android.net.nsd.NsdManager
import android.net.nsd.NsdServiceInfo
import app.flowtype.pairing.BindingStore
import java.util.concurrent.ConcurrentHashMap

@Suppress("DEPRECATION")
class ComputerDiscovery(
    context: Context,
    private val bindings: BindingStore,
    private val listener: (pcId: String, endpoint: String?) -> Unit,
) {
    private val manager = context.getSystemService(NsdManager::class.java)
    private val services = ConcurrentHashMap<String, String>()
    private var started = false

    private val discoveryListener = object : NsdManager.DiscoveryListener {
        override fun onDiscoveryStarted(serviceType: String) = Unit
        override fun onDiscoveryStopped(serviceType: String) = Unit
        override fun onStartDiscoveryFailed(serviceType: String, errorCode: Int) { started = false }
        override fun onStopDiscoveryFailed(serviceType: String, errorCode: Int) { started = false }

        override fun onServiceFound(service: NsdServiceInfo) {
            if (service.serviceType != SERVICE_TYPE) return
            manager.resolveService(service, object : NsdManager.ResolveListener {
                override fun onResolveFailed(serviceInfo: NsdServiceInfo, errorCode: Int) = Unit

                override fun onServiceResolved(serviceInfo: NsdServiceInfo) {
                    val pcId = serviceInfo.attributes["pc_id"]?.toString(Charsets.UTF_8)
                        ?: serviceInfo.serviceName
                    val binding = bindings.list().firstOrNull { it.pcId == pcId } ?: return
                    val host = serviceInfo.host.hostAddress ?: return
                    val bracketedHost = if (host.contains(':')) "[$host]" else host
                    val path = Uri.parse(binding.endpoint).encodedPath?.ifEmpty { null } ?: "/v1/sync"
                    val endpoint = "wss://$bracketedHost:${serviceInfo.port}$path"
                    services[serviceInfo.serviceName] = pcId
                    listener(pcId, endpoint)
                }
            })
        }

        override fun onServiceLost(service: NsdServiceInfo) {
            services.remove(service.serviceName)?.let { listener(it, null) }
        }
    }

    fun start() {
        if (started) return
        started = true
        runCatching { manager.discoverServices(SERVICE_TYPE, NsdManager.PROTOCOL_DNS_SD, discoveryListener) }
            .onFailure { started = false }
    }

    companion object {
        const val SERVICE_TYPE = "_flowtype._tcp."
    }
}
