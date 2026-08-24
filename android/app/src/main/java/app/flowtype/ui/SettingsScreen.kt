package app.flowtype.ui

import android.provider.Settings
import android.view.View
import android.widget.Switch
import androidx.activity.ComponentActivity
import app.flowtype.R
import app.flowtype.FlowTypeApplication

/** Binds persistent settings to controls; permission orchestration stays in MainActivity. */
class SettingsScreen(
    private val activity: ComponentActivity,
    private val controller: FlowTypeApplication,
    private val preparePage: () -> Unit,
    private val applyInsets: () -> Unit,
    private val onBack: () -> Unit,
    private val onOpenComputers: () -> Unit,
    private val onFloatingToggle: (Boolean) -> Unit,
) {
    @Suppress("DEPRECATION")
    fun show() {
        preparePage()
        activity.setContentView(R.layout.page_settings)
        applyInsets()
        activity.findViewById<View>(R.id.back).setOnClickListener { onBack() }
        activity.findViewById<Switch>(R.id.autoSelectComputer).apply {
            isChecked = controller.settings.autoSelectComputer
            isEnabled = !controller.state().activeSession
            setOnCheckedChangeListener { _, checked -> controller.setAutoSelectComputer(checked) }
        }
        activity.findViewById<View>(R.id.autoSelectScope).apply {
            contentDescription = activity.getString(
                R.string.auto_select_scope,
                controller.bindings.autoSelectedIds().size,
            )
            setOnClickListener { onOpenComputers() }
        }
        activity.findViewById<Switch>(R.id.keepScreenOn).apply {
            isChecked = controller.settings.keepScreenOn
            setOnCheckedChangeListener { _, checked -> controller.settings.keepScreenOn = checked }
        }
        activity.findViewById<Switch>(R.id.extraDim).apply {
            isChecked = controller.settings.extraDim
            setOnCheckedChangeListener { _, checked -> controller.settings.extraDim = checked }
        }
        activity.findViewById<Switch>(R.id.floatingInput).apply {
            isChecked = controller.settings.floatingInput && Settings.canDrawOverlays(activity)
            setOnCheckedChangeListener { _, checked -> onFloatingToggle(checked) }
        }
    }
}
