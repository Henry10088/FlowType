package app.flowtype.filetransfer

import android.content.ContentResolver
import android.net.Uri
import android.provider.DocumentsContract
import app.flowtype.data.FileTransferStore
import app.flowtype.data.StoredTransferEntry
import java.io.FileNotFoundException
import java.security.MessageDigest
import app.flowtype.protocol.TransferEntryKind
import app.flowtype.protocol.TransferEntryState

class AndroidTargetStore(
    private val resolver: ContentResolver,
    private val store: FileTransferStore,
) {
    fun prepare(batchId: String, treeUri: Uri) {
        val root = DocumentsContract.buildDocumentUriUsingTree(
            treeUri,
            DocumentsContract.getTreeDocumentId(treeUri),
        )
        val directories = mutableMapOf("" to root)
        val reserved = mutableMapOf<String, MutableSet<String>>()
        var ordinal = 0L
        while (true) {
            val page = store.entriesPage(batchId, ordinal, 256)
            if (page.isEmpty()) break
            for (stored in page) {
                val entry = stored.manifest
                ordinal = entry.ordinal + 1
                // Completed files stay in the original location when changing the destination.
                if (entry.kind == TransferEntryKind.FILE && stored.state == TransferEntryState.COMPLETED) continue
                val parentPath = entry.relativePath.substringBeforeLast('/', "")
                val parent = directories[parentPath] ?: error("directory appears after its child")
                if (stored.targetUri != null) {
                    if (entry.kind == TransferEntryKind.DIRECTORY) {
                        directories[entry.relativePath] = Uri.parse(stored.targetUri)
                    } else {
                        stored.targetName?.let { name ->
                            reserved.getOrPut(parent.toString()) { childNames(parent) }.add(name.lowercase())
                        }
                    }
                    continue
                }
                val names = reserved.getOrPut(parent.toString()) { childNames(parent) }
                val desired = reserve(names, entry.relativePath.substringAfterLast('/'))
                if (entry.kind == TransferEntryKind.DIRECTORY) {
                    val created = requireNotNull(
                        DocumentsContract.createDocument(
                            resolver,
                            parent,
                            DocumentsContract.Document.MIME_TYPE_DIR,
                            desired,
                        ),
                    ) { "cannot create target directory" }
                    directories[entry.relativePath] = created
                    store.setEntryTarget(batchId, entry.ordinal, created.toString(), null, parent.toString())
                    store.updateEntryProgress(
                        batchId,
                        entry.ordinal,
                        TransferEntryState.COMPLETED,
                        0,
                    )
                } else {
                    val tempName = ".flowtype-${entry.entryId}.part"
                    val created = requireNotNull(
                        DocumentsContract.createDocument(
                            resolver,
                            parent,
                            entry.mimeType ?: "application/octet-stream",
                            tempName,
                        ),
                    ) { "cannot create target file" }
                    store.setEntryTarget(
                        batchId,
                        entry.ordinal,
                        created.toString(),
                        desired,
                        parent.toString(),
                    )
                }
            }
        }
    }

    fun recoverCommit(batchId: String, stored: StoredTransferEntry, checkCanceled: () -> Unit): Boolean {
        val hash = stored.commitSha256 ?: return false
        val parent = Uri.parse(requireNotNull(stored.targetParentUri))
        val candidate = stored.finalUri?.let(Uri::parse)
            ?: findChild(parent, requireNotNull(stored.targetName)) ?: return false
        if (!matches(candidate, stored.manifest.size, hash, checkCanceled)) {
            // A copy interrupted by process death belongs to us. Remove it
            // before retrying, but never delete an unrelated name collision.
            if (stored.finalUri != null) {
                runCatching { DocumentsContract.deleteDocument(resolver, candidate) }
                store.journalCommit(batchId, stored.manifest.ordinal, hash)
            }
            return false
        }
        store.updateEntryProgress(batchId, stored.manifest.ordinal, TransferEntryState.COMPLETED,
            stored.manifest.size, targetUri = candidate.toString())
        val old = stored.targetUri?.let(Uri::parse)
        if (old != null && old != candidate) runCatching { DocumentsContract.deleteDocument(resolver, old) }
        return true
    }

    @Synchronized
    fun commit(batchId: String, stored: StoredTransferEntry, hash: String, checkCanceled: () -> Unit): Uri {
        val targetUri = Uri.parse(requireNotNull(stored.targetUri))
        val parentUri = Uri.parse(requireNotNull(stored.targetParentUri))
        // A provider may create a colliding name between prepare and commit.
        val finalName = reserve(childNames(parentUri), requireNotNull(stored.targetName))
        store.setEntryTarget(batchId, stored.manifest.ordinal, targetUri.toString(), finalName, parentUri.toString())
        store.journalCommit(batchId, stored.manifest.ordinal, hash)
        checkCanceled()
        val renamed = try {
            DocumentsContract.renameDocument(resolver, targetUri, finalName)
        } catch (_: UnsupportedOperationException) { null }
        if (renamed != null) {
            store.updateEntryProgress(batchId, stored.manifest.ordinal, TransferEntryState.COMPLETED,
                stored.manifest.size, targetUri = renamed.toString())
            return renamed
        }
        val finalUri = requireNotNull(
            DocumentsContract.createDocument(
                resolver,
                parentUri,
                stored.manifest.mimeType ?: "application/octet-stream",
                finalName,
            ),
        ) { "cannot create final file" }
        try {
            store.journalCommit(batchId, stored.manifest.ordinal, hash, finalUri.toString())
            resolver.openInputStream(targetUri).use { input ->
                requireNotNull(input)
                resolver.openOutputStream(finalUri, "w").use { output ->
                    requireNotNull(output)
                    val buffer = ByteArray(512 * 1024)
                    while (true) {
                        checkCanceled()
                        val read = input.read(buffer)
                        if (read < 0) break
                        output.write(buffer, 0, read)
                    }
                }
            }
            store.updateEntryProgress(batchId, stored.manifest.ordinal, TransferEntryState.COMPLETED,
                stored.manifest.size, targetUri = finalUri.toString())
        } catch (error: Throwable) {
            runCatching { DocumentsContract.deleteDocument(resolver, finalUri) }
            throw error
        }
        runCatching { DocumentsContract.deleteDocument(resolver, targetUri) }
        return finalUri
    }

    private fun matches(uri: Uri, size: Long, expected: String, checkCanceled: () -> Unit): Boolean {
        return try {
            resolver.openInputStream(uri)?.use { input ->
                val digest = MessageDigest.getInstance("SHA-256")
                val buffer = ByteArray(512 * 1024)
                var count = 0L
                while (true) {
                    checkCanceled()
                    val read = input.read(buffer)
                    if (read < 0) break
                    count += read
                    if (count > size) return false
                    digest.update(buffer, 0, read)
                }
                count == size && digest.digest().joinToString("") { "%02x".format(it) } == expected
            } ?: false
        } catch (_: FileNotFoundException) { false }
    }

    private fun findChild(parent: Uri, name: String): Uri? {
        val children = DocumentsContract.buildChildDocumentsUriUsingTree(parent, DocumentsContract.getDocumentId(parent))
        resolver.query(children, arrayOf(DocumentsContract.Document.COLUMN_DOCUMENT_ID,
            DocumentsContract.Document.COLUMN_DISPLAY_NAME), null, null, null)?.use { cursor ->
            while (cursor.moveToNext()) if (cursor.getString(1) == name) {
                return DocumentsContract.buildDocumentUriUsingTree(parent, cursor.getString(0))
            }
        }
        return null
    }

    private fun childNames(parent: Uri): MutableSet<String> {
        val id = DocumentsContract.getDocumentId(parent)
        val children = DocumentsContract.buildChildDocumentsUriUsingTree(parent, id)
        val result = mutableSetOf<String>()
        resolver.query(
            children,
            arrayOf(DocumentsContract.Document.COLUMN_DISPLAY_NAME),
            null,
            null,
            null,
        )?.use { cursor ->
            while (cursor.moveToNext()) result += cursor.getString(0).lowercase()
        }
        return result
    }

    private fun reserve(names: MutableSet<String>, desired: String): String {
        if (names.add(desired.lowercase())) return desired
        val dot = desired.lastIndexOf('.').takeIf { it > 0 } ?: desired.length
        val stem = desired.substring(0, dot)
        val extension = desired.substring(dot)
        for (suffix in 1..Int.MAX_VALUE) {
            val candidate = "$stem ($suffix)$extension"
            if (names.add(candidate.lowercase())) return candidate
        }
        error("cannot reserve target name")
    }
}
