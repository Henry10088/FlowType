package app.flowtype.network

import android.annotation.SuppressLint
import android.content.ContentResolver
import android.net.Uri
import android.util.Base64
import app.flowtype.data.StoredTransferEntry
import app.flowtype.pairing.ComputerBinding
import app.flowtype.protocol.FileTicketMessage
import app.flowtype.protocol.FileTransferCodec
import app.flowtype.protocol.TransferManifestEntry
import okhttp3.ConnectionSpec
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody
import okhttp3.Response
import okhttp3.TlsVersion
import okhttp3.HttpUrl.Companion.toHttpUrl
import okhttp3.RequestBody.Companion.toRequestBody
import okio.BufferedSink
import org.json.JSONObject
import java.io.Closeable
import java.io.EOFException
import java.security.MessageDigest
import java.security.SecureRandom
import java.security.cert.CertificateException
import java.security.cert.X509Certificate
import java.util.concurrent.TimeUnit
import javax.net.ssl.SSLContext
import javax.net.ssl.TrustManager
import javax.net.ssl.X509TrustManager

/**
 * File data uses a separate HTTPS connection so large transfers cannot delay
 * text snapshots on the long-lived WebSocket. The PC certificate is pinned to
 * the permanent binding created by the QR pairing flow.
 */
class FileTransferHttpClient(binding: ComputerBinding) : Closeable {
    private val controlUrl = fileTransferBaseUrl(binding.endpoint).toHttpUrl()
    private val client = buildClient(binding)

    fun uploadManifest(
        ticket: FileTicketMessage,
        entries: Sequence<TransferManifestEntry>,
    ): Response {
        ticket.validate(System.currentTimeMillis() / 1_000)
        val url = controlUrl.newBuilder()
            .scheme("https")
            .port(ticket.dataPort)
            .encodedPath("/flowtype/files/v1/batches/${ticket.batchId}/manifest")
            .query(null)
            .fragment(null)
            .build()
        val body = object : RequestBody() {
            override fun contentType() = NDJSON

            override fun writeTo(sink: BufferedSink) {
                entries.forEach { entry ->
                    entry.validate()
                    sink.writeUtf8(FileTransferCodec.encodeManifestEntry(entry))
                    sink.writeByte('\n'.code)
                }
            }
        }
        val request = Request.Builder()
            .url(url)
            .header("Authorization", "Bearer ${ticket.token}")
            .header("Cache-Control", "no-store")
            .post(body)
            .build()
        return client.newCall(request).execute()
    }

    fun queryCommittedOffset(ticket: FileTicketMessage, entry: TransferManifestEntry): Long {
        val request = authorizedRequest(ticket, "entries/${entry.entryId}")
            .header("x-flowtype-ordinal", entry.ordinal.toString())
            .head()
            .build()
        client.newCall(request).execute().use { response ->
            checkResponse(response, "resume query failed")
            check(response.header("x-flowtype-length")?.toLongOrNull() == entry.size) {
                "remote file size changed"
            }
            return response.header("x-flowtype-offset")?.toLongOrNull()
                ?.takeIf { it in 0..entry.size }
                ?: error("invalid resume offset")
        }
    }

    fun uploadFile(
        resolver: ContentResolver,
        ticket: FileTicketMessage,
        stored: StoredTransferEntry,
        offset: Long,
        onProgress: (Long) -> Unit,
    ): FileUploadReceipt {
        val source = Uri.parse(requireNotNull(stored.sourceUri) { "source URI is unavailable" })
        val entry = stored.manifest
        entry.validate()
        require(offset in 0..entry.size)
        var localHash: String? = null
        val remainingLength = entry.size - offset
        val body = object : RequestBody() {
            override fun contentType() = OCTET_STREAM
            override fun contentLength(): Long = remainingLength

            override fun writeTo(sink: BufferedSink) {
                resolver.openInputStream(source).use { input ->
                    requireNotNull(input) { "source URI cannot be opened" }
                    val buffer = ByteArray(512 * 1024)
                    var prefix = offset
                    while (prefix > 0) {
                        val read = input.read(buffer, 0, minOf(prefix, buffer.size.toLong()).toInt())
                        if (read < 0) throw EOFException("source became shorter")
                        prefix -= read
                    }
                    var remaining = remainingLength
                    while (remaining > 0) {
                        val read = input.read(buffer, 0, minOf(remaining, buffer.size.toLong()).toInt())
                        if (read < 0) throw EOFException("source became shorter")
                        sink.write(buffer, 0, read)
                        onProgress(read.toLong())
                        remaining -= read
                    }
                    check(input.read() == -1) { "source became larger" }
                }
                localHash = hashSource(resolver, source)
            }
        }
        val request = authorizedRequest(ticket, "entries/${entry.entryId}")
            .header("x-flowtype-ordinal", entry.ordinal.toString())
            .header("x-flowtype-offset", offset.toString())
            .post(body)
            .build()
        client.newCall(request).execute().use { response ->
            checkResponse(response, "file upload failed")
            val value = JSONObject(requireNotNull(response.body).string())
            val committed = value.getLong("committed_offset")
            val completed = value.getBoolean("completed")
            val remoteHash = value.optString("sha256").ifEmpty { null }
            if (completed) {
                check(committed == entry.size && remoteHash != null && remoteHash == localHash) {
                    "file integrity check failed"
                }
            }
            return FileUploadReceipt(committed, completed, remoteHash)
        }
    }

    fun completeBatch(ticket: FileTicketMessage) {
        val request = authorizedRequest(ticket, "complete")
            .post(EMPTY_BODY)
            .build()
        client.newCall(request).execute().use { response ->
            checkResponse(response, "batch completion failed")
        }
    }

    fun reportFileFailure(
        ticket: FileTicketMessage,
        entry: TransferManifestEntry,
        errorCode: String,
    ) {
        require(errorCode.matches(Regex("[a-z0-9_]{1,64}")))
        val request = authorizedRequest(ticket, "failures/${entry.entryId}")
            .header("x-flowtype-ordinal", entry.ordinal.toString())
            .header("x-flowtype-error", errorCode)
            .post(EMPTY_BODY)
            .build()
        client.newCall(request).execute().use { response ->
            checkResponse(response, "file failure report failed")
        }
    }

    fun downloadManifestPage(ticket: FileTicketMessage, startOrdinal: Long): ManifestPage {
        val request = authorizedRequest(ticket, "manifest")
            .header("x-flowtype-start", startOrdinal.toString())
            .get()
            .build()
        client.newCall(request).execute().use { response ->
            checkResponse(response, "manifest download failed")
            val entries = requireNotNull(response.body).source().use { source ->
                buildList {
                    while (!source.exhausted()) {
                        val line = source.readUtf8Line() ?: break
                        if (line.isNotEmpty()) add(FileTransferCodec.decodeManifestEntry(line))
                    }
                }
            }
            val next = response.header("x-flowtype-next")?.toLongOrNull()
                ?: error("manifest cursor is unavailable")
            val count = response.header("x-flowtype-count")?.toLongOrNull()
                ?: error("manifest count is unavailable")
            check(next >= startOrdinal && next <= count)
            return ManifestPage(entries, next, count)
        }
    }

    fun downloadChunk(
        ticket: FileTicketMessage,
        entry: TransferManifestEntry,
        offset: Long,
    ): ByteArray {
        require(offset in 0 until entry.size)
        val request = authorizedRequest(ticket, "entries/${entry.entryId}")
            .header("x-flowtype-ordinal", entry.ordinal.toString())
            .header("Range", "bytes=$offset-")
            .get()
            .build()
        client.newCall(request).execute().use { response ->
            if (response.code != 206) throw responseError(response, "file download failed")
            val bytes = requireNotNull(response.body).bytes()
            check(bytes.isNotEmpty() && offset + bytes.size <= entry.size) { "invalid file chunk" }
            return bytes
        }
    }

    fun verifyDownloadedFile(
        ticket: FileTicketMessage,
        entry: TransferManifestEntry,
        sha256: String,
    ) {
        require(sha256.matches(Regex("[0-9a-f]{64}")))
        val request = authorizedRequest(ticket, "verify/${entry.entryId}")
            .header("x-flowtype-ordinal", entry.ordinal.toString())
            .header("x-flowtype-sha256", sha256)
            .post(EMPTY_BODY)
            .build()
        client.newCall(request).execute().use { response ->
            checkResponse(response, "file verification failed")
        }
    }

    private fun authorizedRequest(ticket: FileTicketMessage, action: String): Request.Builder {
        ticket.validate(System.currentTimeMillis() / 1_000)
        val url = controlUrl.newBuilder()
            .scheme("https")
            .port(ticket.dataPort)
            .encodedPath("/flowtype/files/v1/batches/${ticket.batchId}/$action")
            .query(null)
            .fragment(null)
            .build()
        return Request.Builder()
            .url(url)
            .header("Authorization", "Bearer ${ticket.token}")
            .header("Cache-Control", "no-store")
    }

    private fun checkResponse(response: Response, action: String) {
        if (!response.isSuccessful) throw responseError(response, action)
    }

    private fun responseError(response: Response, action: String): IllegalStateException {
        val code = runCatching {
            JSONObject(response.peekBody(8 * 1024).string()).optString("code")
        }.getOrDefault("")
        return IllegalStateException(
            if (code.isBlank()) "$action: HTTP ${response.code}" else "$action: $code",
        )
    }

    override fun close() {
        cancel()
        client.dispatcher.executorService.shutdown()
        client.connectionPool.evictAll()
    }

    fun cancel() {
        client.dispatcher.cancelAll()
    }

    private fun hashSource(resolver: ContentResolver, source: Uri): String {
        val digest = MessageDigest.getInstance("SHA-256")
        resolver.openInputStream(source).use { input ->
            requireNotNull(input) { "source URI cannot be opened" }
            val buffer = ByteArray(512 * 1024)
            while (true) {
                val read = input.read(buffer)
                if (read < 0) break
                digest.update(buffer, 0, read)
            }
        }
        return digest.digest().joinToString("") { "%02x".format(it) }
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
            .connectTimeout(5, TimeUnit.SECONDS)
            // These are per-I/O inactivity limits, not a whole-transfer deadline.
            // Keeping them finite lets a black-holed Wi-Fi connection enter the
            // resumable waiting-network state instead of hanging forever.
            .readTimeout(FILE_IO_IDLE_TIMEOUT_SECONDS, TimeUnit.SECONDS)
            .writeTimeout(FILE_IO_IDLE_TIMEOUT_SECONDS, TimeUnit.SECONDS)
            .retryOnConnectionFailure(false)
            .build()
    }

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

    private companion object {
        val NDJSON = "application/x-ndjson".toMediaType()
        val OCTET_STREAM = "application/octet-stream".toMediaType()
        val EMPTY_BODY = ByteArray(0).toRequestBody(OCTET_STREAM)
    }
}

internal const val FILE_IO_IDLE_TIMEOUT_SECONDS = 30L

data class FileUploadReceipt(
    val committedOffset: Long,
    val completed: Boolean,
    val sha256: String?,
)

data class ManifestPage(
    val entries: List<TransferManifestEntry>,
    val nextOrdinal: Long,
    val totalEntries: Long,
)

internal fun fileTransferBaseUrl(controlEndpoint: String): String = when {
    controlEndpoint.startsWith("wss://") -> "https://" + controlEndpoint.removePrefix("wss://")
    controlEndpoint.startsWith("ws://") -> "http://" + controlEndpoint.removePrefix("ws://")
    else -> error("unsupported control endpoint")
}
