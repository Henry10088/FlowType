package app.flowtype.ui

import android.content.ClipData
import android.content.ClipboardManager
import android.graphics.Typeface
import android.view.Gravity
import android.view.View
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
        entries.forEach { list.addView(historyRow(it)) }
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
                Toast.makeText(activity, R.string.switch_after_finish, Toast.LENGTH_SHORT).show()
            }
        }
        activity.findViewById<View>(R.id.delete).setOnClickListener {
            controller.history.delete(entry.id)
            onBack()
        }
    }

    private fun historyRow(entry: HistoryEntry): View = LinearLayout(activity).apply {
        orientation = LinearLayout.VERTICAL
        gravity = Gravity.CENTER_VERTICAL
        setPadding(0, activity.dp(16), 0, activity.dp(16))
        background = activity.getDrawable(R.drawable.row_background)
        addView(TextView(context).apply {
            text = entry.pcName
            setTextColor(activity.getColor(R.color.text_primary))
            textSize = 18f
            setTypeface(typeface, Typeface.BOLD)
        })
        addView(TextView(context).apply {
            text = activity.formatTime(entry.completedAt)
            setTextColor(activity.getColor(R.color.accent))
            textSize = 13f
            setPadding(0, activity.dp(4), 0, activity.dp(6))
        })
        addView(TextView(context).apply {
            text = entry.text
            maxLines = 2
            setTextColor(activity.getColor(R.color.text_secondary))
            textSize = 16f
        })
        addView(activity.divider())
        setOnClickListener { onOpenDetail(entry.id) }
    }
}
