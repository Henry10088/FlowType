package app.flowtype

import android.provider.DocumentsContract
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import app.flowtype.data.AppDatabase
import app.flowtype.data.FileTransferBatch
import app.flowtype.data.FileTransferStore
import app.flowtype.data.StoredTransferEntry
import app.flowtype.debug.TransferTestDocumentsProvider
import app.flowtype.filetransfer.AndroidTargetStore
import app.flowtype.protocol.*
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import java.io.File
import java.security.MessageDigest
import java.util.UUID
import java.util.concurrent.CancellationException

@RunWith(AndroidJUnit4::class)
class TransferCommitInstrumentedTest {
    @Test fun renameCompletedBeforeCrashIsRecoveredWithoutDuplicateOrMissingFile() = fixture { store, targets, root, entry, hash ->
        TransferTestDocumentsProvider.failAfterRename = true
        assertTrue(runCatching { targets.commit("batch", entry, hash) {} }.isFailure)
        assertTrue(File(root, "测试.txt").isFile)
        assertTrue(targets.recoverCommit("batch", store.entry("batch", 0)!!) {})
        assertEquals(TransferEntryState.COMPLETED, store.entry("batch", 0)!!.state)
        assertEquals(listOf("测试.txt"), root.list()!!.toList())
    }

    @Test fun interruptedCopyIsRemovedAndVerifiedTempCanBeRetried() = fixture { store, targets, root, entry, hash ->
        TransferTestDocumentsProvider.rejectRename = true
        var checks = 0
        assertTrue(runCatching {
            targets.commit("batch", entry, hash) {
                if (++checks == 3) throw CancellationException("interrupt copy")
            }
        }.isFailure)
        assertEquals(listOf(".flowtype-entry.part"), root.list()!!.toList())
        val result = targets.commit("batch", store.entry("batch", 0)!!, hash) {}
        assertEquals("测试.txt", DocumentsContract.getDocumentId(result).substringAfterLast('/'))
        assertEquals(TransferEntryState.COMPLETED, store.entry("batch", 0)!!.state)
        assertEquals(listOf("测试.txt"), root.list()!!.toList())
    }

    @Test fun processDeathDuringCopyDoesNotLeavePartialFinalFiles() = fixture { store, targets, root, entry, hash ->
        val partial = File(root, "测试.txt").apply { writeText("partial") }
        val finalUri = DocumentsContract.buildDocumentUriUsingTree(android.net.Uri.parse(entry.targetParentUri),
            "${root.name}/测试.txt")
        store.journalCommit("batch", 0, hash, finalUri.toString())
        assertFalse(targets.recoverCommit("batch", store.entry("batch", 0)!!) {})
        assertFalse(partial.exists())
        targets.commit("batch", store.entry("batch", 0)!!, hash) {}
        assertEquals(listOf("测试.txt"), root.list()!!.toList())
    }

    @Test fun commitDoesNotOverwriteAFileCreatedAfterPreparation() = fixture { _, targets, root, entry, hash ->
        File(root, "测试.txt").writeText("existing")
        targets.commit("batch", entry, hash) {}
        assertEquals("existing", File(root, "测试.txt").readText())
        assertTrue(File(root, "测试 (1).txt").isFile)
    }

    private fun fixture(test: (FileTransferStore, AndroidTargetStore, File, StoredTransferEntry, String) -> Unit) {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val id = UUID.randomUUID().toString()
        val root = File(context.cacheDir, "transfer-saf-tests/$id").apply { mkdirs() }
        val database = AppDatabase(context, "transfer-commit-$id.db")
        TransferTestDocumentsProvider.rejectRename = false
        TransferTestDocumentsProvider.failAfterRename = false
        try {
            val store = FileTransferStore(database)
            val targets = AndroidTargetStore(context.contentResolver, store)
            val tree = DocumentsContract.buildTreeDocumentUri(TransferTestDocumentsProvider.AUTHORITY, id)
            val bytes = ByteArray(600_000) { (it % 251).toByte() }
            val hash = MessageDigest.getInstance("SHA-256").digest(bytes).joinToString("") { "%02x".format(it) }
            store.createBatch(FileTransferBatch("batch", "pc", TransferDirection.COMPUTER_TO_PHONE,
                TransferBatchState.PREPARING, 1, 1, 1, 0, bytes.size.toLong(), tree.toString()))
            store.appendManifestPage("batch", listOf(StoredTransferEntry(
                TransferManifestEntry(0, "entry", "测试.txt", TransferEntryKind.FILE, bytes.size.toLong(), null, null, null), null, null)))
            targets.prepare("batch", tree)
            val entry = store.entry("batch", 0)!!
            context.contentResolver.openOutputStream(android.net.Uri.parse(entry.targetUri))!!.use { it.write(bytes) }
            test(store, targets, root, entry, hash)
        } finally {
            TransferTestDocumentsProvider.rejectRename = false
            TransferTestDocumentsProvider.failAfterRename = false
            database.close()
            context.deleteDatabase("transfer-commit-$id.db")
            root.deleteRecursively()
        }
    }
}
