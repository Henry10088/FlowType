package app.flowtype.network

import android.annotation.SuppressLint
import android.util.Base64
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
import okio.BufferedSink
import java.io.Closeable
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
            .retryOnConnectionFailure(true)
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
    }
}
