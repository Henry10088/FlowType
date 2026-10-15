package app.flowtype.data

import android.content.ContentValues
import android.database.sqlite.SQLiteDatabase
import app.flowtype.protocol.MAX_BATCH_ID_BYTES
import app.flowtype.protocol.MAX_ENTRY_ID_BYTES
import app.flowtype.protocol.TransferBatchState
import app.flowtype.protocol.TransferDirection
import app.flowtype.protocol.TransferEntryKind
import app.flowtype.protocol.TransferEntryState
import app.flowtype.protocol.TransferManifestEntry
import java.util.Locale

data class FileTransferBatch(
    val batchId: String,
    val pcId: String,
    val direction: TransferDirection,
    val state: TransferBatchState,
    val createdAt: Long,
    val updatedAt: Long,
    val fileCount: Long,
    val directoryCount: Long,
    val totalBytes: Long,
    val targetTreeUri: String?,
)

data class StoredTransferEntry(
    val manifest: TransferManifestEntry,
    val sourceUri: String?,
    val targetUri: String?,
    val targetName: String? = null,
    val targetParentUri: String? = null,
    val state: TransferEntryState = TransferEntryState.PENDING,
    val committedOffset: Long = 0,
    val errorCode: String? = null,
    val commitSha256: String? = null,
    val finalUri: String? = null,
) {
    fun validate() {
        manifest.validate()
        require(committedOffset in 0..manifest.size) { "invalid committed offset" }
        require(state != TransferEntryState.COMPLETED || committedOffset == manifest.size) {
            "completed entry has incomplete offset"
        }
        require(sourceUri == null || sourceUri.length <= MAX_URI_CHARS) { "source URI too long" }
        require(targetUri == null || targetUri.length <= MAX_URI_CHARS) { "target URI too long" }
        require(targetName == null || (targetName.isNotBlank() && targetName.length <= 255)) {
            "invalid target name"
        }
        require(targetParentUri == null || targetParentUri.length <= MAX_URI_CHARS) {
            "target parent URI too long"
        }
        require(errorCode == null || errorCode.matches(Regex("[a-z0-9_]{1,64}"))) {
            "invalid error code"
        }
    }

    private companion object {
        const val MAX_URI_CHARS = 32 * 1024
    }
}

class FileTransferStore(private val database: AppDatabase) {
    fun createBatch(batch: FileTransferBatch) {
        batch.validate()
        database.writableDatabase.insertOrThrow(
            "file_transfer_batches",
            null,
            ContentValues().apply {
                put("batch_id", batch.batchId)
                put("pc_id", batch.pcId)
                put("direction", batch.direction.wireName)
                put("state", batch.state.wireName)
                put("created_at", batch.createdAt)
                put("updated_at", batch.updatedAt)
                put("file_count", batch.fileCount)
                put("directory_count", batch.directoryCount)
                put("total_bytes", batch.totalBytes)
                putNullable("target_tree_uri", batch.targetTreeUri)
            },
        )
    }

    /** Adds one bounded manifest page. The ordinal must continue exactly where
     * the preceding committed page ended, so retries cannot duplicate entries.
     */
    fun appendManifestPage(batchId: String, entries: List<StoredTransferEntry>) {
        require(entries.isNotEmpty() && entries.size <= MAX_MANIFEST_PAGE_ENTRIES) {
            "invalid manifest page size"
        }
        validateIdentifier(batchId, MAX_BATCH_ID_BYTES)
        entries.forEach(StoredTransferEntry::validate)
        val db = database.writableDatabase
        db.beginTransaction()
        try {
            val nextOrdinal = db.rawQuery(
                "SELECT COALESCE(MAX(ordinal) + 1, 0) FROM file_transfer_entries WHERE batch_id = ?",
                arrayOf(batchId),
            ).use { cursor ->
                check(cursor.moveToFirst())
                cursor.getLong(0)
            }
            require(entries.first().manifest.ordinal == nextOrdinal) { "manifest page is not contiguous" }
            entries.zipWithNext().forEach { (left, right) ->
                require(right.manifest.ordinal == left.manifest.ordinal + 1) {
                    "manifest page is not contiguous"
                }
            }
            entries.forEach { entry ->
                db.insertOrThrow("file_transfer_entries", null, entry.values(batchId))
            }
            db.setTransactionSuccessful()
        } finally {
            db.endTransaction()
        }
    }

    fun updateBatchState(batchId: String, next: TransferBatchState, now: Long) {
        val current = loadBatch(batchId) ?: error("unknown batch")
        require(current.state.canTransitionTo(next)) { "invalid batch state transition" }
        database.writableDatabase.update(
            "file_transfer_batches",
            ContentValues().apply {
                put("state", next.wireName)
                put("updated_at", maxOf(now, current.updatedAt))
            },
            "batch_id = ?",
            arrayOf(batchId),
        ).also { check(it == 1) }
    }

    fun cancelIncompleteEntries(batchId: String) {
        val terminal = listOf(
            TransferEntryState.COMPLETED,
            TransferEntryState.FAILED,
            TransferEntryState.CANCELED,
        )
        val placeholders = terminal.joinToString(",") { "?" }
        database.writableDatabase.update(
            "file_transfer_entries",
            ContentValues().apply { put("state", TransferEntryState.CANCELED.wireName) },
            "batch_id = ? AND state NOT IN ($placeholders)",
            arrayOf(batchId, *terminal.map { it.wireName }.toTypedArray()),
        )
    }

    fun setTargetTree(batchId: String, targetTreeUri: String, now: Long) {
        require(targetTreeUri.isNotBlank() && targetTreeUri.length <= 32 * 1024) {
            "invalid target tree URI"
        }
        val previous = requireNotNull(loadBatch(batchId))
        val db = database.writableDatabase
        db.beginTransaction()
        try {
            if (previous.targetTreeUri != null && previous.targetTreeUri != targetTreeUri) {
                db.execSQL(
                    """UPDATE file_transfer_entries SET target_uri = NULL, target_parent_uri = NULL,
                        target_name = NULL, committed_offset = 0, state = ?, error_code = NULL,
                        commit_sha256 = NULL, final_uri = NULL
                        WHERE batch_id = ? AND (kind = ? OR state != ?)""",
                    arrayOf(
                        TransferEntryState.PENDING.wireName, batchId,
                        TransferEntryKind.DIRECTORY.wireName, TransferEntryState.COMPLETED.wireName,
                    ),
                )
            }
            db.update(
                "file_transfer_batches",
                ContentValues().apply {
                    put("target_tree_uri", targetTreeUri)
                    put("updated_at", now)
                },
                "batch_id = ?",
                arrayOf(batchId),
            ).also { check(it == 1) }
            db.setTransactionSuccessful()
        } finally {
            db.endTransaction()
        }
    }

    /** An authenticated terminal receipt repairs a lost completion response. */
    fun reconcileResult(batchId: String, state: TransferBatchState, now: Long) {
        require(state.terminal)
        database.writableDatabase.update(
            "file_transfer_batches",
            ContentValues().apply {
                put("state", state.wireName)
                put("updated_at", now)
            },
            "batch_id = ?",
            arrayOf(batchId),
        ).also { check(it == 1) }
    }

    fun updateEntryProgress(
        batchId: String,
        ordinal: Long,
        state: TransferEntryState,
        committedOffset: Long,
        targetUri: String? = null,
        targetName: String? = null,
        errorCode: String? = null,
    ) {
        val current = entry(batchId, ordinal) ?: error("unknown entry")
        require(committedOffset in 0..current.manifest.size) { "invalid committed offset" }
        require(committedOffset >= current.committedOffset || committedOffset == 0L) {
            "committed offset cannot move backwards unless the file restarts"
        }
        require(state != TransferEntryState.COMPLETED || committedOffset == current.manifest.size) {
            "completed entry has incomplete offset"
        }
        require(errorCode == null || errorCode.matches(Regex("[a-z0-9_]{1,64}"))) {
            "invalid error code"
        }
        database.writableDatabase.update(
            "file_transfer_entries",
            ContentValues().apply {
                put("state", state.wireName)
                put("committed_offset", committedOffset)
                if (targetUri != null) put("target_uri", targetUri)
                if (targetName != null) put("target_name", targetName)
                putNullable("error_code", errorCode)
            },
            "batch_id = ? AND ordinal = ?",
            arrayOf(batchId, ordinal.toString()),
        ).also { check(it == 1) }
    }

    fun setEntryTarget(
        batchId: String,
        ordinal: Long,
        targetUri: String,
        targetName: String?,
        targetParentUri: String?,
    ) {
        require(targetUri.isNotBlank() && targetUri.length <= 32 * 1024)
        require(targetName == null || (targetName.isNotBlank() && targetName.length <= 255))
        database.writableDatabase.update(
            "file_transfer_entries",
            ContentValues().apply {
                put("target_uri", targetUri)
                putNullable("target_name", targetName)
                putNullable("target_parent_uri", targetParentUri)
            },
            "batch_id = ? AND ordinal = ?",
            arrayOf(batchId, ordinal.toString()),
        ).also { check(it == 1) }
    }

    fun journalCommit(batchId: String, ordinal: Long, hash: String, finalUri: String? = null) {
        require(hash.matches(Regex("[0-9a-f]{64}")))
        database.writableDatabase.update("file_transfer_entries", ContentValues().apply {
            put("commit_sha256", hash)
            putNullable("final_uri", finalUri)
        }, "batch_id = ? AND ordinal = ?", arrayOf(batchId, ordinal.toString()))
            .also { check(it == 1) }
    }

    fun loadBatch(batchId: String): FileTransferBatch? = database.readableDatabase.query(
        "file_transfer_batches",
        BATCH_COLUMNS,
        "batch_id = ?",
        arrayOf(batchId),
        null,
        null,
        null,
    ).use { cursor -> if (cursor.moveToFirst()) cursor.batch() else null }

    fun recoverableBatches(): List<FileTransferBatch> {
        val terminal = listOf(
            TransferBatchState.COMPLETED,
            TransferBatchState.PARTIAL_FAILURE,
            TransferBatchState.CANCELED,
        )
        val placeholders = terminal.joinToString(",") { "?" }
        return database.readableDatabase.query(
            "file_transfer_batches",
            BATCH_COLUMNS,
            "state NOT IN ($placeholders)",
            terminal.map { it.wireName }.toTypedArray(),
            null,
            null,
            "created_at, batch_id",
        ).use { cursor -> buildList { while (cursor.moveToNext()) add(cursor.batch()) } }
    }

    fun batchesOlderThan(updatedBefore: Long): List<FileTransferBatch> =
        database.readableDatabase.query(
            "file_transfer_batches",
            BATCH_COLUMNS,
            "updated_at < ?",
            arrayOf(updatedBefore.toString()),
            null,
            null,
            "updated_at, batch_id",
        ).use { cursor -> buildList { while (cursor.moveToNext()) add(cursor.batch()) } }

    fun canceledBatches(pcId: String): List<FileTransferBatch> = database.readableDatabase.query(
        "file_transfer_batches", BATCH_COLUMNS, "pc_id = ? AND state = ?",
        arrayOf(pcId, TransferBatchState.CANCELED.wireName), null, null, "created_at",
    ).use { cursor -> buildList { while (cursor.moveToNext()) add(cursor.batch()) } }

    fun deleteBatch(batchId: String) {
        validateIdentifier(batchId, MAX_BATCH_ID_BYTES)
        database.writableDatabase.delete(
            "file_transfer_batches",
            "batch_id = ?",
            arrayOf(batchId),
        )
    }

    fun entriesPage(batchId: String, startOrdinal: Long, limit: Int): List<StoredTransferEntry> {
        require(startOrdinal >= 0 && limit in 1..MAX_MANIFEST_PAGE_ENTRIES)
        return database.readableDatabase.query(
            "file_transfer_entries",
            ENTRY_COLUMNS,
            "batch_id = ? AND ordinal >= ?",
            arrayOf(batchId, startOrdinal.toString()),
            null,
            null,
            "ordinal",
            limit.toString(),
        ).use { cursor -> buildList { while (cursor.moveToNext()) add(cursor.entry()) } }
    }

    fun entry(batchId: String, ordinal: Long): StoredTransferEntry? = database.readableDatabase.query(
        "file_transfer_entries",
        ENTRY_COLUMNS,
        "batch_id = ? AND ordinal = ?",
        arrayOf(batchId, ordinal.toString()),
        null,
        null,
        null,
    ).use { cursor -> if (cursor.moveToFirst()) cursor.entry() else null }

    fun manifestEntryCount(batchId: String): Long = database.readableDatabase.rawQuery(
        "SELECT COUNT(*) FROM file_transfer_entries WHERE batch_id = ?",
        arrayOf(batchId),
    ).use { cursor ->
        check(cursor.moveToFirst())
        cursor.getLong(0)
    }

    fun failedEntryCount(batchId: String): Long = database.readableDatabase.rawQuery(
        "SELECT COUNT(*) FROM file_transfer_entries WHERE batch_id = ? AND state = ?",
        arrayOf(batchId, TransferEntryState.FAILED.wireName),
    ).use { cursor ->
        check(cursor.moveToFirst())
        cursor.getLong(0)
    }

    fun failedEntries(batchId: String, limit: Int = 20): List<StoredTransferEntry> {
        require(limit in 1..100)
        return database.readableDatabase.query(
            "file_transfer_entries",
            ENTRY_COLUMNS,
            "batch_id = ? AND state = ?",
            arrayOf(batchId, TransferEntryState.FAILED.wireName),
            null,
            null,
            "ordinal",
            limit.toString(),
        ).use { cursor -> buildList { while (cursor.moveToNext()) add(cursor.entry()) } }
    }

    private fun FileTransferBatch.validate() {
        validateIdentifier(batchId, MAX_BATCH_ID_BYTES)
        require(pcId.isNotBlank() && pcId.length <= 128) { "invalid computer" }
        require(createdAt >= 0 && updatedAt >= createdAt) { "invalid timestamp" }
        require(fileCount >= 0 && directoryCount >= 0 && (fileCount > 0 || directoryCount > 0)) {
            "empty batch"
        }
        require(totalBytes >= 0) { "invalid total size" }
        require(targetTreeUri == null || targetTreeUri.length <= 32 * 1024) {
            "target tree URI too long"
        }
    }

    private fun StoredTransferEntry.values(batchId: String) = ContentValues().apply {
        put("batch_id", batchId)
        put("ordinal", manifest.ordinal)
        put("entry_id", manifest.entryId)
        putNullable("source_uri", sourceUri)
        put("relative_path", manifest.relativePath)
        put("kind", manifest.kind.wireName)
        put("size", manifest.size)
        putNullable("modified_at", manifest.modifiedAtEpochMillis)
        putNullable("mime_type", manifest.mimeType)
        putNullable("sha256", manifest.sha256)
        putNullable("target_uri", targetUri)
        putNullable("target_name", targetName)
        putNullable("target_parent_uri", targetParentUri)
        put("state", state.wireName)
        put("committed_offset", committedOffset)
        putNullable("error_code", errorCode)
        putNullable("commit_sha256", commitSha256)
        putNullable("final_uri", finalUri)
    }

    private fun android.database.Cursor.batch() = FileTransferBatch(
        batchId = getString(0),
        pcId = getString(1),
        direction = enumByWireName(getString(2)),
        state = enumByWireName(getString(3)),
        createdAt = getLong(4),
        updatedAt = getLong(5),
        fileCount = getLong(6),
        directoryCount = getLong(7),
        totalBytes = getLong(8),
        targetTreeUri = if (isNull(9)) null else getString(9),
    )

    private fun android.database.Cursor.entry(): StoredTransferEntry = StoredTransferEntry(
        manifest = TransferManifestEntry(
            ordinal = getLong(0),
            entryId = getString(1),
            relativePath = getString(3),
            kind = enumByWireName(getString(4)),
            size = getLong(5),
            modifiedAtEpochMillis = if (isNull(6)) null else getLong(6),
            mimeType = if (isNull(7)) null else getString(7),
            sha256 = if (isNull(8)) null else getString(8),
        ),
        sourceUri = if (isNull(2)) null else getString(2),
        targetUri = if (isNull(9)) null else getString(9),
        targetName = if (isNull(10)) null else getString(10),
        targetParentUri = if (isNull(11)) null else getString(11),
        state = enumByWireName(getString(12)),
        committedOffset = getLong(13),
        errorCode = if (isNull(14)) null else getString(14),
        commitSha256 = if (isNull(15)) null else getString(15),
        finalUri = if (isNull(16)) null else getString(16),
    )

    private inline fun <reified T : Enum<T>> enumByWireName(value: String): T = enumValues<T>().single {
        when (it) {
            is TransferDirection -> it.wireName
            is TransferBatchState -> it.wireName
            is TransferEntryKind -> it.wireName
            is TransferEntryState -> it.wireName
            else -> it.name.lowercase(Locale.ROOT)
        } == value
    }

    private fun validateIdentifier(value: String, maxBytes: Int) {
        val bytes = value.toByteArray(Charsets.UTF_8)
        require(bytes.isNotEmpty() && bytes.size <= maxBytes && bytes.all {
            val byte = it.toInt() and 0xff
            byte in 'a'.code..'z'.code ||
                byte in 'A'.code..'Z'.code ||
                byte in '0'.code..'9'.code ||
                byte == '-'.code ||
                byte == '_'.code
        }) { "invalid identifier" }
    }

    private fun ContentValues.putNullable(key: String, value: String?) {
        if (value == null) putNull(key) else put(key, value)
    }

    private fun ContentValues.putNullable(key: String, value: Long?) {
        if (value == null) putNull(key) else put(key, value)
    }

    private companion object {
        const val MAX_MANIFEST_PAGE_ENTRIES = 256
        val BATCH_COLUMNS = arrayOf(
            "batch_id",
            "pc_id",
            "direction",
            "state",
            "created_at",
            "updated_at",
            "file_count",
            "directory_count",
            "total_bytes",
            "target_tree_uri",
        )
        val ENTRY_COLUMNS = arrayOf(
            "ordinal",
            "entry_id",
            "source_uri",
            "relative_path",
            "kind",
            "size",
            "modified_at",
            "mime_type",
            "sha256",
            "target_uri",
            "target_name",
            "target_parent_uri",
            "state",
            "committed_offset",
            "error_code",
            "commit_sha256",
            "final_uri",
        )
    }
}
