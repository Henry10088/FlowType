package app.flowtype.filetransfer

import android.content.ContentResolver
import android.net.Uri
import android.provider.DocumentsContract
import app.flowtype.data.FileTransferStore
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

    fun commit(targetUri: Uri, parentUri: Uri, finalName: String, mimeType: String?): Uri {
        DocumentsContract.renameDocument(resolver, targetUri, finalName)?.let { return it }
        val finalUri = requireNotNull(
            DocumentsContract.createDocument(
                resolver,
                parentUri,
                mimeType ?: "application/octet-stream",
                finalName,
            ),
        ) { "cannot create final file" }
        resolver.openInputStream(targetUri).use { input ->
            requireNotNull(input)
            resolver.openOutputStream(finalUri, "w").use { output ->
                requireNotNull(output)
                input.copyTo(output, 512 * 1024)
            }
        }
        DocumentsContract.deleteDocument(resolver, targetUri)
        return finalUri
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
