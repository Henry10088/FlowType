package app.flowtype.ui

import android.provider.Settings
import android.view.View
import android.widget.Button
import android.widget.ProgressBar
import android.widget.Switch
import android.widget.TextView
import androidx.activity.ComponentActivity
import app.flowtype.R
import app.flowtype.FlowTypeApplication
import app.flowtype.update.UpdateManager

/** Binds persistent settings to controls; permission orchestration stays in MainActivity. */
class SettingsScreen(
    private val activity: ComponentActivity,
    private val controller: FlowTypeApplication,
    private val preparePage: () -> Unit,
    private val applyInsets: () -> Unit,
    private val onBack: () -> Unit,
    private val onOpenComputers: () -> Unit,
    private val onFloatingToggle: (Boolean) -> Unit,
    private val onInstallUpdate: () -> Unit,
    private val onOpenRelease: () -> Unit,
    private val isVisible: () -> Boolean,
) {
    private val updateObserver: (UpdateManager.State) -> Unit = { state ->
        activity.runOnUiThread { if (isVisible()) renderUpdate(state) }
    }

    init {
        controller.updates.observe(updateObserver)
    }

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
        controller.updates.refreshInstallAvailability()
        renderUpdate(controller.updates.state())
    }

    fun shutdown() {
        controller.updates.removeObserver(updateObserver)
    }

    private fun renderUpdate(state: UpdateManager.State) {
        val status = activity.findViewById<TextView>(R.id.updateStatus) ?: return
        val progress = activity.findViewById<ProgressBar>(R.id.updateProgress)
        val action = activity.findViewById<Button>(R.id.updateAction)
        val notes = activity.findViewById<Button>(R.id.updateNotes)
        status.text = state.message
        progress.visibility = if (state.showProgress) View.VISIBLE else View.GONE
        if (state.showProgress) {
            progress.max = 10_000
            progress.progress = ((state.downloaded.toDouble() / state.total) * progress.max)
                .toInt().coerceIn(0, progress.max)
        }
        action.visibility = if (state.action == UpdateManager.Action.NONE) View.GONE else View.VISIBLE
        action.text = state.actionLabel
        action.setOnClickListener {
            if (state.action == UpdateManager.Action.INSTALL) onInstallUpdate()
            else controller.updates.perform(state.action)
        }
        notes.visibility = if (state.releaseUrl != null) View.VISIBLE else View.GONE
        notes.setOnClickListener { onOpenRelease() }
    }
}
