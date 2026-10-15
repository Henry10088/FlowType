package app.flowtype.debug

import android.database.Cursor
import android.database.MatrixCursor
import android.os.CancellationSignal
import android.os.ParcelFileDescriptor
import android.provider.DocumentsContract.Document
import android.provider.DocumentsProvider
import java.io.File

/** Debug-only local SAF provider for interrupted rename/copy regression tests. */
class TransferTestDocumentsProvider : DocumentsProvider() {
    override fun onCreate() = true

    private fun file(id: String): File {
        val root = File(requireNotNull(context).cacheDir, "transfer-saf-tests").apply { mkdirs() }
        val result = File(root, id).canonicalFile
        require(result.toPath().startsWith(root.canonicalFile.toPath()))
        return result
    }

    private fun row(cursor: MatrixCursor, id: String) {
        val file = file(id)
        require(file.exists())
        cursor.newRow().apply {
            cursor.columnNames.forEach { column ->
                add(column, when (column) {
                    Document.COLUMN_DOCUMENT_ID -> id
                    Document.COLUMN_DISPLAY_NAME -> file.name
                    Document.COLUMN_MIME_TYPE -> if (file.isDirectory) Document.MIME_TYPE_DIR else "application/octet-stream"
                    Document.COLUMN_SIZE -> file.length()
                    Document.COLUMN_FLAGS -> Document.FLAG_SUPPORTS_WRITE or Document.FLAG_SUPPORTS_DELETE or Document.FLAG_SUPPORTS_RENAME
                    else -> null
                })
            }
        }
    }

    override fun queryRoots(projection: Array<out String>?): Cursor = MatrixCursor(projection ?: arrayOf("root_id"))

    override fun isChildDocument(parentDocumentId: String, documentId: String): Boolean =
        file(documentId).toPath().startsWith(file(parentDocumentId).toPath())

    override fun queryDocument(documentId: String, projection: Array<out String>?): Cursor =
        MatrixCursor(projection ?: COLUMNS).also { row(it, documentId) }

    override fun queryChildDocuments(parentDocumentId: String, projection: Array<out String>?, sortOrder: String?): Cursor =
        MatrixCursor(projection ?: COLUMNS).also { cursor ->
            file(parentDocumentId).listFiles().orEmpty().sortedBy { it.name }.forEach {
                row(cursor, "$parentDocumentId/${it.name}")
            }
        }

    override fun createDocument(parentDocumentId: String, mimeType: String, displayName: String): String {
        require('/' !in displayName && displayName != "..")
        val id = "$parentDocumentId/$displayName"
        val target = file(id)
        check(if (mimeType == Document.MIME_TYPE_DIR) target.mkdir() else target.createNewFile())
        return id
    }

    override fun renameDocument(documentId: String, displayName: String): String {
        if (rejectRename) throw UnsupportedOperationException("test provider cannot rename")
        val id = documentId.substringBeforeLast('/') + "/" + displayName
        check(!file(id).exists() && file(documentId).renameTo(file(id)))
        if (failAfterRename) throw IllegalStateException("simulated process loss after rename")
        return id
    }

    override fun deleteDocument(documentId: String) {
        check(file(documentId).deleteRecursively())
    }

    override fun openDocument(documentId: String, mode: String, signal: CancellationSignal?): ParcelFileDescriptor =
        ParcelFileDescriptor.open(file(documentId), ParcelFileDescriptor.parseMode(mode))

    companion object {
        const val AUTHORITY = "app.flowtype.transfer-tests"
        @Volatile var rejectRename = false
        @Volatile var failAfterRename = false
        private val COLUMNS = arrayOf(Document.COLUMN_DOCUMENT_ID, Document.COLUMN_DISPLAY_NAME,
            Document.COLUMN_MIME_TYPE, Document.COLUMN_SIZE, Document.COLUMN_FLAGS)
    }
}
