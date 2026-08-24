package app.flowtype.imespike

import android.content.Intent
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputConnection
import android.widget.EditText
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class InputConnectionInstrumentedTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private lateinit var activity: MainActivity

    @Before
    fun launchActivity() {
        val intent = Intent(instrumentation.targetContext, MainActivity::class.java)
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TASK)
        activity = instrumentation.startActivitySync(intent) as MainActivity
        instrumentation.waitForIdleSync()
    }

    @After
    fun finishActivity() {
        instrumentation.runOnMainSync(activity::finish)
    }

    @Test
    fun composingCorrectionProducesCompleteSnapshots() = withInputConnection { connection, input ->
        assertTrue(connection.setComposingText("今天天气很号", 1))
        assertEquals("今天天气很号", input.text.toString())

        assertTrue(connection.setComposingText("今天天气很好", 1))
        assertEquals("今天天气很好", input.text.toString())

        assertTrue(connection.finishComposingText())
        assertTrue(connection.commitText("。", 1))
        assertEquals("今天天气很好。", input.text.toString())

        val events = activity.snapshotEventsForTest()
        assertStrictSequences(events)
        assertEquals("今天天气很好。", events.last().text)
        assertTrue(events.any { it.kind == ChangeKind.REPLACE })
    }

    @Test
    fun deleteAndReplaceTailAreObserved() = withInputConnection { connection, input ->
        assertTrue(connection.commitText("豆包正在识别语音", 1))
        assertTrue(connection.deleteSurroundingText(2, 0))
        assertEquals("豆包正在识别", input.text.toString())

        assertTrue(connection.commitText("文本", 1))
        assertEquals("豆包正在识别文本", input.text.toString())

        assertTrue(connection.setSelection(2, 4))
        assertTrue(connection.commitText("语音", 1))
        assertEquals("豆包语音识别文本", input.text.toString())

        val events = activity.snapshotEventsForTest()
        assertStrictSequences(events)
        assertEquals("豆包语音识别文本", events.last().text)
        assertTrue(events.any { it.kind == ChangeKind.DELETE })
        assertTrue(events.any { it.kind == ChangeKind.REPLACE })
    }

    @Test
    fun multilineAndEmojiRemainUnchanged() = withInputConnection { connection, input ->
        val expected = "第一行\n第二行🙂\n第三行，完成。"
        assertTrue(connection.commitText(expected, 1))
        assertEquals(expected, input.text.toString())

        val events = activity.snapshotEventsForTest()
        assertStrictSequences(events)
        assertEquals(expected, events.single().text)
    }

    @Test
    fun longSnapshotIsRecordedWithoutTruncation() = withInputConnection { connection, input ->
        val expected = buildString(5_000) {
            repeat(5_000) { append(if (it % 17 == 0) '。' else '说') }
        }
        assertTrue(connection.commitText(expected, 1))
        assertEquals(expected, input.text.toString())

        val events = activity.snapshotEventsForTest()
        assertStrictSequences(events)
        assertEquals(5_000, events.single().text.length)
        assertEquals(expected, events.single().text)
    }

    private fun withInputConnection(block: (InputConnection, EditText) -> Unit) {
        instrumentation.runOnMainSync {
            activity.resetForTest()
            val input = activity.findViewById<EditText>(R.id.input)
            input.requestFocus()
            val connection = input.onCreateInputConnection(EditorInfo())
            assertNotNull(connection)
            block(connection, input)
        }
        instrumentation.waitForIdleSync()
    }

    private fun assertStrictSequences(events: List<SnapshotEvent>) {
        assertTrue(events.isNotEmpty())
        assertEquals((1L..events.size.toLong()).toList(), events.map(SnapshotEvent::sequence))
    }
}
