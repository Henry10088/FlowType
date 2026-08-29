package app.flowtype.ui

import android.content.Context
import android.graphics.Typeface
import android.graphics.drawable.GradientDrawable
import android.view.Gravity
import android.view.View
import android.widget.LinearLayout
import android.widget.TextView
import app.flowtype.FlowTypeController
import app.flowtype.R

fun renderComputerChooser(
    context: Context,
    chooser: LinearLayout,
    state: FlowTypeController.UiState,
    compact: Boolean,
    onSelect: (String) -> Unit,
) {
    fun dp(value: Int) = (value * context.resources.displayMetrics.density).toInt()

    chooser.removeAllViews()
    state.computers.forEach { binding ->
        val selected = binding.pcId == state.binding?.pcId
        val online = binding.pcId in state.onlinePcIds || (selected && state.connected)
        val active = binding.pcId == state.recentActivityPcId
        val chip = LinearLayout(context).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            setPadding(dp(if (compact) 10 else 12), 0, dp(if (compact) 10 else 12), 0)
            background = GradientDrawable().apply {
                cornerRadius = dp(6).toFloat()
                setColor(context.getColor(R.color.surface))
                setStroke(
                    dp(if (selected) 2 else 1),
                    context.getColor(if (selected) R.color.accent else R.color.divider),
                )
            }
            contentDescription = buildString {
                append(binding.pcName)
                if (selected) append(context.getString(R.string.a11y_selected))
                if (active) append(context.getString(R.string.a11y_recently_used))
                append(context.getString(if (online) R.string.a11y_connected else R.string.a11y_disconnected))
            }
            setOnClickListener { onSelect(binding.pcId) }
        }
        chip.addView(View(context).apply {
            background = GradientDrawable().apply {
                shape = GradientDrawable.OVAL
                setColor(context.getColor(if (online) R.color.accent else R.color.status_warning))
            }
        }, LinearLayout.LayoutParams(dp(8), dp(8)).apply {
            marginEnd = dp(if (compact) 6 else 8)
        })
        chip.addView(TextView(context).apply {
            text = binding.pcName
            setTextColor(context.getColor(R.color.text_primary))
            textSize = if (compact) 13f else 14f
            setTypeface(typeface, if (selected) Typeface.BOLD else Typeface.NORMAL)
        })
        if (active) {
            chip.addView(TextView(context).apply {
                text = if (compact) " \u2022" else "  \u2022"
                setTextColor(context.getColor(R.color.status_activity))
                textSize = 16f
                importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_NO
            })
        }
        chooser.addView(
            chip,
            LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.WRAP_CONTENT,
                dp(if (compact) 36 else 40),
            ).apply { marginEnd = dp(if (compact) 6 else 8) },
        )
    }
}
