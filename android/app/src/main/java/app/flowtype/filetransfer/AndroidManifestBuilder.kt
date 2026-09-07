package app.flowtype.filetransfer

import android.content.ContentResolver
import android.net.Uri
import android.provider.DocumentsContract
import android.provider.OpenableColumns
import app.flowtype.data.StoredTransferEntry
import app.flowtype.protocol.TransferEntryKind
import app.flowtype.protocol.TransferManifestEntry
import java.util.UUID

data class PreparedTransfer(
    val entries: List<StoredTransferEntry>,
    val roots: List<String>,
    val fileCount: Long,
    val directoryCount: Long,
    val totalBytes: Long,
)

object AndroidManifestBuilder {
    fun files(resolver: ContentResolver, uris: List<Uri>): PreparedTransfer {
        require(uris.isNotEmpty())
        val entries = uris.mapIndexed { index, uri ->
            val metadata = metadata(resolver, uri)
            StoredTransferEntry(
                manifest = TransferManifestEntry(
                    ordinal = index.toLong(),
                    entryId = UUID.randomUUID().toString(),
                    relativePath = safeSegment(metadata.name),
                    kind = TransferEntryKind.FILE,
                    size = metadata.size,
                    modifiedAtEpochMillis = metadata.modifiedAt,
                    mimeType = metadata.mimeType,
                    sha256 = null,
                ),
                sourceUri = uri.toString(),
                targetUri = null,
            )
        }
        return summarize(entries)
    }

    fun directory(resolver: ContentResolver, treeUri: Uri): PreparedTransfer {
        val rootDocument = DocumentsContract.buildDocumentUriUsingTree(
            treeUri,
            DocumentsContract.getTreeDocumentId(treeUri),
        )
        val root = documentMetadata(resolver, rootDocument)
        val entries = mutableListOf<StoredTransferEntry>()
        appendDirectory(resolver, treeUri, rootDocument, safeSegment(root.name), entries)
        return summarize(entries)
    }

    private fun appendDirectory(
        resolver: ContentResolver,
        treeUri: Uri,
        documentUri: Uri,
        relativePath: String,
        output: MutableList<StoredTransferEntry>,
    ) {
        output += StoredTransferEntry(
            manifest = TransferManifestEntry(
                ordinal = output.size.toLong(),
                entryId = UUID.randomUUID().toString(),
                relativePath = relativePath,
                kind = TransferEntryKind.DIRECTORY,
                size = 0,
                modifiedAtEpochMillis = null,
                mimeType = null,
                sha256 = null,
            ),
            sourceUri = documentUri.toString(),
            targetUri = null,
        )
        val documentId = DocumentsContract.getDocumentId(documentUri)
        val children = DocumentsContract.buildChildDocumentsUriUsingTree(treeUri, documentId)
        resolver.query(
            children,
            arrayOf(
                DocumentsContract.Document.COLUMN_DOCUMENT_ID,
                DocumentsContract.Document.COLUMN_DISPLAY_NAME,
                DocumentsContract.Document.COLUMN_MIME_TYPE,
                DocumentsContract.Document.COLUMN_SIZE,
                DocumentsContract.Document.COLUMN_LAST_MODIFIED,
            ),
            null,
            null,
            null,
        )?.use { cursor ->
            while (cursor.moveToNext()) {
                val childId = cursor.getString(0)
                val name = safeSegment(cursor.getString(1))
                val mime = cursor.getString(2)
                val child = DocumentsContract.buildDocumentUriUsingTree(treeUri, childId)
                val childPath = "$relativePath/$name"
                if (mime == DocumentsContract.Document.MIME_TYPE_DIR) {
                    appendDirectory(resolver, treeUri, child, childPath, output)
                } else {
                    val size = if (cursor.isNull(3)) 0 else cursor.getLong(3)
                    require(size >= 0) { "file size is unavailable" }
                    output += StoredTransferEntry(
                        manifest = TransferManifestEntry(
                            ordinal = output.size.toLong(),
                            entryId = UUID.randomUUID().toString(),
                            relativePath = childPath,
                            kind = TransferEntryKind.FILE,
                            size = size,
                            modifiedAtEpochMillis = if (cursor.isNull(4)) null else cursor.getLong(4),
                            mimeType = mime?.takeUnless { it.isBlank() },
                            sha256 = null,
                        ),
                        sourceUri = child.toString(),
                        targetUri = null,
                    )
                }
            }
        } ?: error("directory cannot be read")
    }

    private fun metadata(resolver: ContentResolver, uri: Uri): Metadata {
        resolver.query(
            uri,
            arrayOf(OpenableColumns.DISPLAY_NAME, OpenableColumns.SIZE),
            null,
            null,
            null,
        )?.use { cursor ->
            check(cursor.moveToFirst()) { "file metadata is unavailable" }
            val name = cursor.getString(0)
            val size = if (cursor.isNull(1)) resolver.openAssetFileDescriptor(uri, "r")?.use { it.length } else cursor.getLong(1)
            require(size != null && size >= 0) { "file size is unavailable" }
            return Metadata(name, size, null, resolver.getType(uri))
        }
        error("file metadata is unavailable")
    }

    private fun documentMetadata(resolver: ContentResolver, uri: Uri): Metadata {
        resolver.query(
            uri,
            arrayOf(
                DocumentsContract.Document.COLUMN_DISPLAY_NAME,
                DocumentsContract.Document.COLUMN_SIZE,
                DocumentsContract.Document.COLUMN_LAST_MODIFIED,
                DocumentsContract.Document.COLUMN_MIME_TYPE,
            ),
            null,
            null,
            null,
        )?.use { cursor ->
            check(cursor.moveToFirst()) { "document metadata is unavailable" }
            return Metadata(
                cursor.getString(0),
                if (cursor.isNull(1)) 0 else cursor.getLong(1),
                if (cursor.isNull(2)) null else cursor.getLong(2),
                if (cursor.isNull(3)) null else cursor.getString(3),
            )
        }
        error("document metadata is unavailable")
    }

    private fun summarize(entries: List<StoredTransferEntry>): PreparedTransfer {
        require(entries.isNotEmpty())
        entries.forEach(StoredTransferEntry::validate)
        val roots = entries.asSequence()
            .map { it.manifest.relativePath.substringBefore('/') }
            .distinct()
            .take(8)
            .toList()
        return PreparedTransfer(
            entries = entries,
            roots = roots,
            fileCount = entries.count { it.manifest.kind == TransferEntryKind.FILE }.toLong(),
            directoryCount = entries.count { it.manifest.kind == TransferEntryKind.DIRECTORY }.toLong(),
            totalBytes = entries.asSequence().sumOf { it.manifest.size },
        )
    }

    private fun safeSegment(value: String): String = value
        .replace('/', '_')
        .replace('\\', '_')
        .trim()
        .takeIf { it.isNotEmpty() && it != "." && it != ".." }
        ?: "未命名"

    private data class Metadata(
        val name: String,
        val size: Long,
        val modifiedAt: Long?,
        val mimeType: String?,
    )
}
