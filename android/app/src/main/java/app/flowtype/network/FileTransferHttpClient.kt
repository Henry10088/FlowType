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
    private val controlUrl = binding.endpoint.toHttpUrl()
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
            check(response.isSuccessful) { "resume query failed: ${response.code}" }
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
    ): FileUploadReceipt {
        val source = Uri.parse(requireNotNull(stored.sourceUri) { "source URI is unavailable" })
        val entry = stored.manifest
        entry.validate()
        require(offset in 0..entry.size)
        var localHash: String? = null
        val body = object : RequestBody() {
            override fun contentType() = OCTET_STREAM
            override fun contentLength(): Long = entry.size - offset

            override fun writeTo(sink: BufferedSink) {
                val digest = MessageDigest.getInstance("SHA-256")
                resolver.openInputStream(source).use { input ->
                    requireNotNull(input) { "source URI cannot be opened" }
                    val buffer = ByteArray(512 * 1024)
                    var prefix = offset
                    while (prefix > 0) {
                        val read = input.read(buffer, 0, minOf(prefix, buffer.size.toLong()).toInt())
                        if (read < 0) throw EOFException("source became shorter")
                        digest.update(buffer, 0, read)
                        prefix -= read
                    }
                    var remaining = entry.size - offset
                    while (remaining > 0) {
                        val read = input.read(buffer, 0, minOf(remaining, buffer.size.toLong()).toInt())
                        if (read < 0) throw EOFException("source became shorter")
                        sink.write(buffer, 0, read)
                        digest.update(buffer, 0, read)
                        remaining -= read
                    }
                    check(input.read() == -1) { "source became larger" }
                }
                localHash = digest.digest().joinToString("") { "%02x".format(it) }
            }
        }
        val request = authorizedRequest(ticket, "entries/${entry.entryId}")
            .header("x-flowtype-ordinal", entry.ordinal.toString())
            .header("x-flowtype-offset", offset.toString())
            .post(body)
            .build()
        client.newCall(request).execute().use { response ->
            check(response.isSuccessful) { "file upload failed: ${response.code}" }
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
            check(response.isSuccessful) { "batch completion failed: ${response.code}" }
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

    override fun close() {
        client.dispatcher.executorService.shutdown()
        client.connectionPool.evictAll()
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
            .readTimeout(0, TimeUnit.MILLISECONDS)
            .writeTimeout(0, TimeUnit.MILLISECONDS)
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

data class FileUploadReceipt(
    val committedOffset: Long,
    val completed: Boolean,
    val sha256: String?,
)
