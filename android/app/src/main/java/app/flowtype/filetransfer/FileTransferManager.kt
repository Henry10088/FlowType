package app.flowtype.filetransfer

import android.content.Context
import android.net.Uri
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
import java.nio.ByteBuffer
import java.security.MessageDigest
import java.util.UUID
import java.util.concurrent.Executors

class FileTransferManager(
    private val context: Context,
    private val store: FileTransferStore,
    private val phoneId: String,
    private val sendOffer: (FileOfferMessage) -> Boolean,
    private val sendDecision: (String, Boolean) -> Boolean,
    private val onChanged: (Snapshot) -> Unit,
) : Closeable {
    data class Snapshot(
        val active: Boolean,
        val batchId: String? = null,
        val completedBytes: Long = 0,
        val totalBytes: Long = 0,
        val error: String? = null,
        val incomingOffer: FileOfferMessage? = null,
    )

    private val worker = Executors.newSingleThreadExecutor { runnable ->
        Thread(runnable, "flowtype-file-transfer").apply { isDaemon = true }
    }
    @Volatile private var binding: ComputerBinding? = null
    @Volatile private var pendingIncomingOffer: FileOfferMessage? = null

    fun setBinding(value: ComputerBinding?) {
        binding = value
    }

    fun createFiles(uris: List<Uri>) = create { AndroidManifestBuilder.files(context.contentResolver, uris) }

    fun createDirectory(uri: Uri) = create { AndroidManifestBuilder.directory(context.contentResolver, uri) }

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
        val offer = pendingIncomingOffer ?: return
        worker.execute {
            runCatching {
                store.setTargetTree(offer.batchId, treeUri.toString(), System.currentTimeMillis())
                store.updateBatchState(offer.batchId, TransferBatchState.QUEUED, System.currentTimeMillis())
                pendingIncomingOffer = null
                check(sendDecision(offer.batchId, true)) { "waiting_network" }
                onChanged(Snapshot(true, offer.batchId, totalBytes = offer.totalBytes))
            }.onFailure { error ->
                onChanged(Snapshot(true, offer.batchId, totalBytes = offer.totalBytes, error = error.message ?: "target_failed"))
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

    private fun create(prepare: () -> PreparedTransfer) {
        val current = binding ?: run {
            onChanged(Snapshot(false, error = "computer_not_connected"))
            return
        }
        worker.execute {
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
                prepared.entries.chunked(256).forEach { store.appendManifestPage(batch.batchId, it) }
                onChanged(Snapshot(true, batch.batchId, totalBytes = batch.totalBytes))
                offer(batch, prepared.roots)
            }.onFailure { error ->
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
        runCatching {
            store.updateBatchState(batch.batchId, TransferBatchState.PREPARING, System.currentTimeMillis())
            FileTransferHttpClient(binding).use { client ->
                client.uploadManifest(ticket, manifestSequence(batch.batchId)).use { response ->
                    check(response.isSuccessful) { "manifest upload failed: ${response.code}" }
                }
                store.updateBatchState(batch.batchId, TransferBatchState.TRANSFERRING, System.currentTimeMillis())
                var completed = 0L
                var ordinal = 0L
                while (true) {
                    val page = store.entriesPage(batch.batchId, ordinal, 256)
                    if (page.isEmpty()) break
                    page.forEach { entry ->
                        ordinal = entry.manifest.ordinal + 1
                        if (entry.manifest.kind == TransferEntryKind.DIRECTORY) return@forEach
                        if (entry.state == TransferEntryState.COMPLETED) {
                            completed += entry.manifest.size
                            return@forEach
                        }
                        val offset = client.queryCommittedOffset(ticket, entry.manifest)
                        store.updateEntryProgress(
                            batch.batchId,
                            entry.manifest.ordinal,
                            TransferEntryState.TRANSFERRING,
                            offset,
                        )
                        val receipt = client.uploadFile(context.contentResolver, ticket, entry, offset)
                        store.updateEntryProgress(
                            batch.batchId,
                            entry.manifest.ordinal,
                            if (receipt.completed) TransferEntryState.COMPLETED else TransferEntryState.WAITING_NETWORK,
                            receipt.committedOffset,
                        )
                        check(receipt.completed) { "file upload interrupted" }
                        completed += entry.manifest.size
                        onChanged(Snapshot(true, batch.batchId, completed, batch.totalBytes))
                    }
                }
                client.completeBatch(ticket)
            }
            store.updateBatchState(batch.batchId, TransferBatchState.COMPLETED, System.currentTimeMillis())
            onChanged(Snapshot(false, batch.batchId, batch.totalBytes, batch.totalBytes))
        }.onFailure { error ->
            runCatching {
                store.updateBatchState(batch.batchId, TransferBatchState.WAITING_NETWORK, System.currentTimeMillis())
            }
            onChanged(Snapshot(true, batch.batchId, totalBytes = batch.totalBytes, error = error.message ?: "transfer_failed"))
        }
    }

    private fun download(binding: ComputerBinding, ticket: FileTicketMessage) {
        val batch = store.loadBatch(ticket.batchId) ?: return
        if (batch.pcId != binding.pcId || batch.direction != TransferDirection.COMPUTER_TO_PHONE) return
        runCatching {
            val treeUri = Uri.parse(requireNotNull(batch.targetTreeUri) { "save location is unavailable" })
            if (batch.state == TransferBatchState.QUEUED || batch.state == TransferBatchState.WAITING_NETWORK) {
                store.updateBatchState(batch.batchId, TransferBatchState.PREPARING, System.currentTimeMillis())
            }
            FileTransferHttpClient(binding).use { client ->
                downloadManifest(client, ticket, batch)
                AndroidTargetStore(context.contentResolver, store).prepare(batch.batchId, treeUri)
                store.updateBatchState(batch.batchId, TransferBatchState.TRANSFERRING, System.currentTimeMillis())
                var completed = completedBytes(batch.batchId)
                var ordinal = 0L
                while (true) {
                    val page = store.entriesPage(batch.batchId, ordinal, 256)
                    if (page.isEmpty()) break
                    page.forEach { stored ->
                        ordinal = stored.manifest.ordinal + 1
                        if (stored.manifest.kind == TransferEntryKind.DIRECTORY ||
                            stored.state == TransferEntryState.COMPLETED
                        ) return@forEach
                        downloadEntry(client, ticket, batch.batchId, stored)
                        completed += stored.manifest.size
                        onChanged(Snapshot(true, batch.batchId, completed, batch.totalBytes))
                    }
                }
                client.completeBatch(ticket)
            }
            store.updateBatchState(batch.batchId, TransferBatchState.COMPLETED, System.currentTimeMillis())
            onChanged(Snapshot(false, batch.batchId, batch.totalBytes, batch.totalBytes))
        }.onFailure { error ->
            runCatching {
                store.updateBatchState(batch.batchId, TransferBatchState.WAITING_NETWORK, System.currentTimeMillis())
            }
            onChanged(Snapshot(true, batch.batchId, completedBytes(batch.batchId), batch.totalBytes, error.message ?: "transfer_failed"))
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
                    val bytes = client.downloadChunk(ticket, stored.manifest, offset)
                    channel.position(offset)
                    val buffer = ByteBuffer.wrap(bytes)
                    while (buffer.hasRemaining()) channel.write(buffer)
                    digest.update(bytes)
                    offset += bytes.size
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
