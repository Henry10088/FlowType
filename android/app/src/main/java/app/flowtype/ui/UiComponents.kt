package app.flowtype.ui

import android.view.View
import android.view.ViewGroup
import android.widget.Button
import android.widget.LinearLayout
import androidx.activity.ComponentActivity
import app.flowtype.R
import java.text.DateFormat
import java.util.Date

fun ComponentActivity.dp(value: Int): Int =
    (value * resources.displayMetrics.density).toInt()

fun ComponentActivity.divider(): View = View(this).apply {
    setBackgroundColor(getColor(R.color.divider))
    layoutParams = LinearLayout.LayoutParams(
        ViewGroup.LayoutParams.MATCH_PARENT,
        dp(1),
    ).apply { topMargin = dp(16) }
}

fun ComponentActivity.smallButton(text: Int, action: () -> Unit): Button = Button(this).apply {
    setText(text)
    isAllCaps = false
    setTextColor(getColor(R.color.text_primary))
    textSize = 14f
    background = getDrawable(R.drawable.button_secondary)
    setOnClickListener { action() }
    layoutParams = LinearLayout.LayoutParams(0, dp(42), 1f).apply { marginEnd = dp(8) }
}

fun ComponentActivity.formatTime(time: Long): String =
    DateFormat.getDateTimeInstance(DateFormat.MEDIUM, DateFormat.SHORT).format(Date(time))
