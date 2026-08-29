package app.flowtype.network

import android.annotation.SuppressLint

import android.os.Build
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.util.Base64
import app.flowtype.pairing.ComputerBinding
import app.flowtype.image.ImageTransferReply
import app.flowtype.image.PreparedImage
import app.flowtype.protocol.AckMessage
import app.flowtype.protocol.CancelMessage
import app.flowtype.protocol.ClientSessionState
import app.flowtype.protocol.ErrorCode
import app.flowtype.protocol.ErrorMessage
import app.flowtype.protocol.HealthCheckMessage
import app.flowtype.protocol.ProtocolCodec
import app.flowtype.protocol.PROTOCOL_VERSION
import app.flowtype.protocol.ResumeMessage
import app.flowtype.protocol.ServerMessage
import app.flowtype.protocol.SnapshotMessage
import app.flowtype.protocol.SwitchAckMessage
import app.flowtype.protocol.TargetMessage
import app.flowtype.protocol.TargetState
import app.flowtype.security.PhoneIdentity
import okhttp3.ConnectionSpec
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.Response
import okhttp3.TlsVersion
import okhttp3.WebSocket
import okhttp3.WebSocketListener
import okio.ByteString.Companion.toByteString
import org.json.JSONObject
import org.json.JSONArray
import java.security.MessageDigest
import java.security.SecureRandom
import java.security.cert.CertificateException
import java.security.cert.X509Certificate
import java.util.concurrent.TimeUnit
import javax.net.ssl.SSLContext
import javax.net.ssl.TrustManager
import javax.net.ssl.X509TrustManager

class SyncClient(
    private val phoneId: String,
    private val phoneIdentity: PhoneIdentity,
    private val listener: Listener,
) {
    interface Listener {
        fun onReady(binding: ComputerBinding)
        fun onAck(ack: AckMessage)
        fun onTarget(target: TargetMessage)
        fun onSwitchComputer(pcId: String, completion: (Boolean) -> Unit)
        fun onDisconnected(binding: ComputerBinding)
        fun onFailure(binding: ComputerBinding)
        fun onServerError(binding: ComputerBinding, error: ErrorMessage)
        fun onPairingInvalid(binding: ComputerBinding)
        fun onImageTransferred(transferId: String)
        fun onImageTransferFailed(transferId: String)
    }

    private val lock = Any()
    private val handler = Handler(Looper.getMainLooper())
    private val queue = OutboundQueue()
    private var client: OkHttpClient? = null
    private var socket: WebSocket? = null
    private var binding: ComputerBinding? = null
    private var ready = false
    private var stopped = false
    private var generation = 0L
    private var reconnectAttempt = 0
    private var reconnectRunnable: Runnable? = null
    private var connectTimeoutRunnable: Runnable? = null
    private var healthTimeoutRunnable: Runnable? = null
    private var targetRetryRunnable: Runnable? = null
    private var socketOpenedAtMs = 0L
    private var lastServerResponseAtMs = 0L
    private var healthCheckPending = false
    private var healthCheckSupported = false
    private var pendingImageId: String? = null

    fun connect(binding: ComputerBinding) {
        synchronized(lock) {
            stopped = false
            invalidateSocketLocked()
            client?.dispatcher?.executorService?.shutdown()
            this.binding = binding
            client = buildClient(binding)
            reconnectAttempt = 0
            openSocketLocked()
        }
    }

    fun ensureConnected() {
        synchronized(lock) {
            if (stopped || ready || binding == null) return
            if (socket != null && connectionAgeMsLocked() < CONNECT_TIMEOUT_MS) return
            restartSocketLocked()
        }
    }

    fun forceReconnect() {
        synchronized(lock) {
            if (stopped || binding == null) return
            reconnectAttempt = 0
            restartSocketLocked()
        }
    }

    fun send(message: SnapshotMessage) {
        synchronized(lock) {
            val action = queue.offer(message)
            if (ready) {
                if (sendHealthCheckIfIdleLocked()) action?.let(::sendActionLocked)
            } else {
                wakeConnectionForTextLocked()
            }
        }
    }

    fun sendImage(image: PreparedImage): Boolean = synchronized(lock) {
        val currentSocket = socket
        if (!ready || currentSocket == null || pendingImageId != null) return@synchronized false
        pendingImageId = image.transferId
        val sent = currentSocket.send(image.header(phoneId)) &&
            currentSocket.send(image.bytes.toByteString())
        if (!sent) {
            pendingImageId = null
            restartSocketLocked()
        }
        sent
    }

    fun abandonSession(sessionId: String) {
        synchronized(lock) {
            queue.abandonSession()
            if (ready && socket?.send(ProtocolCodec.encode(CancelMessage(phoneId, sessionId))) != true) {
                restartSocketLocked()
            }
        }
    }

    fun restore(message: SnapshotMessage, acknowledgedSequence: Long, remoteStarted: Boolean) {
        synchronized(lock) { queue.restore(message, acknowledgedSequence, remoteStarted) }
    }

    fun startOfflineDraft() {
        synchronized(lock) { queue.startOfflineDraft()?.let(::sendActionLocked) }
    }

    fun requiresExplicitStart(): Boolean = synchronized(lock) { queue.requiresExplicitStart() }

    fun remoteStarted(): Boolean = synchronized(lock) { queue.remoteStarted() }

    fun shutdown() {
        synchronized(lock) {
            stopped = true
            invalidateSocketLocked()
            socket?.close(1000, null)
            socket = null
            client?.dispatcher?.executorService?.shutdown()
            client = null
            queue.onDisconnected()
        }
    }

    fun resetForTargetSelection() {
        synchronized(lock) {
            stopped = true
            invalidateSocketLocked()
            socket?.close(1000, "selecting target")
            socket = null
            client?.dispatcher?.executorService?.shutdown()
            client = null
            binding = null
            queue.abandonSession()
        }
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

    private fun openSocketLocked() {
        val current = binding ?: return
        val currentClient = client ?: return
        cancelReconnectLocked()
        generation += 1
        val currentGeneration = generation
        ready = false
        healthCheckPending = false
        healthCheckSupported = false
        queue.onDisconnected()
        socketOpenedAtMs = SystemClock.elapsedRealtime()
        socket = currentClient.newWebSocket(
            Request.Builder().url(current.endpoint).build(),
            SocketListener(current, currentGeneration),
        )
        val timeout = Runnable {
            synchronized(lock) {
                if (currentGeneration != generation || stopped || ready) return@synchronized
                connectionLostLocked(current, currentGeneration, failed = true, cancelSocket = true)
            }
        }
        connectTimeoutRunnable = timeout
        handler.postDelayed(timeout, CONNECT_TIMEOUT_MS)
    }

    private fun authenticate(webSocket: WebSocket, current: ComputerBinding, challenge: JSONObject): Boolean {
        require(challenge.getInt("protocol_version") == PROTOCOL_VERSION)
        require(challenge.getString("pc_id") == current.pcId)
        val nonce = challenge.getString("nonce")
        val payload = "flowtype-auth-v1\u0000${current.pcId}\u0000$phoneId\u0000$nonce"
            .toByteArray(Charsets.UTF_8)
        val auth = JSONObject()
            .put("protocol_version", PROTOCOL_VERSION)
            .put("type", if (current.pairingToken == null) "authenticate" else "pair")
            .put("phone_id", phoneId)
            .put("phone_name", Build.MODEL.ifBlank { "Android" })
            .put("capabilities", JSONArray(listOf("health_check", "switch_ack")))
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
        return webSocket.send(auth.toString())
    }

    private fun sendActionLocked(action: OutboundAction) {
        val payload = if (action.resume) {
            ProtocolCodec.encode(
                ResumeMessage(
                    phoneId = action.snapshot.phoneId,
                    sessionId = action.snapshot.sessionId,
                    lastAckSequence = queue.lastAckSequence,
                    sequence = action.snapshot.sequence,
                    fullText = action.snapshot.fullText,
                    sessionState = if (action.snapshot.type.wireName == "finish") {
                        ClientSessionState.FINISHING
                    } else {
                        ClientSessionState.ACTIVE
                    },
                ),
            )
        } else {
            ProtocolCodec.encode(action.snapshot)
        }
        if (socket?.send(payload) != true) {
            restartSocketLocked()
        }
    }

    private fun scheduleTargetRetryLocked() {
        if (targetRetryRunnable != null || !ready) return
        val retry = Runnable {
            synchronized(lock) {
                targetRetryRunnable = null
                if (ready) queue.retry()?.let(::sendActionLocked)
            }
        }
        targetRetryRunnable = retry
        handler.postDelayed(retry, 500)
    }

    private fun scheduleReconnectLocked() {
        if (stopped || binding == null || reconnectRunnable != null) return
        socket = null
        val delay = RECONNECT_DELAYS[minOf(reconnectAttempt, RECONNECT_DELAYS.lastIndex)]
        reconnectAttempt += 1
        val reconnect = Runnable {
            synchronized(lock) {
                reconnectRunnable = null
                if (!stopped && !ready && socket == null) openSocketLocked()
            }
        }
        reconnectRunnable = reconnect
        handler.postDelayed(reconnect, delay)
    }

    private fun sendHealthCheckIfIdleLocked(): Boolean {
        if (!healthCheckSupported || healthCheckPending ||
            SystemClock.elapsedRealtime() - lastServerResponseAtMs < HEALTH_IDLE_MS
        ) {
            return true
        }
        val currentGeneration = generation
        if (socket?.send(ProtocolCodec.encode(HealthCheckMessage(phoneId))) != true) {
            restartSocketLocked()
            return false
        }
        healthCheckPending = true
        val timeout = Runnable {
            synchronized(lock) {
                if (currentGeneration != generation || !healthCheckPending || stopped) return@synchronized
                restartSocketLocked()
            }
        }
        healthTimeoutRunnable = timeout
        handler.postDelayed(timeout, HEALTH_TIMEOUT_MS)
        return true
    }

    private fun markServerResponsiveLocked() {
        lastServerResponseAtMs = SystemClock.elapsedRealtime()
        healthCheckPending = false
        healthTimeoutRunnable?.let(handler::removeCallbacks)
        healthTimeoutRunnable = null
    }

    private fun wakeConnectionForTextLocked() {
        if (stopped || binding == null) return
        if (socket != null && connectionAgeMsLocked() < CONNECT_TIMEOUT_MS) return
        restartSocketLocked()
    }

    private fun connectionAgeMsLocked(): Long =
        (SystemClock.elapsedRealtime() - socketOpenedAtMs).coerceAtLeast(0L)

    private fun restartSocketLocked() {
        if (stopped || binding == null) return
        invalidateSocketLocked()
        openSocketLocked()
    }

    private fun invalidateSocketLocked() {
        generation += 1
        ready = false
        healthCheckPending = false
        connectTimeoutRunnable?.let(handler::removeCallbacks)
        connectTimeoutRunnable = null
        healthTimeoutRunnable?.let(handler::removeCallbacks)
        healthTimeoutRunnable = null
        targetRetryRunnable?.let(handler::removeCallbacks)
        targetRetryRunnable = null
        cancelReconnectLocked()
        socket?.cancel()
        socket = null
        queue.onDisconnected()
    }

    private fun cancelReconnectLocked() {
        reconnectRunnable?.let(handler::removeCallbacks)
        reconnectRunnable = null
    }

    private inner class SocketListener(
        private val current: ComputerBinding,
        private val currentGeneration: Long,
    ) : WebSocketListener() {
        override fun onMessage(webSocket: WebSocket, text: String) {
            synchronized(lock) {
                if (currentGeneration != generation || stopped) return
                markServerResponsiveLocked()
                val value = JSONObject(text)
                val type = value.getString("type")
                when (type) {
                    "challenge" -> if (!authenticate(webSocket, current, value)) restartSocketLocked()
                    "ready" -> {
                        connectTimeoutRunnable?.let(handler::removeCallbacks)
                        connectTimeoutRunnable = null
                        ready = true
                        reconnectAttempt = 0
                        healthCheckSupported = value.optJSONArray("capabilities")?.let { capabilities ->
                            (0 until capabilities.length()).any {
                                capabilities.optString(it) == "health_check"
                            }
                        } == true
                        val authenticated = current.pairedAtCurrentEndpoint()
                        binding = authenticated
                        val action = queue.onConnected()
                        listener.onReady(authenticated)
                        action?.let(::sendActionLocked)
                    }
                    "image_ack", "image_error" -> {
                        when (val reply = ImageTransferReply.decode(JSONObject(text))) {
                            is ImageTransferReply.Ack -> {
                                if (reply.transferId != pendingImageId) return
                                pendingImageId = null
                                listener.onImageTransferred(reply.transferId)
                            }
                            is ImageTransferReply.Error -> {
                                if (reply.transferId != pendingImageId) return
                                pendingImageId = null
                                listener.onImageTransferFailed(reply.transferId)
                            }
                        }
                    }
                    else -> when (val message = ProtocolCodec.decodeServer(text)) {
                        is ServerMessage.ProbeResult -> {
                            // Probe sockets are owned by TargetProbeClient. Ignore an
                            // unexpected probe response on the long-lived sync socket.
                        }
                        is ServerMessage.HealthAck -> Unit
                        is ServerMessage.Ack -> {
                            if (!queue.acceptsSession(message.value.sessionId)) return
                            queue.acknowledge(message.value)?.let(::sendActionLocked)
                            listener.onAck(message.value)
                        }
                        is ServerMessage.Target -> {
                            if (!queue.acceptsSession(message.value.sessionId)) return
                            if (message.value.targetState == TargetState.ACTIVE) {
                                queue.markSessionStarted()
                            } else if (message.value.targetState == TargetState.NOT_FOREGROUND) {
                                scheduleTargetRetryLocked()
                            } else if (message.value.targetState == TargetState.INVALID) {
                                queue.requireExplicitStart()
                            }
                            listener.onTarget(message.value)
                        }
                        is ServerMessage.SwitchComputer -> {
                            listener.onSwitchComputer(message.value.pcId) { accepted ->
                                val requestId = message.value.requestId ?: return@onSwitchComputer
                                synchronized(lock) {
                                    if (currentGeneration != generation || stopped || !ready) return@synchronized
                                    if (socket?.send(
                                            ProtocolCodec.encode(
                                                SwitchAckMessage(requestId, message.value.pcId, accepted),
                                            ),
                                        ) != true
                                    ) {
                                        restartSocketLocked()
                                    }
                                }
                            }
                        }
                        is ServerMessage.Error -> {
                            if (message.value.sessionId?.let(queue::acceptsSession) == false) return
                            if (message.value.code == ErrorCode.AUTH_FAILED) {
                                stopped = true
                                invalidateSocketLocked()
                                socket?.close(1008, "authentication failed")
                                socket = null
                                listener.onPairingInvalid(current)
                            } else {
                                if (message.value.code == ErrorCode.TARGET_SUBMITTED) {
                                    // The Windows target accepted the Enter key and
                                    // already ended the remote session. Drop the
                                    // queued snapshot so the next text starts cleanly.
                                    queue.abandonSession()
                                } else {
                                    queue.requireExplicitStart()
                                }
                                listener.onServerError(current, message.value)
                            }
                        }
                    }
                }
            }
        }

        override fun onClosed(webSocket: WebSocket, code: Int, reason: String) {
            connectionLost(false)
        }

        override fun onFailure(webSocket: WebSocket, error: Throwable, response: Response?) {
            connectionLost(true)
        }

        private fun connectionLost(failed: Boolean) {
            synchronized(lock) {
                connectionLostLocked(current, currentGeneration, failed, cancelSocket = false)
            }
        }
    }

    private fun connectionLostLocked(
        current: ComputerBinding,
        currentGeneration: Long,
        failed: Boolean,
        cancelSocket: Boolean,
    ) {
        if (currentGeneration != generation || stopped) return
        val reconnectBinding = binding?.takeIf { it.pcId == current.pcId } ?: current
        generation += 1
        ready = false
        connectTimeoutRunnable?.let(handler::removeCallbacks)
        connectTimeoutRunnable = null
        healthTimeoutRunnable?.let(handler::removeCallbacks)
        healthTimeoutRunnable = null
        healthCheckPending = false
        targetRetryRunnable?.let(handler::removeCallbacks)
        targetRetryRunnable = null
        if (cancelSocket) socket?.cancel()
        socket = null
        queue.onDisconnected()
        pendingImageId?.let(listener::onImageTransferFailed)
        pendingImageId = null
        if (failed) listener.onFailure(reconnectBinding) else listener.onDisconnected(reconnectBinding)
        binding = reconnectBinding.nextPairingEndpoint()
        scheduleReconnectLocked()
    }

    private companion object {
        const val CONNECT_TIMEOUT_MS = 4_000L
        const val HEALTH_IDLE_MS = 30_000L
        const val HEALTH_TIMEOUT_MS = 1_500L
        val RECONNECT_DELAYS = longArrayOf(250L, 1_000L, 2_000L, 5_000L, 10_000L)
    }

    // The QR supplies this out-of-band SPKI identity; normal CA trust cannot validate the self-signed PC.
    @SuppressLint("CustomX509TrustManager")
    private class PinnedTrustManager(expectedHash: String) : X509TrustManager {
        private val expected = Base64.decode(expectedHash, Base64.DEFAULT)

        override fun checkServerTrusted(chain: Array<out X509Certificate>?, authType: String?) {
            val certificate = chain?.firstOrNull() ?: throw CertificateException("missing certificate")
            val actual = MessageDigest.getInstance("SHA-256").digest(certificate.publicKey.encoded)
            if (!MessageDigest.isEqual(expected, actual)) {
                throw CertificateException("computer identity does not match")
            }
        }

        override fun checkClientTrusted(chain: Array<out X509Certificate>?, authType: String?) {
            throw CertificateException("client certificates are not used")
        }

        override fun getAcceptedIssuers(): Array<X509Certificate> = emptyArray()
    }
}
