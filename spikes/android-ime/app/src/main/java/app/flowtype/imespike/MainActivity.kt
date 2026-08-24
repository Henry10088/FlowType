package app.flowtype.imespike

import android.app.Activity
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.os.Bundle
import android.os.SystemClock
import android.text.Editable
import android.text.TextWatcher
import android.view.inputmethod.BaseInputConnection
import android.view.inputmethod.InputMethodManager
import android.widget.Button
import android.widget.EditText
import android.widget.TextView
import android.widget.Toast

class MainActivity : Activity() {
    private lateinit var input: EditText
    private lateinit var summary: TextView
    private lateinit var eventLog: TextView

    private val events = ArrayList<SnapshotEvent>()
    private var sequence = 0L
    private var startedAtMs = 0L
    private var changeStart = 0
    private var removedUtf16 = 0
    private var addedUtf16 = 0

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_main)

        input = findViewById(R.id.input)
        summary = findViewById(R.id.summary)
        eventLog = findViewById(R.id.eventLog)
        startedAtMs = SystemClock.elapsedRealtime()

        input.addTextChangedListener(snapshotWatcher)

        findViewById<Button>(R.id.clear).setOnClickListener {
            input.text.clear()
            input.requestFocus()
            showKeyboard()
        }
        findViewById<Button>(R.id.copyReport).setOnClickListener {
            copyReport()
        }

        input.post {
            input.requestFocus()
            input.setSelection(input.text.length)
            showKeyboard()
        }
    }

    private val snapshotWatcher = object : TextWatcher {
        override fun beforeTextChanged(text: CharSequence?, start: Int, count: Int, after: Int) {
            changeStart = start
            removedUtf16 = count
            addedUtf16 = after
        }

        override fun onTextChanged(text: CharSequence?, start: Int, before: Int, count: Int) {
            changeStart = start
            removedUtf16 = before
            addedUtf16 = count
        }

        override fun afterTextChanged(editable: Editable) {
            sequence += 1
            val event = SnapshotEvent(
                sequence = sequence,
                elapsedMs = SystemClock.elapsedRealtime() - startedAtMs,
                kind = ChangeKind.from(removedUtf16, addedUtf16),
                startUtf16 = changeStart,
                removedUtf16 = removedUtf16,
                addedUtf16 = addedUtf16,
                selectionStart = input.selectionStart,
                selectionEnd = input.selectionEnd,
                composingStart = BaseInputConnection.getComposingSpanStart(editable),
                composingEnd = BaseInputConnection.getComposingSpanEnd(editable),
                text = editable.toString(),
            )
            events += event
            renderEvents()
        }
    }

    private fun renderEvents() {
        val latest = events.lastOrNull() ?: return
        summary.text = getString(
            R.string.event_summary,
            events.size,
            latest.kind.label,
            latest.text.length,
        )
        eventLog.text = events.asReversed().take(30).joinToString("\n", transform = SnapshotEvent::displayLine)
    }

    private fun copyReport() {
        if (events.isEmpty()) {
            Toast.makeText(this, R.string.empty_report, Toast.LENGTH_SHORT).show()
            return
        }
        val report = events.joinToString("\n", transform = SnapshotEvent::toJsonLine)
        val clipboard = getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
        clipboard.setPrimaryClip(ClipData.newPlainText("说写输入法验证报告", report))
        Toast.makeText(this, R.string.report_copied, Toast.LENGTH_SHORT).show()
    }

    private fun showKeyboard() {
        val inputMethodManager = getSystemService(Context.INPUT_METHOD_SERVICE) as InputMethodManager
        inputMethodManager.showSoftInput(input, InputMethodManager.SHOW_IMPLICIT)
    }

    fun resetForTest() {
        input.text.clear()
        events.clear()
        sequence = 0
        startedAtMs = SystemClock.elapsedRealtime()
        summary.setText(R.string.empty_report)
        eventLog.text = ""
    }

    fun snapshotEventsForTest(): List<SnapshotEvent> = events.toList()
}
