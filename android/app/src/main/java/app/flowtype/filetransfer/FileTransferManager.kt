package app.flowtype.filetransfer

import android.content.Context
import android.net.Uri
import app.flowtype.data.FileTransferBatch
import app.flowtype.data.FileTransferStore
import app.flowtype.data.StoredTransferEntry
import app.flowtype.network.FileTransferHttpClient
import app.flowtype.pairing.ComputerBinding
import app.flowtype.protocol.FileOfferMessage
import app.flowtype.protocol.FileTicketMessage
import app.flowtype.protocol.TransferBatchState
import app.flowtype.protocol.TransferDirection
import app.flowtype.protocol.TransferEntryKind
import app.flowtype.protocol.TransferEntryState
import java.io.Closeable
import java.util.UUID
import java.util.concurrent.Executors

class FileTransferManager(
    private val context: Context,
    private val store: FileTransferStore,
    private val phoneId: String,
    private val sendOffer: (FileOfferMessage) -> Boolean,
    private val onChanged: (Snapshot) -> Unit,
) : Closeable {
    data class Snapshot(
        val active: Boolean,
        val batchId: String? = null,
        val completedBytes: Long = 0,
        val totalBytes: Long = 0,
        val error: String? = null,
    )

    private val worker = Executors.newSingleThreadExecutor { runnable ->
        Thread(runnable, "flowtype-file-transfer").apply { isDaemon = true }
    }
    @Volatile private var binding: ComputerBinding? = null

    fun setBinding(value: ComputerBinding?) {
        binding = value
    }

    fun createFiles(uris: List<Uri>) = create { AndroidManifestBuilder.files(context.contentResolver, uris) }

    fun createDirectory(uri: Uri) = create { AndroidManifestBuilder.directory(context.contentResolver, uri) }

    fun onTicket(ticket: FileTicketMessage) {
        val current = binding ?: return
        worker.execute { transfer(current, ticket) }
    }

    fun resumeRecoverable() {
        worker.execute {
            store.recoverableBatches()
                .filter { it.direction == TransferDirection.PHONE_TO_COMPUTER }
                .forEach { batch -> offer(batch) }
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

    private fun transfer(binding: ComputerBinding, ticket: FileTicketMessage) {
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
