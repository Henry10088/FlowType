package app.flowtype

import android.content.Context
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import app.flowtype.data.AppDatabase
import app.flowtype.data.HistoryStore
import app.flowtype.data.FileTransferBatch
import app.flowtype.data.FileTransferStore
import app.flowtype.data.StoredTransferEntry
import app.flowtype.pairing.BindingStore
import app.flowtype.pairing.ComputerBinding
import app.flowtype.security.SecureDraftStore
import app.flowtype.session.ComputerSessions
import app.flowtype.session.InputSession
import app.flowtype.protocol.TransferBatchState
import app.flowtype.protocol.TransferDirection
import app.flowtype.protocol.TransferEntryKind
import app.flowtype.protocol.TransferEntryState
import app.flowtype.protocol.TransferManifestEntry
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Assume.assumeTrue
import org.junit.Test
import org.junit.runner.RunWith
import java.util.UUID

@RunWith(AndroidJUnit4::class)
class ProductDataInstrumentedTest {
    private val context: Context
        get() = InstrumentationRegistry.getInstrumentation().targetContext

    @Test
    fun historyRoundTripsWithoutPlaintextInDatabase() {
        val databaseName = "flowtype-test-${UUID.randomUUID()}.db"
        val database = AppDatabase(context, databaseName)
        try {
            val store = HistoryStore(database)
            val marker = "加密历史-${UUID.randomUUID()}"
            val binding = binding("history-${UUID.randomUUID()}")
            store.add(binding, marker)

            val entry = store.list().first { it.text == marker }
            val encrypted = database.readableDatabase.query(
                "history", arrayOf("encrypted_text"), "id = ?", arrayOf(entry.id.toString()), null, null, null,
            ).use { cursor -> cursor.moveToFirst(); cursor.getBlob(0) }
            assertFalse(encrypted.toString(Charsets.UTF_8).contains(marker))
            assertEquals(marker, entry.text)
        } finally {
            database.close()
            context.deleteDatabase(databaseName)
        }
    }

    @Test
    fun bindingStoreKeepsMultipleComputersAndSelection() {
        val databaseName = "flowtype-test-${UUID.randomUUID()}.db"
        val database = AppDatabase(context, databaseName)
        try {
            val store = BindingStore(context, database)
            val first = binding("test-a-${UUID.randomUUID()}")
            val second = binding("test-b-${UUID.randomUUID()}")
            store.save(first)
            store.save(second)
            assertTrue(store.list().any { it.pcId == first.pcId })
            assertEquals(second.pcId, store.load()?.pcId)
            assertEquals(first.pcId, store.select(first.pcId)?.pcId)
            assertEquals(first.pcId, store.load()?.pcId)
        } finally {
            database.close()
            context.deleteDatabase(databaseName)
        }
    }

    @Test
    fun independentDraftsRoundTripWithoutPlaintextInPreferences() {
        val preferencesName = "flowtype-test-draft-${UUID.randomUUID()}"
        val store = SecureDraftStore(context, preferencesName, "flowtype-test-key-${UUID.randomUUID()}")
        val markerA = "电脑A草稿-${UUID.randomUUID()}"
        val markerB = "电脑B草稿-${UUID.randomUUID()}"
        store.save(
            "pc-a",
            ComputerSessions.ParkedSession(
                InputSession.State("session-a", markerA, 2, 1, false),
                remoteStarted = true,
            ),
        )
        store.save(
            "pc-b",
            ComputerSessions.ParkedSession(
                InputSession.State("session-b", markerB, 1, 0, false),
                remoteStarted = false,
            ),
        )
        val raw = context.getSharedPreferences(preferencesName, Context.MODE_PRIVATE)
            .all.values.joinToString()
        assertFalse(raw.contains(markerA))
        assertFalse(raw.contains(markerB))
        assertEquals(markerA, store.load("pc-a")?.state?.text)
        assertEquals(markerB, store.load("pc-b")?.state?.text)
        assertTrue(store.load("pc-a")?.remoteStarted == true)
        store.clear("pc-a")
        assertEquals(markerB, store.load("pc-b")?.state?.text)
        store.clear("pc-b")
        context.deleteSharedPreferences(preferencesName)
    }

    @Test
    fun fileTransferManifestIsPagedAndProgressSurvivesReload() {
        val databaseName = "flowtype-test-${UUID.randomUUID()}.db"
        val database = AppDatabase(context, databaseName)
        try {
            val store = FileTransferStore(database)
            store.createBatch(
                FileTransferBatch(
                    batchId = "batch-1",
                    pcId = "pc-1",
                    direction = TransferDirection.COMPUTER_TO_PHONE,
                    state = TransferBatchState.WAITING_CONFIRMATION,
                    createdAt = 10,
                    updatedAt = 10,
                    fileCount = 1,
                    directoryCount = 1,
                    totalBytes = 42,
                    targetTreeUri = null,
                ),
            )
            store.appendManifestPage(
                "batch-1",
                listOf(
                    stored(0, "资料", TransferEntryKind.DIRECTORY, 0),
                    stored(1, "资料/说明.txt", TransferEntryKind.FILE, 42),
                ),
            )
            store.setTargetTree("batch-1", "content://documents/tree/downloads", 11)
            store.updateBatchState("batch-1", TransferBatchState.QUEUED, 12)
            store.updateBatchState("batch-1", TransferBatchState.PREPARING, 13)
            store.setEntryTarget(
                "batch-1",
                1,
                "content://documents/document/partial",
                "说明 (1).txt",
                "content://documents/document/资料",
            )
            store.updateEntryProgress(
                "batch-1",
                1,
                TransferEntryState.WAITING_NETWORK,
                16,
            )

            assertEquals(2, store.entriesPage("batch-1", 0, 10).size)
            assertEquals(16L, store.entry("batch-1", 1)?.committedOffset)
            assertEquals("说明 (1).txt", store.entry("batch-1", 1)?.targetName)
            assertEquals(
                "content://documents/document/资料",
                store.entry("batch-1", 1)?.targetParentUri,
            )
            assertEquals(TransferBatchState.PREPARING, store.recoverableBatches().single().state)
        } finally {
            database.close()
            context.deleteDatabase(databaseName)
        }
    }

    @Test
    fun fileTransferStoreHandlesOneHundredThousandPagedEntries() {
        assumeTrue(
            "explicit 100k-entry release acceptance",
            InstrumentationRegistry.getArguments().getString("flowtypeStress") == "true",
        )
        val databaseName = "flowtype-stress-${UUID.randomUUID()}.db"
        val database = AppDatabase(context, databaseName)
        val entryCount = 100_000L
        try {
            val store = FileTransferStore(database)
            store.createBatch(
                FileTransferBatch(
                    batchId = "batch-100k",
                    pcId = "pc-1",
                    direction = TransferDirection.COMPUTER_TO_PHONE,
                    state = TransferBatchState.WAITING_CONFIRMATION,
                    createdAt = 10,
                    updatedAt = 10,
                    fileCount = entryCount,
                    directoryCount = 0,
                    totalBytes = entryCount,
                    targetTreeUri = null,
                ),
            )
            var ordinal = 0L
            while (ordinal < entryCount) {
                val pageEnd = minOf(ordinal + 256, entryCount)
                store.appendManifestPage(
                    "batch-100k",
                    (ordinal until pageEnd).map { index ->
                        stored(
                            index,
                            "directory-${index / 1_000}/file-$index.txt",
                            TransferEntryKind.FILE,
                            1,
                        )
                    },
                )
                ordinal = pageEnd
            }
            assertEquals(256, store.entriesPage("batch-100k", 0, 256).size)
            assertEquals(
                entryCount - 1,
                store.entry("batch-100k", entryCount - 1)?.manifest?.ordinal,
            )
        } finally {
            database.close()
            context.deleteDatabase(databaseName)
        }
    }

    @Test
    fun fileTransferSchemaUpgradesFromVersionsFourAndFive() {
        listOf(4, 5).forEach { oldVersion ->
            val databaseName = "flowtype-migration-$oldVersion-${UUID.randomUUID()}.db"
            context.openOrCreateDatabase(databaseName, Context.MODE_PRIVATE, null).use { db ->
                db.execSQL(
                    """CREATE TABLE file_transfer_entries (
                        batch_id TEXT NOT NULL,
                        ordinal INTEGER NOT NULL,
                        entry_id TEXT NOT NULL,
                        source_uri TEXT,
                        relative_path TEXT NOT NULL,
                        kind TEXT NOT NULL,
                        size INTEGER NOT NULL,
                        modified_at INTEGER,
                        mime_type TEXT,
                        sha256 TEXT,
                        target_uri TEXT,
                        ${if (oldVersion >= 5) "target_name TEXT," else ""}
                        state TEXT NOT NULL,
                        committed_offset INTEGER NOT NULL DEFAULT 0,
                        error_code TEXT,
                        PRIMARY KEY (batch_id, ordinal)
                    )""",
                )
                db.version = oldVersion
            }
            val database = AppDatabase(context, databaseName)
            try {
                val columns = database.readableDatabase.rawQuery(
                    "PRAGMA table_info(file_transfer_entries)",
                    null,
                ).use { cursor ->
                    buildSet {
                        while (cursor.moveToNext()) add(cursor.getString(1))
                    }
                }
                assertTrue(columns.contains("target_name"))
                assertTrue(columns.contains("target_parent_uri"))
            } finally {
                database.close()
                context.deleteDatabase(databaseName)
            }
        }
    }

    private fun binding(pcId: String) = ComputerBinding(
        pcId = pcId,
        pcName = pcId,
        endpoint = "wss://127.0.0.1:39421/v1/sync",
        tlsSpkiSha256 = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
        pairingToken = null,
    )

    private fun stored(
        ordinal: Long,
        path: String,
        kind: TransferEntryKind,
        size: Long,
    ) = StoredTransferEntry(
        manifest = TransferManifestEntry(
            ordinal = ordinal,
            entryId = "entry-$ordinal",
            relativePath = path,
            kind = kind,
            size = size,
            modifiedAtEpochMillis = null,
            mimeType = null,
            sha256 = null,
        ),
        sourceUri = "content://documents/document/$ordinal",
        targetUri = null,
    )
}
