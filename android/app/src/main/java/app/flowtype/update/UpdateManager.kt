package app.flowtype.update

import android.app.DownloadManager
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Environment
import android.provider.Settings
import androidx.core.content.FileProvider
import app.flowtype.R
import okhttp3.CacheControl
import okhttp3.OkHttpClient
import okhttp3.Request
import org.json.JSONObject
import java.io.File
import java.security.KeyFactory
import java.security.MessageDigest
import java.security.Signature
import java.security.spec.X509EncodedKeySpec
import java.util.Base64
import java.util.concurrent.CopyOnWriteArraySet
import java.util.concurrent.Executors
import java.util.concurrent.ScheduledFuture
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean

class UpdateManager(
    private val context: Context,
    private val installationBusy: () -> Boolean,
) {
    enum class Action { NONE, CHECK, DOWNLOAD, CANCEL, INSTALL }

    data class State(
        val message: String,
        val action: Action,
        val actionLabel: String,
        val downloaded: Long = 0,
        val total: Long = 0,
        val releaseUrl: String? = null,
        val version: String? = null,
    ) {
        val showProgress: Boolean get() = total > 0 && downloaded in 0..total
    }

    internal data class Asset(val url: String, val sha256: String, val size: Long)
    internal data class Manifest(
        val raw: ByteArray,
        val signature: ByteArray,
        val version: String,
        val versionCode: Long,
        val releaseUrl: String,
        val notes: String,
        val android: Asset,
    )

    private val preferences = context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE)
    private val currentPackage = context.packageManager.getPackageInfo(context.packageName, 0)
    private val currentVersionCode = currentPackage.longVersionCode
    private val currentVersionName = currentPackage.versionName ?: currentVersionCode.toString()
    private val downloads = context.getSystemService(DownloadManager::class.java)
    private val client = OkHttpClient.Builder()
        .connectTimeout(12, TimeUnit.SECONDS)
        .readTimeout(20, TimeUnit.SECONDS)
        .callTimeout(30, TimeUnit.SECONDS)
        .build()
    private val executor = Executors.newSingleThreadScheduledExecutor { runnable ->
        Thread(runnable, "flowtype-update").apply { isDaemon = true }
    }
    private val observers = CopyOnWriteArraySet<(State) -> Unit>()
    private val checking = AtomicBoolean(false)
    @Volatile private var state = idleState()
    @Volatile private var available: Manifest? = null
    @Volatile private var polling: ScheduledFuture<*>? = null

    init {
        restoreDownload()
        executor.schedule({ check(manual = false) }, AUTO_CHECK_DELAY_SECONDS, TimeUnit.SECONDS)
    }

    fun state(): State = state

    fun observe(observer: (State) -> Unit) {
        observers += observer
        observer(state)
    }

    fun removeObserver(observer: (State) -> Unit) {
        observers -= observer
    }

    fun perform(action: Action) {
        when (action) {
            Action.CHECK -> executor.execute { check(manual = true) }
            Action.DOWNLOAD -> executor.execute(::startDownload)
            Action.CANCEL -> executor.execute(::cancelDownload)
            else -> Unit
        }
    }

    fun releaseUrl(): Uri? = state.releaseUrl?.let(Uri::parse)

    /** Returns an installer intent only after repeating every local package check. */
    fun prepareInstall(): Result<Intent> {
        if (installationBusy()) return Result.failure(IllegalStateException(context.getString(R.string.update_finish_work_first)))
        val manifest = available ?: loadPersistedManifest()
            ?: return Result.failure(IllegalStateException(context.getString(R.string.update_manifest_unavailable)))
        val file = updateFile(manifest)
        return runCatching {
            verifyApk(file, manifest.android)
            val uri = FileProvider.getUriForFile(context, "${context.packageName}.files", file)
            Intent(Intent.ACTION_VIEW).apply {
                setDataAndType(uri, "application/vnd.android.package-archive")
                addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_ACTIVITY_NEW_TASK)
            }
        }.recoverCatching {
            throw IllegalStateException(context.getString(R.string.update_verification_failed), it)
        }.onFailure { setFailure(manifest, it.message ?: context.getString(R.string.update_verification_failed)) }
    }

    fun canRequestPackageInstalls(): Boolean = context.packageManager.canRequestPackageInstalls()

    fun unknownSourcesIntent(): Intent = Intent(
        Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES,
        Uri.parse("package:${context.packageName}"),
    ).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)

    fun refreshInstallAvailability() {
        if (preferences.getBoolean(DOWNLOAD_VERIFIED, false)) {
            (available ?: loadPersistedManifest())?.let { setReady(it.version) }
        }
    }

    private fun check(manual: Boolean) {
        if (!manual && !shouldAutoCheck()) return
        if (!checking.compareAndSet(false, true)) return
        setState(State(context.getString(R.string.update_checking), Action.NONE, ""))
        try {
            val manifest = fetchVerifiedManifest()
            val highest = preferences.getLong(HIGHEST_VERSION_CODE, 0)
            require(manifest.versionCode >= highest) { "update manifest rolled back" }
            preferences.edit()
                .putLong(LAST_SUCCESSFUL_CHECK, System.currentTimeMillis())
                .putLong(HIGHEST_VERSION_CODE, maxOf(highest, manifest.versionCode))
                .apply()
            if (manifest.versionCode > currentVersionCode) {
                available = manifest
                persistManifest(manifest)
                val downloaded = updateFile(manifest)
                if (preferences.getBoolean(DOWNLOAD_VERIFIED, false) && downloaded.isFile) {
                    runCatching { verifyApk(downloaded, manifest.android) }
                        .onSuccess { setReady(manifest.version) }
                        .onFailure {
                            downloaded.delete()
                            preferences.edit().putBoolean(DOWNLOAD_VERIFIED, false).apply()
                            setAvailable(manifest)
                        }
                } else {
                    setAvailable(manifest)
                }
            } else {
                available = null
                setState(State(
                    message = context.getString(R.string.update_latest),
                    action = Action.CHECK,
                    actionLabel = context.getString(R.string.update_check_again),
                    releaseUrl = manifest.releaseUrl,
                ))
            }
        } catch (error: Exception) {
            val cached = available ?: loadPersistedManifest()
            if (cached != null && cached.versionCode > currentVersionCode) {
                available = cached
                setState(State(
                    message = context.getString(R.string.update_cached_available, cached.version),
                    action = Action.DOWNLOAD,
                    actionLabel = context.getString(R.string.update_download),
                    releaseUrl = cached.releaseUrl,
                    version = cached.version,
                ))
            } else {
                setState(State(
                    message = context.getString(R.string.update_check_failed, friendlyError(error)),
                    action = Action.CHECK,
                    actionLabel = context.getString(R.string.retry),
                    releaseUrl = RELEASES_URL,
                ))
            }
        } finally {
            checking.set(false)
        }
    }

    private fun fetchVerifiedManifest(): Manifest {
        val bytes = get(MANIFEST_URL, MAX_MANIFEST_BYTES)
        val untrustedVersion = JSONObject(bytes.toString(Charsets.UTF_8)).optString("version")
        require(parseVersion(untrustedVersion) != null) { "invalid update version" }
        val signatureUrl = "$RELEASE_DOWNLOAD_PREFIX/v$untrustedVersion/flowtype-update.json.sig"
        val signature = get(signatureUrl, MAX_SIGNATURE_BYTES)
        return verifyManifest(bytes, signature)
    }

    private fun get(url: String, limit: Int): ByteArray {
        val request = Request.Builder()
            .url(url)
            .cacheControl(CacheControl.FORCE_NETWORK)
            .header("Accept", "application/octet-stream, application/json")
            .build()
        client.newCall(request).execute().use { response ->
            require(response.isSuccessful) { context.getString(R.string.update_server_error, response.code) }
            val body = response.body ?: error(context.getString(R.string.update_invalid_response))
            val declared = body.contentLength()
            require(declared < 0 || declared <= limit) { context.getString(R.string.update_invalid_response) }
            val bytes = body.byteStream().use { input ->
                val output = java.io.ByteArrayOutputStream(minOf(limit, 8192))
                val buffer = ByteArray(8192)
                while (true) {
                    val read = input.read(buffer)
                    if (read < 0) break
                    require(output.size() + read <= limit) { context.getString(R.string.update_invalid_response) }
                    output.write(buffer, 0, read)
                }
                output.toByteArray()
            }
            return bytes
        }
    }

    private fun startDownload() {
        val manifest = available ?: loadPersistedManifest() ?: return
        cancelDownload(removeState = false)
        try {
            val file = updateFile(manifest)
            file.parentFile?.mkdirs()
            file.delete()
            val request = DownloadManager.Request(Uri.parse(manifest.android.url))
                .setTitle(context.getString(R.string.update_download_title, manifest.version))
                .setDescription(context.getString(R.string.update_downloading))
                .setMimeType("application/vnd.android.package-archive")
                .setNotificationVisibility(DownloadManager.Request.VISIBILITY_VISIBLE_NOTIFY_COMPLETED)
                .setAllowedOverMetered(true)
                .setAllowedOverRoaming(false)
                .setDestinationUri(Uri.fromFile(file))
            val id = downloads.enqueue(request)
            preferences.edit().putLong(DOWNLOAD_ID, id).apply()
            persistManifest(manifest)
            setDownloading(manifest, 0, manifest.android.size, context.getString(R.string.update_preparing_download))
            beginPolling(id, manifest)
        } catch (error: Exception) {
            setFailure(manifest, context.getString(R.string.update_start_failed, friendlyError(error)))
        }
    }

    private fun beginPolling(id: Long, manifest: Manifest) {
        polling?.cancel(false)
        polling = executor.scheduleWithFixedDelay(
            { pollDownload(id, manifest) }, 0, 1, TimeUnit.SECONDS,
        )
    }

    private fun pollDownload(id: Long, manifest: Manifest) {
        val query = DownloadManager.Query().setFilterById(id)
        downloads.query(query)?.use { cursor ->
            if (!cursor.moveToFirst()) {
                polling?.cancel(false)
                preferences.edit().remove(DOWNLOAD_ID).apply()
                setFailure(manifest, context.getString(R.string.update_task_missing))
                return
            }
            val status = cursor.getInt(cursor.getColumnIndexOrThrow(DownloadManager.COLUMN_STATUS))
            val downloaded = cursor.getLong(cursor.getColumnIndexOrThrow(DownloadManager.COLUMN_BYTES_DOWNLOADED_SO_FAR))
                .coerceAtLeast(0)
            val reportedTotal = cursor.getLong(cursor.getColumnIndexOrThrow(DownloadManager.COLUMN_TOTAL_SIZE_BYTES))
            val total = if (reportedTotal > 0) reportedTotal else manifest.android.size
            when (status) {
                DownloadManager.STATUS_SUCCESSFUL -> {
                    polling?.cancel(false)
                    setState(State(context.getString(R.string.update_verifying), Action.NONE, "", releaseUrl = manifest.releaseUrl, version = manifest.version))
                    runCatching { verifyApk(updateFile(manifest), manifest.android) }
                        .onSuccess {
                            preferences.edit().remove(DOWNLOAD_ID).putBoolean(DOWNLOAD_VERIFIED, true).apply()
                            available = manifest
                            setReady(manifest.version)
                        }
                        .onFailure {
                            updateFile(manifest).delete()
                            preferences.edit().remove(DOWNLOAD_ID).putBoolean(DOWNLOAD_VERIFIED, false).apply()
                            setFailure(manifest, context.getString(R.string.update_verification_failed))
                        }
                }
                DownloadManager.STATUS_FAILED -> {
                    polling?.cancel(false)
                    preferences.edit().remove(DOWNLOAD_ID).apply()
                    setFailure(manifest, context.getString(R.string.update_download_failed))
                }
                DownloadManager.STATUS_PAUSED -> setDownloading(manifest, downloaded, total, context.getString(R.string.update_waiting_network))
                else -> setDownloading(manifest, downloaded, total, formatProgress(downloaded, total))
            }
        }
    }

    private fun cancelDownload(removeState: Boolean = true) {
        polling?.cancel(false)
        polling = null
        val id = preferences.getLong(DOWNLOAD_ID, -1)
        if (id >= 0) downloads.remove(id)
        preferences.edit().remove(DOWNLOAD_ID).putBoolean(DOWNLOAD_VERIFIED, false).apply()
        val manifest = available ?: loadPersistedManifest()
        if (manifest != null) updateFile(manifest).delete()
        if (removeState && manifest != null) setAvailable(manifest)
    }

    private fun restoreDownload() {
        val manifest = loadPersistedManifest() ?: return
        if (manifest.versionCode <= currentVersionCode) return
        available = manifest
        val file = updateFile(manifest)
        if (preferences.getBoolean(DOWNLOAD_VERIFIED, false) && file.isFile) {
            executor.execute {
                runCatching { verifyApk(file, manifest.android) }
                    .onSuccess { setReady(manifest.version) }
                    .onFailure { file.delete(); setAvailable(manifest) }
            }
            return
        }
        val id = preferences.getLong(DOWNLOAD_ID, -1)
        if (id >= 0) beginPolling(id, manifest) else setAvailable(manifest)
    }

    private fun verifyApk(file: File, asset: Asset) {
        require(file.isFile && file.length() == asset.size) { context.getString(R.string.update_verification_failed) }
        val digest = file.inputStream().use { input ->
            val hash = MessageDigest.getInstance("SHA-256")
            val buffer = ByteArray(64 * 1024)
            while (true) {
                val read = input.read(buffer)
                if (read < 0) break
                hash.update(buffer, 0, read)
            }
            hash.digest().joinToString("") { "%02x".format(it) }
        }
        require(MessageDigest.isEqual(digest.toByteArray(), asset.sha256.toByteArray())) { context.getString(R.string.update_verification_failed) }
        val archive = context.packageManager.getPackageArchiveInfo(
            file.absolutePath,
            PackageManager.GET_SIGNING_CERTIFICATES,
        ) ?: error(context.getString(R.string.update_verification_failed))
        require(archive.packageName == context.packageName) { context.getString(R.string.update_verification_failed) }
        val current = context.packageManager.getPackageInfo(
            context.packageName,
            PackageManager.GET_SIGNING_CERTIFICATES,
        )
        val archiveSigners = archive.signingInfo?.apkContentsSigners?.map { it.toByteArray() } ?: emptyList()
        val currentSigners = current.signingInfo?.apkContentsSigners?.map { it.toByteArray() } ?: emptyList()
        require(sameCertificates(archiveSigners, currentSigners)) { context.getString(R.string.update_verification_failed) }
    }

    private fun setAvailable(manifest: Manifest) = setState(State(
        message = context.getString(R.string.update_found, manifest.version),
        action = Action.DOWNLOAD,
        actionLabel = context.getString(R.string.update_download),
        releaseUrl = manifest.releaseUrl,
        version = manifest.version,
    ))

    private fun setReady(version: String) {
        val manifest = available ?: loadPersistedManifest() ?: return
        val busy = installationBusy()
        setState(State(
            message = context.getString(if (busy) R.string.update_downloaded_busy else R.string.update_downloaded),
            action = if (busy) Action.NONE else Action.INSTALL,
            actionLabel = if (busy) "" else context.getString(R.string.update_install),
            releaseUrl = manifest.releaseUrl,
            version = version,
        ))
    }

    private fun setDownloading(manifest: Manifest, downloaded: Long, total: Long, message: String) = setState(State(
        message = message,
        action = Action.CANCEL,
        actionLabel = context.getString(R.string.cancel),
        downloaded = downloaded,
        total = total,
        releaseUrl = manifest.releaseUrl,
        version = manifest.version,
    ))

    private fun setFailure(manifest: Manifest, message: String) = setState(State(
        message = message,
        action = Action.DOWNLOAD,
        actionLabel = context.getString(R.string.retry),
        releaseUrl = manifest.releaseUrl,
        version = manifest.version,
    ))

    private fun setState(value: State) {
        state = value
        observers.forEach { observer -> runCatching { observer(value) } }
    }

    private fun idleState() = State(
        message = context.getString(R.string.update_current_version, currentVersionName),
        action = Action.CHECK,
        actionLabel = context.getString(R.string.check_updates),
    )

    private fun shouldAutoCheck(): Boolean =
        System.currentTimeMillis() - preferences.getLong(LAST_SUCCESSFUL_CHECK, 0) >= CHECK_INTERVAL_MILLIS

    private fun persistManifest(manifest: Manifest) {
        preferences.edit()
            .putString(MANIFEST_JSON, manifest.raw.toString(Charsets.UTF_8))
            .putString(MANIFEST_SIGNATURE, Base64.getEncoder().encodeToString(manifest.signature))
            .apply()
    }

    private fun loadPersistedManifest(): Manifest? {
        val json = preferences.getString(MANIFEST_JSON, null) ?: return null
        val encodedSignature = preferences.getString(MANIFEST_SIGNATURE, null) ?: return null
        return runCatching {
            verifyManifest(
                json.toByteArray(Charsets.UTF_8),
                Base64.getDecoder().decode(encodedSignature),
            )
        }.getOrNull()
    }

    private fun updateFile(manifest: Manifest): File {
        val root = context.getExternalFilesDir(Environment.DIRECTORY_DOWNLOADS)
            ?: File(context.filesDir, "downloads")
        return File(File(root, "updates"), "FlowType-${manifest.version}-android-release.apk")
    }

    private fun formatProgress(downloaded: Long, total: Long): String {
        if (total <= 0) return context.getString(R.string.update_downloaded_amount, formatBytes(downloaded))
        val percent = (downloaded * 100 / total).coerceIn(0, 100)
        return context.getString(
            R.string.update_download_progress,
            percent,
            formatBytes(downloaded),
            formatBytes(total),
        )
    }

    private fun friendlyError(error: Exception): String {
        if (error is java.net.SocketTimeoutException) return context.getString(R.string.error_timeout)
        if (error is java.net.UnknownHostException || error is java.io.IOException) {
            return context.getString(R.string.error_network_unavailable)
        }
        val serverPrefix = context.getString(R.string.update_server_error, 0).substringBefore('0')
        return error.message?.takeIf { it.startsWith(serverPrefix) }
            ?: context.getString(R.string.update_invalid_response)
    }

    companion object {
        private const val MANIFEST_URL = "https://github.com/Henry10088/FlowType/releases/latest/download/flowtype-update.json"
        private const val RELEASE_DOWNLOAD_PREFIX = "https://github.com/Henry10088/FlowType/releases/download"
        private const val RELEASE_TAG_PREFIX = "https://github.com/Henry10088/FlowType/releases/tag/"
        private const val RELEASES_URL = "https://github.com/Henry10088/FlowType/releases"
        private const val KEY_ID = "flowtype-update-2026"
        private const val PUBLIC_KEY = "MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAE81gLLKum3oiKT8hYqGYfnYpgeHmAt/xnfD4F39yrg+5/++M5/UNSUvU2aWA7iLro/+irFe/SwoHQ45WPVLnAdw=="
        private const val MAX_MANIFEST_BYTES = 64 * 1024
        private const val MAX_SIGNATURE_BYTES = 1024
        private const val MAX_APK_BYTES = 200L * 1024 * 1024
        private const val AUTO_CHECK_DELAY_SECONDS = 30L
        private const val CHECK_INTERVAL_MILLIS = 24L * 60 * 60 * 1000
        private const val PREFERENCES = "flowtype-update-v1"
        private const val LAST_SUCCESSFUL_CHECK = "last-successful-check"
        private const val HIGHEST_VERSION_CODE = "highest-version-code"
        private const val DOWNLOAD_ID = "download-id"
        private const val DOWNLOAD_VERIFIED = "download-verified"
        private const val MANIFEST_JSON = "manifest-json"
        private const val MANIFEST_SIGNATURE = "manifest-signature"

        internal fun verifyManifest(bytes: ByteArray, signatureText: ByteArray): Manifest {
            require(bytes.size <= MAX_MANIFEST_BYTES && signatureText.size <= MAX_SIGNATURE_BYTES) { "update manifest too large" }
            val publicKey = KeyFactory.getInstance("EC").generatePublic(
                X509EncodedKeySpec(Base64.getDecoder().decode(PUBLIC_KEY)),
            )
            val verifier = Signature.getInstance("SHA256withECDSA")
            verifier.initVerify(publicKey)
            verifier.update(bytes)
            val signature = Base64.getDecoder().decode(signatureText.toString(Charsets.UTF_8).trim())
            require(verifier.verify(signature)) { "update manifest signature mismatch" }
            return parseAndValidateManifest(bytes).copy(signature = signatureText.copyOf())
        }

        internal fun parseAndValidateManifest(bytes: ByteArray): Manifest {
            require(bytes.size <= MAX_MANIFEST_BYTES) { "update manifest too large" }
            val root = JSONObject(bytes.toString(Charsets.UTF_8))
            require(root.getInt("schema") == 1 && root.getString("key_id") == KEY_ID) { "unsupported update manifest" }
            val version = root.getString("version")
            require(parseVersion(version) != null) { "invalid update version" }
            require(root.optString("published_at").length <= 40) { "update manifest field too long" }
            val notes = root.optString("notes_zh_cn")
            require(notes.length <= 8192) { "update manifest field too long" }
            val releaseUrl = root.getString("release_url")
            require(releaseUrl == "${RELEASE_TAG_PREFIX}v$version") { "invalid release URL" }
            val android = root.getJSONObject("android")
            val versionCode = android.getLong("version_code")
            require(versionCode > 0) { "invalid Android versionCode" }
            val asset = Asset(
                url = android.getString("url"),
                sha256 = android.getString("sha256"),
                size = android.getLong("size"),
            )
            require(asset.url.startsWith("$RELEASE_DOWNLOAD_PREFIX/v$version/") && asset.url.length <= 2048) { "invalid update asset URL" }
            require(asset.size in 1..MAX_APK_BYTES) { "invalid update asset size" }
            require(asset.sha256.matches(Regex("[0-9a-f]{64}"))) { "invalid update asset digest" }
            return Manifest(bytes.copyOf(), byteArrayOf(), version, versionCode, releaseUrl, notes, asset)
        }

        internal fun parseVersion(value: String): Triple<Int, Int, Int>? {
            val match = Regex("^(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)$").matchEntire(value) ?: return null
            return runCatching { Triple(match.groupValues[1].toInt(), match.groupValues[2].toInt(), match.groupValues[3].toInt()) }.getOrNull()
        }

        internal fun sameCertificates(left: List<ByteArray>, right: List<ByteArray>): Boolean {
            if (left.isEmpty() || left.size != right.size) return false
            val normalize: (List<ByteArray>) -> List<String> = { list ->
                list.map { bytes -> MessageDigest.getInstance("SHA-256").digest(bytes).joinToString("") { "%02x".format(it) } }.sorted()
            }
            return normalize(left) == normalize(right)
        }

        private fun formatBytes(bytes: Long): String = when {
            bytes >= 1024 * 1024 -> "%.1f MB".format(bytes / (1024.0 * 1024.0))
            bytes >= 1024 -> "%.1f KB".format(bytes / 1024.0)
            else -> "$bytes B"
        }
    }
}
