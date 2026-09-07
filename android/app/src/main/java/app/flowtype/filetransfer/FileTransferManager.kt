package app.flowtype.filetransfer

import android.content.Context
import android.net.Uri
import android.provider.DocumentsContract
import android.util.Log
import app.flowtype.data.FileTransferBatch
import app.flowtype.data.FileTransferStore
import app.flowtype.data.StoredTransferEntry
import app.flowtype.network.FileTransferHttpClient
import app.flowtype.pairing.ComputerBinding
import app.flowtype.protocol.FileOfferMessage
import app.flowtype.protocol.FileDecisionMessage
import app.flowtype.protocol.FileTicketMessage
import app.flowtype.protocol.TransferBatchState
import app.flowtype.protocol.TransferDirection
import app.flowtype.protocol.TransferEntryKind
import app.flowtype.protocol.TransferEntryState
import java.io.Closeable
import java.io.EOFException
import java.io.FileOutputStream
import java.io.FileNotFoundException
import java.nio.ByteBuffer
import java.security.MessageDigest
import java.util.UUID
import java.util.concurrent.Executors
import java.util.concurrent.CancellationException
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.ExecutionException
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicLong

class FileTransferManager(
    private val context: Context,
    private val store: FileTransferStore,
    private val phoneId: String,
    private val sendOffer: (FileOfferMessage) -> Boolean,
    private val sendDecision: (String, Boolean) -> Boolean,
    private val sendCancel: (String) -> Boolean,
    private val parallelism: () -> Int,
    private val onChanged: (Snapshot) -> Unit,
) : Closeable {
    data class Snapshot(
        val active: Boolean,
        val batchId: String? = null,
        val completedBytes: Long = 0,
        val totalBytes: Long = 0,
        val error: String? = null,
        val incomingOffer: FileOfferMessage? = null,
        val needsReselection: Boolean = false,
        val failedPaths: List<String> = emptyList(),
    )

    private val worker = Executors.newSingleThreadScheduledExecutor { runnable ->
        Thread(runnable, "flowtype-file-transfer").apply { isDaemon = true }
    }
    @Volatile private var binding: ComputerBinding? = null
    @Volatile private var pendingIncomingOffer: FileOfferMessage? = null
    @Volatile private var activeBatchId: String? = null
    @Volatile private var activeHttpClient: FileTransferHttpClient? = null
    @Volatile private var pendingReselectionBatchId: String? = null
    private val canceledBatches = ConcurrentHashMap.newKeySet<String>()

    fun setBinding(value: ComputerBinding?) {
        binding = value
    }

    fun createFiles(uris: List<Uri>) = create { AndroidManifestBuilder.files(context.contentResolver, uris) }

    fun createDirectory(uri: Uri) = create { AndroidManifestBuilder.directory(context.contentResolver, uri) }

    fun cancelActive() {
        val batchId = activeBatchId ?: pendingIncomingOffer?.batchId ?: return
        canceledBatches += batchId
        activeHttpClient?.cancel()
        pendingIncomingOffer = null
        sendCancel(batchId)
        worker.execute {
            runCatching {
                val batch = store.loadBatch(batchId) ?: return@runCatching
                if (!batch.state.terminal) {
                    store.updateBatchState(batchId, TransferBatchState.CANCELED, System.currentTimeMillis())
                }
                cleanupPartialTargets(batchId)
            }
            activeBatchId = null
            onChanged(Snapshot(false))
        }
    }

    fun onTicket(ticket: FileTicketMessage) {
        val current = binding ?: return
        worker.execute {
            val batch = store.loadBatch(ticket.batchId) ?: return@execute
            if (batch.direction == TransferDirection.PHONE_TO_COMPUTER) {
                upload(current, ticket)
            } else {
                download(current, ticket)
            }
        }
    }

    fun onOffer(offer: FileOfferMessage) {
        val current = binding ?: return
        if (offer.direction != TransferDirection.COMPUTER_TO_PHONE || offer.phoneId != phoneId) return
        worker.execute {
            runCatching {
                offer.validate(phoneId)
                check(current.pcId == binding?.pcId) { "computer changed" }
                val existing = store.loadBatch(offer.batchId)
                if (existing == null) {
                    val now = System.currentTimeMillis()
                    store.createBatch(
                        FileTransferBatch(
                            batchId = offer.batchId,
                            pcId = current.pcId,
                            direction = offer.direction,
                            state = TransferBatchState.WAITING_CONFIRMATION,
                            createdAt = now,
                            updatedAt = now,
                            fileCount = offer.fileCount,
                            directoryCount = offer.directoryCount,
                            totalBytes = offer.totalBytes,
                            targetTreeUri = null,
                        ),
                    )
                    pendingIncomingOffer = offer
                    onChanged(Snapshot(false, offer.batchId, totalBytes = offer.totalBytes, incomingOffer = offer))
                } else {
                    check(
                        existing.pcId == current.pcId &&
                            existing.direction == offer.direction &&
                            existing.fileCount == offer.fileCount &&
                            existing.directoryCount == offer.directoryCount &&
                            existing.totalBytes == offer.totalBytes,
                    ) { "batch id conflict" }
                    if (!existing.state.terminal && existing.targetTreeUri != null) {
                        if (!sendDecision(offer.batchId, true)) {
                            onChanged(Snapshot(true, offer.batchId, totalBytes = offer.totalBytes, error = "waiting_network"))
                        }
                    } else if (!existing.state.terminal) {
                        pendingIncomingOffer = offer
                        onChanged(Snapshot(false, offer.batchId, totalBytes = offer.totalBytes, incomingOffer = offer))
                    }
                }
            }.onFailure { error ->
                onChanged(Snapshot(false, offer.batchId, totalBytes = offer.totalBytes, error = error.message ?: "invalid_offer"))
            }
        }
    }

    fun acceptIncoming(treeUri: Uri) {
        val offer = pendingIncomingOffer
        val reselectBatchId = pendingReselectionBatchId
        if (offer == null && reselectBatchId == null) return
        worker.execute {
            runCatching {
                val batchId = offer?.batchId ?: requireNotNull(reselectBatchId)
                val batch = requireNotNull(store.loadBatch(batchId))
                store.setTargetTree(batchId, treeUri.toString(), System.currentTimeMillis())
                store.updateBatchState(
                    batchId,
                    if (batch.state == TransferBatchState.WAITING_CONFIRMATION) {
                        TransferBatchState.QUEUED
                    } else {
                        TransferBatchState.PREPARING
                    },
                    System.currentTimeMillis(),
                )
                pendingIncomingOffer = null
                pendingReselectionBatchId = null
                activeBatchId = batchId
                check(sendDecision(batchId, true)) { "waiting_network" }
                onChanged(Snapshot(true, batchId, totalBytes = batch.totalBytes))
            }.onFailure { error ->
                val batchId = offer?.batchId ?: reselectBatchId
                val total = batchId?.let(store::loadBatch)?.totalBytes ?: 0
                onChanged(Snapshot(true, batchId, totalBytes = total, error = error.message ?: "target_failed"))
            }
        }
    }

    fun rejectIncoming() {
        val offer = pendingIncomingOffer ?: return
        pendingIncomingOffer = null
        worker.execute {
            runCatching {
                store.updateBatchState(offer.batchId, TransferBatchState.CANCELED, System.currentTimeMillis())
                sendDecision(offer.batchId, false)
            }
            onChanged(Snapshot(false))
        }
    }

    fun resumeRecoverable() {
        worker.execute {
            cleanupExpired()
            store.recoverableBatches()
                .forEach { batch ->
                    if (batch.direction == TransferDirection.PHONE_TO_COMPUTER) {
                        offer(batch)
                    } else if (batch.targetTreeUri != null) {
                        if (!sendDecision(batch.batchId, true)) {
                            onChanged(Snapshot(true, batch.batchId, totalBytes = batch.totalBytes, error = "waiting_network"))
                        }
                    }
                }
        }
    }

    private fun cleanupExpired() {
        val cutoff = System.currentTimeMillis() - 7L * 24 * 60 * 60 * 1000
        store.batchesOlderThan(cutoff).forEach { batch ->
            var ordinal = 0L
            while (true) {
                val page = store.entriesPage(batch.batchId, ordinal, 256)
                if (page.isEmpty()) break
                page.forEach { entry ->
                    ordinal = entry.manifest.ordinal + 1
                    if (entry.state != TransferEntryState.COMPLETED) {
                        entry.targetUri?.let { value ->
                            runCatching {
                                DocumentsContract.deleteDocument(context.contentResolver, Uri.parse(value))
                            }
                        }
                    }
                }
            }
            store.deleteBatch(batch.batchId)
        }
    }

    private fun cleanupPartialTargets(batchId: String) {
        var ordinal = 0L
        while (true) {
            val page = store.entriesPage(batchId, ordinal, 256)
            if (page.isEmpty()) break
            page.forEach { entry ->
                ordinal = entry.manifest.ordinal + 1
                if (entry.state != TransferEntryState.COMPLETED) {
                    entry.targetUri?.let { value ->
                        runCatching {
                            DocumentsContract.deleteDocument(context.contentResolver, Uri.parse(value))
                        }
                    }
                }
            }
        }
    }

    private fun create(prepare: () -> PreparedTransfer) {
        if (activeBatchId != null || pendingIncomingOffer != null) return
        val current = binding ?: run {
            onChanged(Snapshot(false, error = "computer_not_connected"))
            return
        }
        onChanged(Snapshot(true, totalBytes = 0))
        worker.execute {
            var createdBatchId: String? = null
            runCatching {
                val prepared = prepare()
                val now = System.currentTimeMillis()
                val batch = FileTransferBatch(
                    batchId = UUID.randomUUID().toString(),
                    pcId = current.pcId,
                    direction = TransferDirection.PHONE_TO_COMPUTER,
                    state = TransferBatchState.QUEUED,
                    createdAt = now,
                    updatedAt = now,
                    fileCount = prepared.fileCount,
                    directoryCount = prepared.directoryCount,
                    totalBytes = prepared.totalBytes,
                    targetTreeUri = null,
                )
                store.createBatch(batch)
                createdBatchId = batch.batchId
                activeBatchId = batch.batchId
                var actualFiles = 0L
                var actualDirectories = 0L
                var actualBytes = 0L
                prepared.entries.chunked(256).forEach { page ->
                    page.forEach { entry ->
                        if (entry.manifest.kind == TransferEntryKind.FILE) actualFiles++ else actualDirectories++
                        actualBytes = Math.addExact(actualBytes, entry.manifest.size)
                    }
                    store.appendManifestPage(batch.batchId, page)
                }
                check(
                    actualFiles == batch.fileCount &&
                        actualDirectories == batch.directoryCount &&
                        actualBytes == batch.totalBytes,
                ) { "source changed while preparing transfer" }
                onChanged(Snapshot(true, batch.batchId, totalBytes = batch.totalBytes))
                offer(batch, prepared.roots)
            }.onFailure { error ->
                createdBatchId?.let { batchId ->
                    runCatching { store.deleteBatch(batchId) }
                    if (activeBatchId == batchId) activeBatchId = null
                }
                onChanged(Snapshot(false, error = error.message ?: "prepare_failed"))
            }
        }
    }

    private fun offer(batch: FileTransferBatch, roots: List<String> = roots(batch.batchId)) {
        val sent = sendOffer(
            FileOfferMessage(
                phoneId = phoneId,
                batchId = batch.batchId,
                direction = batch.direction,
                fileCount = batch.fileCount,
                directoryCount = batch.directoryCount,
                totalBytes = batch.totalBytes,
                roots = roots,
            ),
        )
        if (!sent) onChanged(Snapshot(true, batch.batchId, totalBytes = batch.totalBytes, error = "waiting_network"))
    }

    private fun upload(binding: ComputerBinding, ticket: FileTicketMessage) {
        val batch = store.loadBatch(ticket.batchId) ?: return
        if (batch.pcId != binding.pcId || batch.direction != TransferDirection.PHONE_TO_COMPUTER) return
        activeBatchId = batch.batchId
        runCatching {
            store.updateBatchState(batch.batchId, TransferBatchState.PREPARING, System.currentTimeMillis())
            withActiveClient(binding) { client ->
                client.uploadManifest(ticket, manifestSequence(batch.batchId)).use { response ->
                    check(response.isSuccessful) { "manifest upload failed: ${response.code}" }
                }
                reportStoredFailures(client, ticket, batch.batchId)
                store.updateBatchState(batch.batchId, TransferBatchState.TRANSFERRING, System.currentTimeMillis())
                val transferred = AtomicLong(transferredBytes(batch.batchId))
                var ordinal = 0L
                while (true) {
                    val page = store.entriesPage(batch.batchId, ordinal, 256)
                    if (page.isEmpty()) break
                    ordinal = page.last().manifest.ordinal + 1
                    runParallel(page.filter { entry ->
                        entry.manifest.kind == TransferEntryKind.FILE &&
                            entry.state != TransferEntryState.COMPLETED &&
                            entry.state != TransferEntryState.FAILED
                    }) { entry ->
                        try {
                            checkNotCanceled(batch.batchId)
                            val offset = client.queryCommittedOffset(ticket, entry.manifest)
                            check(offset in 0..entry.manifest.size) { "invalid server offset" }
                            transferred.addAndGet(offset - entry.committedOffset)
                            store.updateEntryProgress(
                                batch.batchId,
                                entry.manifest.ordinal,
                                TransferEntryState.TRANSFERRING,
                                offset,
                            )
                            var committed = offset
                            var completedFile = false
                            while (!completedFile) {
                                checkNotCanceled(batch.batchId)
                                var reported = 0L
                                var pendingUiBytes = 0L
                                var lastUiUpdate = System.nanoTime()
                                val receipt = client.uploadFile(
                                    context.contentResolver,
                                    ticket,
                                    entry,
                                    committed,
                                ) { bytes ->
                                    reported += bytes
                                    pendingUiBytes += bytes
                                    val now = System.nanoTime()
                                    if (pendingUiBytes >= 8L * 1024 * 1024 ||
                                        now - lastUiUpdate >= 250L * 1_000_000
                                    ) {
                                        val current = transferred.addAndGet(pendingUiBytes)
                                        pendingUiBytes = 0
                                        lastUiUpdate = now
                                        onChanged(Snapshot(true, batch.batchId, current, batch.totalBytes))
                                    }
                                }
                                if (pendingUiBytes > 0) {
                                    transferred.addAndGet(pendingUiBytes)
                                }
                                check(receipt.committedOffset > committed || receipt.completed) {
                                    "file upload made no progress"
                                }
                                val previous = committed
                                committed = receipt.committedOffset
                                transferred.addAndGet(committed - previous - reported)
                                completedFile = receipt.completed
                                store.updateEntryProgress(
                                    batch.batchId,
                                    entry.manifest.ordinal,
                                    if (completedFile) TransferEntryState.COMPLETED else TransferEntryState.TRANSFERRING,
                                    committed,
                                )
                                onChanged(Snapshot(true, batch.batchId, transferred.get(), batch.totalBytes))
                            }
                        } catch (error: Throwable) {
                            val code = permanentEntryFailureCode(error, batch.direction) ?: throw error
                            val current = store.entry(batch.batchId, entry.manifest.ordinal) ?: entry
                            store.updateEntryProgress(
                                batch.batchId,
                                entry.manifest.ordinal,
                                TransferEntryState.FAILED,
                                current.committedOffset,
                                errorCode = code,
                            )
                            client.reportFileFailure(ticket, entry.manifest, code)
                        }
                    }
                }
                client.completeBatch(ticket)
            }
            val partial = store.failedEntryCount(batch.batchId) > 0
            store.updateBatchState(
                batch.batchId,
                if (partial) TransferBatchState.PARTIAL_FAILURE else TransferBatchState.COMPLETED,
                System.currentTimeMillis(),
            )
            activeBatchId = null
            onChanged(
                Snapshot(
                    false,
                    batch.batchId,
                    completedBytes(batch.batchId),
                    batch.totalBytes,
                    if (partial) "partial_failure" else null,
                    failedPaths = failedPaths(batch.batchId),
                ),
            )
        }.onFailure { error ->
            if (error is CancellationException || batch.batchId in canceledBatches) return@onFailure
            handleTransferFailure(batch, error)
        }
    }

    private fun download(binding: ComputerBinding, ticket: FileTicketMessage) {
        val batch = store.loadBatch(ticket.batchId) ?: return
        if (batch.pcId != binding.pcId || batch.direction != TransferDirection.COMPUTER_TO_PHONE) return
        activeBatchId = batch.batchId
        runCatching {
            val treeUri = Uri.parse(requireNotNull(batch.targetTreeUri) { "save location is unavailable" })
            if (batch.state == TransferBatchState.QUEUED || batch.state == TransferBatchState.WAITING_NETWORK) {
                store.updateBatchState(batch.batchId, TransferBatchState.PREPARING, System.currentTimeMillis())
            }
            withActiveClient(binding) { client ->
                downloadManifest(client, ticket, batch)
                AndroidTargetStore(context.contentResolver, store).prepare(batch.batchId, treeUri)
                reportStoredFailures(client, ticket, batch.batchId)
                store.updateBatchState(batch.batchId, TransferBatchState.TRANSFERRING, System.currentTimeMillis())
                val transferred = AtomicLong(transferredBytes(batch.batchId))
                var ordinal = 0L
                while (true) {
                    val page = store.entriesPage(batch.batchId, ordinal, 256)
                    if (page.isEmpty()) break
                    ordinal = page.last().manifest.ordinal + 1
                    runParallel(page.filter { stored ->
                        stored.manifest.kind == TransferEntryKind.FILE &&
                            stored.state != TransferEntryState.COMPLETED &&
                            stored.state != TransferEntryState.FAILED
                    }) { stored ->
                        try {
                            checkNotCanceled(batch.batchId)
                            downloadEntry(client, ticket, batch.batchId, stored) { bytes ->
                                val current = transferred.addAndGet(bytes)
                                onChanged(Snapshot(true, batch.batchId, current, batch.totalBytes))
                            }
                        } catch (error: Throwable) {
                            val code = permanentEntryFailureCode(error, batch.direction) ?: throw error
                            val current = store.entry(batch.batchId, stored.manifest.ordinal) ?: stored
                            store.updateEntryProgress(
                                batch.batchId,
                                stored.manifest.ordinal,
                                TransferEntryState.FAILED,
                                current.committedOffset,
                                errorCode = code,
                            )
                            client.reportFileFailure(ticket, stored.manifest, code)
                        }
                    }
                }
                client.completeBatch(ticket)
            }
            val partial = store.failedEntryCount(batch.batchId) > 0
            store.updateBatchState(
                batch.batchId,
                if (partial) TransferBatchState.PARTIAL_FAILURE else TransferBatchState.COMPLETED,
                System.currentTimeMillis(),
            )
            activeBatchId = null
            onChanged(
                Snapshot(
                    false,
                    batch.batchId,
                    completedBytes(batch.batchId),
                    batch.totalBytes,
                    if (partial) "partial_failure" else null,
                    failedPaths = failedPaths(batch.batchId),
                ),
            )
        }.onFailure { error ->
            if (error is CancellationException || batch.batchId in canceledBatches) return@onFailure
            handleTransferFailure(batch, error)
        }
    }

    private fun downloadManifest(
        client: FileTransferHttpClient,
        ticket: FileTicketMessage,
        batch: FileTransferBatch,
    ) {
        val expected = batch.fileCount + batch.directoryCount
        var next = store.manifestEntryCount(batch.batchId)
        check(next <= expected) { "local manifest is invalid" }
        while (next < expected) {
            val page = client.downloadManifestPage(ticket, next)
            check(page.totalEntries == expected && page.entries.isNotEmpty()) { "manifest count changed" }
            check(page.entries.first().ordinal == next && page.nextOrdinal > next) { "manifest is not contiguous" }
            store.appendManifestPage(
                batch.batchId,
                page.entries.map { StoredTransferEntry(it, null, null) },
            )
            next = page.nextOrdinal
        }
        var files = 0L
        var directories = 0L
        var bytes = 0L
        var ordinal = 0L
        while (true) {
            val page = store.entriesPage(batch.batchId, ordinal, 256)
            if (page.isEmpty()) break
            page.forEach {
                ordinal = it.manifest.ordinal + 1
                if (it.manifest.kind == TransferEntryKind.FILE) files++ else directories++
                bytes = Math.addExact(bytes, it.manifest.size)
            }
        }
        check(files == batch.fileCount && directories == batch.directoryCount && bytes == batch.totalBytes) {
            "manifest summary mismatch"
        }
    }

    private fun downloadEntry(
        client: FileTransferHttpClient,
        ticket: FileTicketMessage,
        batchId: String,
        original: StoredTransferEntry,
        onProgress: (Long) -> Unit,
    ) {
        val resolver = context.contentResolver
        val targetStore = AndroidTargetStore(resolver, store)
        var stored = store.entry(batchId, original.manifest.ordinal) ?: error("entry disappeared")
        val target = Uri.parse(requireNotNull(stored.targetUri) { "target file is unavailable" })
        val parent = Uri.parse(requireNotNull(stored.targetParentUri) { "target directory is unavailable" })
        var offset = stored.committedOffset
        val digest = MessageDigest.getInstance("SHA-256")
        resolver.openFileDescriptor(target, "rw").use { descriptor ->
            requireNotNull(descriptor) { "target file cannot be opened" }
            FileOutputStream(descriptor.fileDescriptor).channel.use { channel ->
                val actual = channel.size()
                if (actual < offset) offset = 0
                if (actual != offset) channel.truncate(offset)
                if (offset > 0) {
                    resolver.openInputStream(target).use { input ->
                        requireNotNull(input)
                        val buffer = ByteArray(512 * 1024)
                        var remaining = offset
                        while (remaining > 0) {
                            val count = input.read(buffer, 0, minOf(remaining, buffer.size.toLong()).toInt())
                            if (count < 0) throw EOFException("partial file became shorter")
                            digest.update(buffer, 0, count)
                            remaining -= count
                        }
                    }
                }
                store.updateEntryProgress(batchId, stored.manifest.ordinal, TransferEntryState.TRANSFERRING, offset)
                while (offset < stored.manifest.size) {
                    checkNotCanceled(batchId)
                    val bytes = client.downloadChunk(ticket, stored.manifest, offset)
                    channel.position(offset)
                    val buffer = ByteBuffer.wrap(bytes)
                    while (buffer.hasRemaining()) channel.write(buffer)
                    digest.update(bytes)
                    offset += bytes.size
                    onProgress(bytes.size.toLong())
                    channel.force(false)
                    store.updateEntryProgress(batchId, stored.manifest.ordinal, TransferEntryState.TRANSFERRING, offset)
                }
            }
        }
        val hash = digest.digest().joinToString("") { "%02x".format(it) }
        client.verifyDownloadedFile(ticket, stored.manifest, hash)
        val finalUri = targetStore.commit(
            target,
            parent,
            requireNotNull(stored.targetName),
            stored.manifest.mimeType,
        )
        store.updateEntryProgress(
            batchId,
            stored.manifest.ordinal,
            TransferEntryState.COMPLETED,
            stored.manifest.size,
            targetUri = finalUri.toString(),
        )
    }

    private fun completedBytes(batchId: String): Long {
        var completed = 0L
        var ordinal = 0L
        while (true) {
            val page = store.entriesPage(batchId, ordinal, 256)
            if (page.isEmpty()) break
            page.forEach {
                ordinal = it.manifest.ordinal + 1
                if (it.state == TransferEntryState.COMPLETED) completed += it.manifest.size
            }
        }
        return completed
    }

    private fun failedPaths(batchId: String): List<String> =
        store.failedEntries(batchId).map { it.manifest.relativePath }

    private fun reportStoredFailures(
        client: FileTransferHttpClient,
        ticket: FileTicketMessage,
        batchId: String,
    ) {
        var ordinal = 0L
        while (true) {
            val page = store.entriesPage(batchId, ordinal, 256)
            if (page.isEmpty()) break
            page.filter { it.state == TransferEntryState.FAILED }.forEach { entry ->
                client.reportFileFailure(
                    ticket,
                    entry.manifest,
                    entry.errorCode ?: "transfer_failed",
                )
            }
            ordinal = page.last().manifest.ordinal + 1
        }
    }

    private fun transferredBytes(batchId: String): Long {
        var transferred = 0L
        var ordinal = 0L
        while (true) {
            val page = store.entriesPage(batchId, ordinal, 256)
            if (page.isEmpty()) break
            page.forEach {
                ordinal = it.manifest.ordinal + 1
                transferred += it.committedOffset
            }
        }
        return transferred
    }

    private fun <T> runParallel(items: List<T>, action: (T) -> Unit) {
        if (items.isEmpty()) return
        val executor = Executors.newFixedThreadPool(parallelism().coerceIn(1, 2)) { runnable ->
            Thread(runnable, "flowtype-file-worker").apply { isDaemon = true }
        }
        try {
            items.map { item -> executor.submit { action(item) } }.forEach { future ->
                try {
                    future.get()
                } catch (error: ExecutionException) {
                    throw error.cause ?: error
                }
            }
        } finally {
            executor.shutdownNow()
        }
    }

    private fun <T> withActiveClient(
        binding: ComputerBinding,
        action: (FileTransferHttpClient) -> T,
    ): T {
        val client = FileTransferHttpClient(binding)
        activeHttpClient = client
        try {
            return client.use(action)
        } finally {
            if (activeHttpClient === client) activeHttpClient = null
        }
    }

    private fun checkNotCanceled(batchId: String) {
        if (batchId in canceledBatches) throw CancellationException("transfer canceled")
    }

    private fun handleTransferFailure(batch: FileTransferBatch, error: Throwable) {
        Log.e("FlowTypeFiles", "batch ${batch.batchId} transfer failed", error)
        val message = error.message ?: "transfer_failed"
        val permanent = listOf(
            "source_changed",
            "integrity_failed",
            "manifest summary mismatch",
            "batch id conflict",
            "invalid manifest",
        ).any(message::contains)
        val needsReselection = error is SecurityException ||
            error is FileNotFoundException ||
            listOf("target file", "target directory", "save location").any(message::contains)
        if (needsReselection && batch.direction == TransferDirection.COMPUTER_TO_PHONE) {
            runCatching {
                store.updateBatchState(
                    batch.batchId,
                    TransferBatchState.NEEDS_RESELECTION,
                    System.currentTimeMillis(),
                )
            }
            activeBatchId = null
            pendingReselectionBatchId = batch.batchId
            onChanged(
                Snapshot(
                    false,
                    batch.batchId,
                    completedBytes(batch.batchId),
                    batch.totalBytes,
                    message,
                    needsReselection = true,
                ),
            )
            return
        }
        if (permanent) {
            runCatching {
                store.updateBatchState(
                    batch.batchId,
                    TransferBatchState.PARTIAL_FAILURE,
                    System.currentTimeMillis(),
                )
            }
            activeBatchId = null
            onChanged(
                Snapshot(false, batch.batchId, completedBytes(batch.batchId), batch.totalBytes, message),
            )
            return
        }
        runCatching {
            store.updateBatchState(
                batch.batchId,
                TransferBatchState.WAITING_NETWORK,
                System.currentTimeMillis(),
            )
        }
        onChanged(
            Snapshot(true, batch.batchId, completedBytes(batch.batchId), batch.totalBytes, message),
        )
        worker.schedule({
            if (batch.batchId in canceledBatches ||
                store.loadBatch(batch.batchId)?.state?.terminal != false
            ) return@schedule
            if (batch.direction == TransferDirection.PHONE_TO_COMPUTER) {
                offer(batch)
            } else {
                sendDecision(batch.batchId, true)
            }
        }, 3, TimeUnit.SECONDS)
    }

    private fun manifestSequence(batchId: String): Sequence<app.flowtype.protocol.TransferManifestEntry> = sequence {
        var ordinal = 0L
        while (true) {
            val page = store.entriesPage(batchId, ordinal, 256)
            if (page.isEmpty()) break
            page.forEach { yield(it.manifest) }
            ordinal = page.last().manifest.ordinal + 1
        }
    }

    private fun roots(batchId: String): List<String> = store.entriesPage(batchId, 0, 256)
        .asSequence()
        .map { it.manifest.relativePath.substringBefore('/') }
        .distinct()
        .take(8)
        .toList()

    override fun close() {
        worker.shutdownNow()
    }
}

internal fun permanentEntryFailureCode(
    error: Throwable,
    direction: TransferDirection,
): String? {
    if (direction == TransferDirection.PHONE_TO_COMPUTER &&
        (error is SecurityException || error is FileNotFoundException)
    ) {
        return "source_unavailable"
    }
    val message = error.message.orEmpty().lowercase()
    return when {
        "source_changed" in message || "source became" in message ||
            "remote file size changed" in message -> "source_changed"
        "source_unavailable" in message || "source uri" in message -> "source_unavailable"
        "integrity_failed" in message || "integrity check failed" in message -> "integrity_failed"
        "target_exists" in message -> "target_exists"
        else -> null
    }
}
