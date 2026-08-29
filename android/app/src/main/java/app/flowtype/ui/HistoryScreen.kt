package app.flowtype.ui

import android.content.ClipData
import android.content.ClipboardManager
import android.app.AlertDialog
import android.graphics.Typeface
import android.view.Gravity
import android.view.View
import android.widget.Button
import android.widget.CheckBox
import android.widget.LinearLayout
import android.widget.TextView
import android.widget.Toast
import androidx.activity.ComponentActivity
import app.flowtype.R
import app.flowtype.FlowTypeApplication
import app.flowtype.data.HistoryEntry

/** Renders persisted input history without owning session or navigation state. */
class HistoryScreen(
    private val activity: ComponentActivity,
    private val controller: FlowTypeApplication,
    private val preparePage: () -> Unit,
    private val applyInsets: () -> Unit,
    private val onBack: () -> Unit,
    private val onOpenInput: (Boolean) -> Unit,
    private val onOpenDetail: (Long) -> Unit,
) {
    fun show() {
        preparePage()
        activity.setContentView(R.layout.page_history)
        applyInsets()
        activity.findViewById<View>(R.id.back).setOnClickListener { onBack() }
        val list = activity.findViewById<LinearLayout>(R.id.historyList)
        val entries = controller.history.list()
        activity.findViewById<View>(R.id.empty).visibility =
            if (entries.isEmpty()) View.VISIBLE else View.GONE
        val selected = linkedSetOf<Long>()
        val selectedCount = activity.findViewById<TextView>(R.id.selectedCount)
        val selectAll = activity.findViewById<Button>(R.id.selectAll)
        val deleteSelected = activity.findViewById<View>(R.id.deleteSelected)
        val selectButton = activity.findViewById<View>(R.id.selectHistory)
        var selecting = false

        fun renderRows() {
            list.removeAllViews()
            entries.forEach { entry ->
                list.addView(historyRow(entry, selecting, selected.contains(entry.id)) {
                    if (selected.contains(entry.id)) selected.remove(entry.id) else selected.add(entry.id)
                    renderRows()
                })
            }
            selectedCount.visibility = if (selecting) View.VISIBLE else View.GONE
            selectedCount.text = activity.getString(R.string.selected_count, selected.size)
            selectAll.visibility = if (selecting && entries.isNotEmpty()) View.VISIBLE else View.GONE
            if (selecting && entries.isNotEmpty()) {
                selectAll.setText(
                    if (selected.isEmpty()) R.string.select_all else R.string.invert_selection,
                )
            }
            deleteSelected.visibility = if (selecting) View.VISIBLE else View.GONE
            deleteSelected.isEnabled = selected.isNotEmpty()
            selectButton.contentDescription = activity.getString(
                if (selecting) R.string.cancel else R.string.select_history,
            )
        }

        selectButton.setOnClickListener {
            selecting = !selecting
            selected.clear()
            renderRows()
        }
        selectAll.setOnClickListener {
            if (selected.isEmpty()) {
                selected += entries.map { it.id }
            } else {
                val current = selected.toSet()
                selected.clear()
                selected += entries.map { it.id }.filterNot(current::contains)
            }
            renderRows()
        }
        deleteSelected.setOnClickListener {
            if (selected.isEmpty()) return@setOnClickListener
            AlertDialog.Builder(activity)
                .setMessage(activity.getString(R.string.confirm_delete_history, selected.size))
                .setNegativeButton(R.string.cancel, null)
                .setPositiveButton(R.string.delete) { _, _ ->
                    controller.history.delete(selected)
                    show()
                }
                .show()
        }
        renderRows()
    }

    fun showDetail(id: Long) {
        val entry = controller.history.get(id) ?: return onBack()
        activity.setContentView(R.layout.page_history_detail)
        applyInsets()
        activity.findViewById<View>(R.id.back).setOnClickListener { onBack() }
        activity.findViewById<TextView>(R.id.detailComputer).text = entry.pcName
        activity.findViewById<TextView>(R.id.detailTime).text = activity.formatTime(entry.completedAt)
        activity.findViewById<TextView>(R.id.detailText).text = entry.text
        activity.findViewById<View>(R.id.copy).setOnClickListener {
            activity.getSystemService(ClipboardManager::class.java).setPrimaryClip(
                ClipData.newPlainText(activity.getString(R.string.app_name), entry.text),
            )
            Toast.makeText(activity, R.string.copied, Toast.LENGTH_SHORT).show()
        }
        activity.findViewById<View>(R.id.useAsNew).setOnClickListener {
            if (controller.replaceWithHistory(entry.text)) onOpenInput(true) else {
                Toast.makeText(activity, R.string.operation_requires_new_session, Toast.LENGTH_SHORT).show()
            }
        }
        activity.findViewById<View>(R.id.delete).setOnClickListener {
            controller.history.delete(entry.id)
            onBack()
        }
    }

    private fun historyRow(
        entry: HistoryEntry,
        selecting: Boolean,
        checked: Boolean,
        onToggle: () -> Unit,
    ): View = LinearLayout(activity).apply {
        orientation = LinearLayout.HORIZONTAL
        gravity = Gravity.CENTER_VERTICAL
        setPadding(0, activity.dp(10), 0, activity.dp(10))
        background = activity.getDrawable(R.drawable.row_background)
        val checkbox = CheckBox(activity).apply {
            isChecked = checked
            visibility = if (selecting) View.VISIBLE else View.GONE
            contentDescription = entry.pcName
            setOnClickListener { onToggle() }
        }
        addView(checkbox, LinearLayout.LayoutParams(activity.dp(48), activity.dp(56)))
        val body = LinearLayout(activity).apply {
            orientation = LinearLayout.VERTICAL
            gravity = Gravity.CENTER_VERTICAL
            setPadding(activity.dp(4), activity.dp(6), 0, activity.dp(6))
            addView(TextView(activity).apply {
                text = entry.pcName
                setTextColor(activity.getColor(R.color.text_primary))
                textSize = 18f
                setTypeface(typeface, Typeface.BOLD)
            })
            addView(TextView(activity).apply {
                text = activity.formatTime(entry.completedAt)
                setTextColor(activity.getColor(R.color.accent))
                textSize = 13f
                setPadding(0, activity.dp(4), 0, activity.dp(6))
            })
            addView(TextView(activity).apply {
                text = entry.text
                maxLines = 2
                setTextColor(activity.getColor(R.color.text_secondary))
                textSize = 16f
            })
        }
        addView(body, LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f))
        setOnClickListener { if (selecting) onToggle() else onOpenDetail(entry.id) }
    }
}
