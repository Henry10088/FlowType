package app.flowtype.ui

import android.graphics.Typeface
import android.view.View
import android.widget.LinearLayout
import android.widget.Switch
import android.widget.TextView
import androidx.activity.ComponentActivity
import app.flowtype.R
import app.flowtype.FlowTypeApplication

/** Renders computer bindings; mutations are delegated to the application coordinator. */
class ComputersScreen(
    private val activity: ComponentActivity,
    private val controller: FlowTypeApplication,
    private val preparePage: () -> Unit,
    private val applyInsets: () -> Unit,
    private val onBack: () -> Unit,
    private val onAdd: () -> Unit,
    private val onRename: (String, String) -> Unit,
    private val onUnbind: (String, String) -> Unit,
    private val onOpenInput: () -> Unit,
) {
    fun show() {
        preparePage()
        activity.setContentView(R.layout.page_computers)
        applyInsets()
        activity.findViewById<View>(R.id.back).setOnClickListener { onBack() }
        activity.findViewById<View>(R.id.addComputer).setOnClickListener { onAdd() }
        val list = activity.findViewById<LinearLayout>(R.id.computerList)
        val computers = controller.bindings.list()
        val state = controller.state()
        activity.findViewById<View>(R.id.empty).visibility =
            if (computers.isEmpty()) View.VISIBLE else View.GONE
        computers.forEach { binding ->
            val selected = binding.pcId == state.binding?.pcId
            val row = LinearLayout(activity).apply {
                orientation = LinearLayout.VERTICAL
                setPadding(0, activity.dp(16), 0, activity.dp(12))
                background = activity.getDrawable(R.drawable.row_background)
            }
            row.addView(LinearLayout(activity).apply {
                orientation = LinearLayout.HORIZONTAL
                gravity = android.view.Gravity.CENTER_VERTICAL
                addView(TextView(activity).apply {
                    text = binding.pcName
                    setTextColor(activity.getColor(R.color.text_primary))
                    textSize = 18f
                    setTypeface(typeface, Typeface.BOLD)
                    layoutParams = LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f)
                })
                addView(Switch(activity).apply {
                    text = activity.getString(R.string.auto_select_label)
                    textSize = 13f
                    setTextColor(activity.getColor(R.color.text_secondary))
                    isChecked = controller.isComputerAutoSelected(binding.pcId)
                    isEnabled = !state.activeSession
                    setOnClickListener {
                        controller.setComputerAutoSelected(binding.pcId, isChecked)
                    }
                })
            })
            row.addView(TextView(activity).apply {
                text = when {
                    selected && state.connected -> activity.getString(R.string.current_computer) + " · 在线"
                    binding.pcId in state.onlinePcIds -> "在线"
                    else -> "离线"
                }
                setTextColor(activity.getColor(if ((selected && state.connected) || binding.pcId in state.onlinePcIds) R.color.accent else R.color.text_secondary))
                textSize = 13f
                setPadding(0, activity.dp(4), 0, activity.dp(8))
            })
            row.addView(LinearLayout(activity).apply {
                orientation = LinearLayout.HORIZONTAL
                addView(activity.smallButton(R.string.rename) { onRename(binding.pcId, binding.pcName) })
                addView(activity.smallButton(R.string.unbind) { onUnbind(binding.pcId, binding.pcName) })
            })
            row.addView(activity.divider())
            row.setOnClickListener {
                if (selected) return@setOnClickListener
                controller.selectComputer(binding.pcId)
                onOpenInput()
            }
            list.addView(row)
        }
    }
}
