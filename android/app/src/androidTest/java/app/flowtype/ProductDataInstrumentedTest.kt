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
    fun reselectingAnotherTreeResetsPartialTargetsButPreservesCompletedFiles() {
        val databaseName = "flowtype-reselection-${UUID.randomUUID()}.db"
        val database = AppDatabase(context, databaseName)
        try {
            val store = FileTransferStore(database)
            store.createBatch(FileTransferBatch("reselect", "pc-1", TransferDirection.COMPUTER_TO_PHONE,
                TransferBatchState.WAITING_CONFIRMATION, 1, 1, 2, 1, 84, null))
            store.appendManifestPage("reselect", listOf(
                stored(0, "folder", TransferEntryKind.DIRECTORY, 0),
                stored(1, "folder/partial", TransferEntryKind.FILE, 42),
                stored(2, "folder/completed", TransferEntryKind.FILE, 42),
            ))
            store.setTargetTree("reselect", "content://documents/tree/old", 2)
            for (ordinal in 0L..2L) {
                store.setEntryTarget("reselect", ordinal, "content://documents/old/$ordinal", "file-$ordinal", "content://documents/old")
            }
            store.updateEntryProgress("reselect", 0, TransferEntryState.COMPLETED, 0)
            store.updateEntryProgress("reselect", 1, TransferEntryState.WAITING_NETWORK, 16)
            store.updateEntryProgress("reselect", 2, TransferEntryState.COMPLETED, 42)
            store.setTargetTree("reselect", "content://documents/tree/old", 3)
            assertEquals(16L, store.entry("reselect", 1)?.committedOffset)
            assertEquals("content://documents/old/1", store.entry("reselect", 1)?.targetUri)
            store.setTargetTree("reselect", "content://documents/tree/new", 4)
            assertEquals(null, store.entry("reselect", 0)?.targetUri)
            assertEquals(null, store.entry("reselect", 1)?.targetUri)
            assertEquals(null, store.entry("reselect", 1)?.targetParentUri)
            assertEquals(0L, store.entry("reselect", 1)?.committedOffset)
            assertEquals(TransferEntryState.PENDING, store.entry("reselect", 1)?.state)
            assertEquals("content://documents/old/2", store.entry("reselect", 2)?.targetUri)
            assertEquals(TransferEntryState.COMPLETED, store.entry("reselect", 2)?.state)
        } finally {
            database.close()
            context.deleteDatabase(databaseName)
        }
    }

    @Test
    fun recoveryUsesOnlySelectedComputerAndTerminalReceiptClearsPendingBatch() {
        val databaseName = "flowtype-recovery-${UUID.randomUUID()}.db"
        val database = AppDatabase(context, databaseName)
        val store = FileTransferStore(database)
        val requests = java.util.concurrent.CopyOnWriteArrayList<String>()
        val completed = java.util.concurrent.CountDownLatch(1)
        val manager = app.flowtype.filetransfer.FileTransferManager(
            context, store, "phone-1",
            sendOffer = { pcId, offer -> requests += "$pcId:${offer.batchId}"; true },
            sendDecision = { pcId, batchId, _ -> requests += "$pcId:$batchId"; true },
            sendCancel = { _, _ -> true },
            parallelism = { 1 },
            onChanged = { if (!it.active && it.batchId == "selected") completed.countDown() },
        )
        try {
            val now = System.currentTimeMillis()
            store.createBatch(FileTransferBatch("other", "pc-a", TransferDirection.PHONE_TO_COMPUTER,
                TransferBatchState.WAITING_NETWORK, now, now, 1, 0, 42, null))
            store.createBatch(FileTransferBatch("selected", "pc-b", TransferDirection.COMPUTER_TO_PHONE,
                TransferBatchState.WAITING_NETWORK, now, now, 1, 0, 42, "content://documents/tree/target"))
            manager.setBinding(binding("pc-b"))
            manager.resumeRecoverable()
            // The worker processes the receipt after all recovery requests.
            manager.onResult("selected", TransferBatchState.COMPLETED)
            assertTrue(completed.await(10, java.util.concurrent.TimeUnit.SECONDS))
            assertEquals(listOf("pc-b:selected"), requests.toList())
            assertEquals(TransferBatchState.COMPLETED, store.loadBatch("selected")?.state)
            assertEquals(TransferBatchState.WAITING_NETWORK, store.loadBatch("other")?.state)
        } finally {
            manager.close()
            database.close()
            context.deleteDatabase(databaseName)
        }
    }

    @Test
    fun fileTransferSchemaUpgradesFromVersionsThreeThroughSix() {
        listOf(3, 4, 5, 6).forEach { oldVersion ->
            val databaseName = "flowtype-migration-$oldVersion-${UUID.randomUUID()}.db"
            context.openOrCreateDatabase(databaseName, Context.MODE_PRIVATE, null).use { db ->
                if (oldVersion >= 4) db.execSQL(
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
                        ${if (oldVersion >= 6) "target_parent_uri TEXT," else ""}
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
                assertTrue(columns.contains("commit_sha256"))
                assertTrue(columns.contains("final_uri"))
            } finally {
                database.close()
                context.deleteDatabase(databaseName)
            }
        }
    }

    @Test
    fun systemPauseDefersRecoveryAndReconnectReplaysOfflineCancellation() {
        val databaseName = "flowtype-pause-${UUID.randomUUID()}.db"
        val database = AppDatabase(context, databaseName)
        val store = FileTransferStore(database)
        val requests = java.util.concurrent.CopyOnWriteArrayList<String>()
        val pausedBarrier = java.util.concurrent.CountDownLatch(1)
        val resumedBarrier = java.util.concurrent.CountDownLatch(1)
        val manager = app.flowtype.filetransfer.FileTransferManager(context, store, "phone-1",
            sendOffer = { _, offer -> requests += "offer:${offer.batchId}"; true },
            sendDecision = { _, _, _ -> true },
            sendCancel = { _, id -> requests += "cancel:$id"; true }, parallelism = { 2 },
            onChanged = {
                if (it.batchId == "barrier") pausedBarrier.countDown()
                if (it.batchId == "first") resumedBarrier.countDown()
            })
        try {
            val now = System.currentTimeMillis()
            listOf("canceled", "first", "second", "barrier").forEachIndexed { index, id ->
                store.createBatch(FileTransferBatch(id, "pc", TransferDirection.PHONE_TO_COMPUTER,
                    if (id == "canceled") TransferBatchState.CANCELED else TransferBatchState.WAITING_NETWORK,
                    now + index, now + index, 1, 0, 1, null))
            }
            manager.setBinding(binding("pc"))
            manager.pauseExecution()
            manager.resumeRecoverable()
            manager.onResult("barrier", TransferBatchState.COMPLETED)
            assertTrue(pausedBarrier.await(10, java.util.concurrent.TimeUnit.SECONDS))
            assertTrue(requests.isEmpty())
            manager.resumeExecution()
            manager.onResult("first", TransferBatchState.COMPLETED)
            assertTrue(resumedBarrier.await(10, java.util.concurrent.TimeUnit.SECONDS))
            assertEquals(listOf("cancel:canceled", "offer:first"), requests.toList())
            assertEquals(TransferBatchState.WAITING_NETWORK, store.loadBatch("second")!!.state)
        } finally {
            manager.close()
            database.close()
            context.deleteDatabase(databaseName)
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
