package app.flowtype

import android.content.Context
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import app.flowtype.data.AppDatabase
import app.flowtype.data.HistoryStore
import app.flowtype.pairing.BindingStore
import app.flowtype.pairing.ComputerBinding
import app.flowtype.security.SecureDraftStore
import app.flowtype.session.ComputerSessions
import app.flowtype.session.InputSession
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
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

    private fun binding(pcId: String) = ComputerBinding(
        pcId = pcId,
        pcName = pcId,
        endpoint = "wss://127.0.0.1:39421/v1/sync",
        tlsSpkiSha256 = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
        pairingToken = null,
    )
}
