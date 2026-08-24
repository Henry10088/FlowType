package app.flowtype.network

import android.os.Build
import android.os.Handler
import android.os.Looper
import android.util.Base64
import app.flowtype.pairing.ComputerBinding
import app.flowtype.protocol.ProbeMessage
import app.flowtype.protocol.ProbeState
import app.flowtype.protocol.ProtocolCodec
import app.flowtype.protocol.PROTOCOL_VERSION
import app.flowtype.protocol.ServerMessage
import app.flowtype.security.PhoneIdentity
import okhttp3.ConnectionSpec
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.Response
import okhttp3.TlsVersion
import okhttp3.WebSocket
import okhttp3.WebSocketListener
import org.json.JSONObject
import java.security.MessageDigest
import java.security.SecureRandom
import java.security.cert.CertificateException
import java.security.cert.X509Certificate
import java.util.Collections
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicInteger
import javax.net.ssl.SSLContext
import javax.net.ssl.TrustManager
import javax.net.ssl.X509TrustManager

enum class ProbeOutcome { READY, UNSUPPORTED, INVALID, UNAVAILABLE }

data class TargetProbeResult(
    val binding: ComputerBinding,
    val outcome: ProbeOutcome,
    val targetName: String? = null,
    val activityAgeMs: Long? = null,
)

/** Opens short-lived authenticated control sockets; no input text is sent during probing. */
class TargetProbeClient(
    private val phoneId: String,
    private val phoneIdentity: PhoneIdentity,
) {
    private val handler = Handler(Looper.getMainLooper())

    fun probe(bindings: List<ComputerBinding>, callback: (List<TargetProbeResult>) -> Unit) {
        if (bindings.isEmpty()) {
            handler.post { callback(emptyList()) }
            return
        }
        val results = Collections.synchronizedList(mutableListOf<TargetProbeResult>())
        val remaining = AtomicInteger(bindings.size)
        bindings.forEach { binding ->
            probeOne(binding) { result ->
                results += result
                if (remaining.decrementAndGet() == 0) {
                    handler.post { callback(results.toList()) }
                }
            }
        }
    }

    private fun probeOne(binding: ComputerBinding, callback: (TargetProbeResult) -> Unit) {
        val client = runCatching { buildClient(binding) }.getOrElse {
            callback(TargetProbeResult(binding, ProbeOutcome.UNAVAILABLE))
            return
        }
        val completed = AtomicBoolean(false)
        lateinit var timeoutTask: Runnable
        fun complete(result: TargetProbeResult, socket: WebSocket? = null) {
            if (!completed.compareAndSet(false, true)) return
            handler.removeCallbacks(timeoutTask)
            callback(result)
            socket?.close(1000, "probe complete")
            client.dispatcher.executorService.shutdown()
        }
        val listener = object : WebSocketListener() {
            override fun onMessage(webSocket: WebSocket, text: String) {
                runCatching {
                    val type = JSONObject(text).getString("type")
                    when (type) {
                        "challenge" -> authenticate(webSocket, binding, JSONObject(text))
                        "ready" -> {
                            check(webSocket.send(ProtocolCodec.encode(ProbeMessage(phoneId))))
                        }
                        "probe_result" -> {
                            when (val message = ProtocolCodec.decodeServer(text)) {
                                is ServerMessage.ProbeResult -> {
                                    val value = message.value
                                    complete(
                                        TargetProbeResult(
                                            binding = binding,
                                            outcome = when (value.targetState) {
                                                ProbeState.READY -> ProbeOutcome.READY
                                                ProbeState.UNSUPPORTED -> ProbeOutcome.UNSUPPORTED
                                                ProbeState.INVALID -> ProbeOutcome.INVALID
                                            },
                                            targetName = value.targetName,
                                            activityAgeMs = value.activityAgeMs,
                                        ),
                                        webSocket,
                                    )
                                }
                                else -> complete(TargetProbeResult(binding, ProbeOutcome.UNAVAILABLE), webSocket)
                            }
                        }
                        "error" -> complete(TargetProbeResult(binding, ProbeOutcome.UNAVAILABLE), webSocket)
                    }
                }.onFailure {
                    complete(TargetProbeResult(binding, ProbeOutcome.UNAVAILABLE), webSocket)
                }
            }

            override fun onFailure(webSocket: WebSocket, t: Throwable, response: Response?) {
                complete(TargetProbeResult(binding, ProbeOutcome.UNAVAILABLE), webSocket)
            }

            override fun onClosed(webSocket: WebSocket, code: Int, reason: String) {
                complete(TargetProbeResult(binding, ProbeOutcome.UNAVAILABLE))
            }
        }
        timeoutTask = Runnable {
            complete(TargetProbeResult(binding, ProbeOutcome.UNAVAILABLE))
        }
        handler.postDelayed(timeoutTask, PROBE_TIMEOUT_MS)
        client.newWebSocket(Request.Builder().url(binding.endpoint).build(), listener)
    }

    private fun buildClient(binding: ComputerBinding): OkHttpClient {
        val trustManager = PinnedTrustManager(binding.tlsSpkiSha256)
        val sslContext = SSLContext.getInstance("TLS").apply {
            init(null, arrayOf<TrustManager>(trustManager), SecureRandom())
        }
        val tls13 = ConnectionSpec.Builder(ConnectionSpec.MODERN_TLS)
            .tlsVersions(TlsVersion.TLS_1_3)
            .build()
        return OkHttpClient.Builder()
            .sslSocketFactory(sslContext.socketFactory, trustManager)
            .hostnameVerifier { _, _ -> true }
            .connectionSpecs(listOf(tls13))
            .connectTimeout(1500, TimeUnit.MILLISECONDS)
            .readTimeout(2000, TimeUnit.MILLISECONDS)
            .writeTimeout(2000, TimeUnit.MILLISECONDS)
            .build()
    }

    private fun authenticate(webSocket: WebSocket, binding: ComputerBinding, challenge: JSONObject) {
        require(challenge.getInt("protocol_version") == PROTOCOL_VERSION)
        require(challenge.getString("pc_id") == binding.pcId)
        val nonce = challenge.getString("nonce")
        val payload = "flowtype-auth-v1\u0000${binding.pcId}\u0000$phoneId\u0000$nonce"
            .toByteArray(Charsets.UTF_8)
        val auth = JSONObject()
            .put("protocol_version", PROTOCOL_VERSION)
            .put("type", if (binding.pairingToken == null) "authenticate" else "pair")
            .put("phone_id", phoneId)
            .put("phone_name", Build.MODEL.ifBlank { "Android 手机" })
            .put(
                "signature",
                Base64.encodeToString(phoneIdentity.sign(binding.pcId, payload), Base64.NO_WRAP),
            )
        binding.pairingToken?.let {
            auth.put("pairing_token", it)
            auth.put(
                "public_key_spki",
                Base64.encodeToString(phoneIdentity.publicKey(binding.pcId), Base64.NO_WRAP),
            )
        }
        check(webSocket.send(auth.toString()))
    }

    private class PinnedTrustManager(expectedHash: String) : X509TrustManager {
        private val expected = Base64.decode(expectedHash, Base64.DEFAULT)

        override fun checkServerTrusted(chain: Array<out X509Certificate>?, authType: String?) {
            val certificate = chain?.firstOrNull() ?: throw CertificateException("missing certificate")
            val actual = MessageDigest.getInstance("SHA-256").digest(certificate.publicKey.encoded)
            if (!MessageDigest.isEqual(expected, actual)) {
                throw CertificateException("computer identity does not match")
            }
        }

        override fun checkClientTrusted(chain: Array<out X509Certificate>?, authType: String?) =
            throw CertificateException("client certificates are not used")

        override fun getAcceptedIssuers(): Array<X509Certificate> = emptyArray()
    }

    companion object {
        private const val PROBE_TIMEOUT_MS = 2_500L
    }
}
