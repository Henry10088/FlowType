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
    val entries: Sequence<StoredTransferEntry>,
    val roots: List<String>,
    val fileCount: Long,
    val directoryCount: Long,
    val totalBytes: Long,
)

object AndroidManifestBuilder {
    fun files(resolver: ContentResolver, uris: List<Uri>): PreparedTransfer {
        require(uris.isNotEmpty())
        val entries = sequence {
            val names = mutableSetOf<String>()
            uris.forEachIndexed { index, uri ->
                val metadata = metadata(resolver, uri)
                val name = reserve(names, safeSegment(metadata.name))
                yield(
                    StoredTransferEntry(
                        manifest = TransferManifestEntry(
                            ordinal = index.toLong(),
                            entryId = stableEntryId(index.toLong(), uri),
                            relativePath = name,
                            kind = TransferEntryKind.FILE,
                            size = metadata.size,
                            modifiedAtEpochMillis = metadata.modifiedAt,
                            mimeType = metadata.mimeType,
                            sha256 = null,
                        ),
                        sourceUri = uri.toString(),
                        targetUri = null,
                    ),
                )
            }
        }
        return summarize(entries)
    }

    fun directory(resolver: ContentResolver, treeUri: Uri): PreparedTransfer {
        val rootDocument = DocumentsContract.buildDocumentUriUsingTree(
            treeUri,
            DocumentsContract.getTreeDocumentId(treeUri),
        )
        val root = documentMetadata(resolver, rootDocument)
        val entries = sequence {
            var ordinal = 0L
            appendDirectory(
                resolver,
                treeUri,
                rootDocument,
                safeSegment(root.name),
            ) { ordinal++ }
        }
        return summarize(entries)
    }

    private suspend fun SequenceScope<StoredTransferEntry>.appendDirectory(
        resolver: ContentResolver,
        treeUri: Uri,
        documentUri: Uri,
        relativePath: String,
        nextOrdinal: () -> Long,
    ) {
        val directoryOrdinal = nextOrdinal()
        yield(StoredTransferEntry(
            manifest = TransferManifestEntry(
                ordinal = directoryOrdinal,
                entryId = stableEntryId(directoryOrdinal, documentUri),
                relativePath = relativePath,
                kind = TransferEntryKind.DIRECTORY,
                size = 0,
                modifiedAtEpochMillis = null,
                mimeType = null,
                sha256 = null,
            ),
            sourceUri = documentUri.toString(),
            targetUri = null,
        ))
        val documentId = DocumentsContract.getDocumentId(documentUri)
        val children = DocumentsContract.buildChildDocumentsUriUsingTree(treeUri, documentId)
        val names = mutableSetOf<String>()
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
                val name = reserve(names, safeSegment(cursor.getString(1)))
                val mime = cursor.getString(2)
                val child = DocumentsContract.buildDocumentUriUsingTree(treeUri, childId)
                val childPath = "$relativePath/$name"
                if (mime == DocumentsContract.Document.MIME_TYPE_DIR) {
                    appendDirectory(resolver, treeUri, child, childPath, nextOrdinal)
                } else {
                    val size = if (cursor.isNull(3)) {
                        resolver.openAssetFileDescriptor(child, "r")?.use { it.length } ?: -1
                    } else cursor.getLong(3)
                    require(size >= 0) { "file size is unavailable" }
                    val fileOrdinal = nextOrdinal()
                    yield(StoredTransferEntry(
                        manifest = TransferManifestEntry(
                            ordinal = fileOrdinal,
                            entryId = stableEntryId(fileOrdinal, child),
                            relativePath = childPath,
                            kind = TransferEntryKind.FILE,
                            size = size,
                            modifiedAtEpochMillis = if (cursor.isNull(4)) null else cursor.getLong(4),
                            mimeType = mime?.takeUnless { it.isBlank() },
                            sha256 = null,
                        ),
                        sourceUri = child.toString(),
                        targetUri = null,
                    ))
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

    private fun summarize(entries: Sequence<StoredTransferEntry>): PreparedTransfer {
        val roots = linkedSetOf<String>()
        var fileCount = 0L
        var directoryCount = 0L
        var totalBytes = 0L
        entries.forEach { entry ->
            entry.validate()
            if (roots.size < 8) roots += entry.manifest.relativePath.substringBefore('/')
            if (entry.manifest.kind == TransferEntryKind.FILE) fileCount++ else directoryCount++
            totalBytes = Math.addExact(totalBytes, entry.manifest.size)
        }
        require(fileCount > 0 || directoryCount > 0)
        return PreparedTransfer(
            entries = entries,
            roots = roots.toList(),
            fileCount = fileCount,
            directoryCount = directoryCount,
            totalBytes = totalBytes,
        )
    }

    private fun stableEntryId(ordinal: Long, uri: Uri): String = UUID.nameUUIDFromBytes(
        "$ordinal\u0000$uri".toByteArray(Charsets.UTF_8),
    ).toString()

    private fun safeSegment(value: String): String = value
        .replace('/', '_')
        .replace('\\', '_')
        .trim()
        .takeIf { it.isNotEmpty() && it != "." && it != ".." }
        ?: "未命名"

    private fun reserve(names: MutableSet<String>, desired: String): String {
        if (names.add(desired.lowercase())) return desired
        val dot = desired.lastIndexOf('.').takeIf { it > 0 } ?: desired.length
        val stem = desired.substring(0, dot)
        val extension = desired.substring(dot)
        for (suffix in 1..Int.MAX_VALUE) {
            val candidate = "$stem ($suffix)$extension"
            if (names.add(candidate.lowercase())) return candidate
        }
        error("cannot reserve file name")
    }

    private data class Metadata(
        val name: String,
        val size: Long,
        val modifiedAt: Long?,
        val mimeType: String?,
    )
}
