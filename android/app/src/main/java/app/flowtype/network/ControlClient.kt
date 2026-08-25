package app.flowtype.network

import android.os.Build
import android.os.Handler
import android.os.Looper
import android.util.Base64
import app.flowtype.pairing.ComputerBinding
import app.flowtype.protocol.PROTOCOL_VERSION
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
import java.util.concurrent.TimeUnit
import javax.net.ssl.SSLContext
import javax.net.ssl.TrustManager
import javax.net.ssl.X509TrustManager

/**
 * A small authenticated socket used only for commands from a Windows client.
 * It never carries input text or image data, so every paired computer can offer
 * a responsive floating-ball switch without creating another input session.
 */
class ControlClient(
    private val phoneId: String,
    private val phoneIdentity: PhoneIdentity,
    private val listener: Listener,
) {
    interface Listener {
        fun onSwitchComputer(pcId: String)
    }

    private val lock = Any()
    private val handler = Handler(Looper.getMainLooper())
    private var client: OkHttpClient? = null
    private var socket: WebSocket? = null
    private var binding: ComputerBinding? = null
    private var generation = 0L
    private var reconnectAttempt = 0
    private var retryScheduled = false
    private var stopped = true

    fun update(next: ComputerBinding) {
        synchronized(lock) {
            if (!stopped && binding == next && socket != null) return
            stopLocked()
            binding = next
            stopped = false
            client = buildClient(next)
            reconnectAttempt = 0
            openSocketLocked()
        }
    }

    fun shutdown() {
        synchronized(lock) {
            stopLocked()
            binding = null
        }
    }

    private fun stopLocked() {
        stopped = true
        generation += 1
        retryScheduled = false
        handler.removeCallbacksAndMessages(null)
        socket?.close(1000, "control stopped")
        socket = null
        client?.dispatcher?.executorService?.shutdown()
        client = null
    }

    private fun openSocketLocked() {
        val current = binding ?: return
        val currentClient = client ?: return
        generation += 1
        val currentGeneration = generation
        socket = currentClient.newWebSocket(
            Request.Builder().url(current.endpoint).build(),
            SocketListener(current, currentGeneration),
        )
    }

    private fun scheduleReconnectLocked() {
        if (stopped || binding == null || retryScheduled) return
        retryScheduled = true
        val delay = minOf(10_000L, 250L * (1L shl minOf(reconnectAttempt, 5)))
        reconnectAttempt += 1
        handler.postDelayed({
            synchronized(lock) {
                retryScheduled = false
                if (!stopped && socket == null) openSocketLocked()
            }
        }, delay)
    }

    private fun buildClient(binding: ComputerBinding): OkHttpClient {
        val trustManager = PinnedTrustManager(binding.tlsSpkiSha256)
        val sslContext = SSLContext.getInstance("TLS")
        sslContext.init(null, arrayOf<TrustManager>(trustManager), SecureRandom())
        val tls13 = ConnectionSpec.Builder(ConnectionSpec.MODERN_TLS)
            .tlsVersions(TlsVersion.TLS_1_3)
            .build()
        return OkHttpClient.Builder()
            .sslSocketFactory(sslContext.socketFactory, trustManager)
            .hostnameVerifier { _, _ -> true }
            .connectionSpecs(listOf(tls13))
            .connectTimeout(2, TimeUnit.SECONDS)
            .readTimeout(0, TimeUnit.MILLISECONDS)
            .writeTimeout(2, TimeUnit.SECONDS)
            .pingInterval(15, TimeUnit.SECONDS)
            .build()
    }

    private fun authenticate(webSocket: WebSocket, current: ComputerBinding, challenge: JSONObject) {
        require(challenge.getInt("protocol_version") == PROTOCOL_VERSION)
        require(challenge.getString("pc_id") == current.pcId)
        val nonce = challenge.getString("nonce")
        val payload = "flowtype-auth-v1\u0000${current.pcId}\u0000$phoneId\u0000$nonce"
            .toByteArray(Charsets.UTF_8)
        val auth = JSONObject()
            .put("protocol_version", PROTOCOL_VERSION)
            .put("type", if (current.pairingToken == null) "authenticate" else "pair")
            .put("phone_id", phoneId)
            .put("phone_name", Build.MODEL.ifBlank { "Android 手机" })
            .put("connection_mode", "control")
            .put(
                "signature",
                Base64.encodeToString(phoneIdentity.sign(current.pcId, payload), Base64.NO_WRAP),
            )
        current.pairingToken?.let {
            auth.put("pairing_token", it)
            auth.put(
                "public_key_spki",
                Base64.encodeToString(phoneIdentity.publicKey(current.pcId), Base64.NO_WRAP),
            )
        }
        check(webSocket.send(auth.toString()))
    }

    private inner class SocketListener(
        private val current: ComputerBinding,
        private val currentGeneration: Long,
    ) : WebSocketListener() {
        override fun onMessage(webSocket: WebSocket, text: String) {
            synchronized(lock) {
                if (currentGeneration != generation || stopped) return
                runCatching {
                    val value = JSONObject(text)
                    when (value.getString("type")) {
                        "challenge" -> authenticate(webSocket, current, value)
                        "ready" -> reconnectAttempt = 0
                        "switch_computer" -> {
                            if (value.optInt("protocol_version", PROTOCOL_VERSION) != PROTOCOL_VERSION) return@runCatching
                            listener.onSwitchComputer(value.getString("pc_id"))
                        }
                    }
                }
            }
        }

        override fun onClosed(webSocket: WebSocket, code: Int, reason: String) {
            connectionLost()
        }

        override fun onFailure(webSocket: WebSocket, error: Throwable, response: Response?) {
            connectionLost()
        }

        private fun connectionLost() {
            synchronized(lock) {
                if (currentGeneration != generation || stopped) return
                socket = null
                binding = current.nextEndpoint()
                scheduleReconnectLocked()
            }
        }
    }

    @Suppress("CustomX509TrustManager")
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
}
