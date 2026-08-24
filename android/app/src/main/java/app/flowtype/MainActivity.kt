package app.flowtype

import android.Manifest
import android.app.AlertDialog
import android.content.Intent
import android.content.pm.ActivityInfo
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.provider.Settings
import android.text.Editable
import android.text.TextWatcher
import android.view.View
import android.view.ViewGroup
import android.view.WindowManager
import android.view.inputmethod.InputMethodManager
import android.widget.Button
import android.widget.EditText
import android.widget.ImageButton
import android.widget.TextView
import android.widget.Toast
import androidx.activity.ComponentActivity
import androidx.activity.OnBackPressedCallback
import androidx.activity.result.contract.ActivityResultContracts
import androidx.core.content.FileProvider
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import app.flowtype.floating.FloatingInputService
import app.flowtype.pairing.BindingStore
import app.flowtype.ui.ComputersScreen
import app.flowtype.ui.HistoryScreen
import app.flowtype.ui.ImageScreen
import app.flowtype.ui.Screen
import app.flowtype.ui.SettingsScreen
import com.journeyapps.barcodescanner.ScanContract
import com.journeyapps.barcodescanner.ScanOptions
import java.io.File

class MainActivity : ComponentActivity() {
    private val controller by lazy { application as FlowTypeApplication }
    private var page = Screen.INPUT
    private var suppressTextChange = false
    private var scanLaunched = false
    private var waitingForOverlayPermission = false
    private lateinit var input: EditText
    private lateinit var finish: Button
    private lateinit var abandonFinish: Button
    private lateinit var syncCurrentCursor: Button
    private lateinit var syncCurrentCursorFinishing: Button
    private lateinit var pair: Button
    private lateinit var syncFullText: Button
    private lateinit var resetSession: ImageButton
    private lateinit var status: TextView
    private lateinit var computerName: TextView
    private var cameraImage: Uri? = null
    private val imageScreen by lazy {
        ImageScreen(
            activity = this,
            controller = controller,
            applyInsets = ::applySystemInsets,
            onBack = { showInput(focus = false) },
            onCompleted = { showInput(focus = false) },
            isVisible = { page == Screen.IMAGE },
        )
    }
    private val historyScreen by lazy {
        HistoryScreen(
            activity = this,
            controller = controller,
            preparePage = { hideKeyboard(); clearInputWindowSettings() },
            applyInsets = ::applySystemInsets,
            onBack = { showInput() },
            onOpenInput = { focus -> showInput(focus) },
            onOpenDetail = ::showHistoryDetail,
        )
    }
    private val computersScreen by lazy {
        ComputersScreen(
            activity = this,
            controller = controller,
            preparePage = { hideKeyboard(); clearInputWindowSettings() },
            applyInsets = ::applySystemInsets,
            onBack = { showInput() },
            onAdd = ::launchScanner,
            onRename = ::renameComputer,
            onUnbind = ::confirmUnbind,
            onOpenInput = { showInput() },
        )
    }
    private val settingsScreen by lazy {
        SettingsScreen(
            activity = this,
            controller = controller,
            preparePage = { hideKeyboard(); clearInputWindowSettings() },
            applyInsets = ::applySystemInsets,
            onBack = { showInput() },
            onOpenComputers = ::showComputers,
            onFloatingToggle = { checked ->
                if (checked) requestFloatingPermission() else {
                    controller.settings.floatingInput = false
                    FloatingInputService.stop(this)
                }
            },
        )
    }
    private val observer: (FlowTypeApplication.UiState) -> Unit = { state ->
        when (page) {
            Screen.INPUT -> renderInput(state)
            Screen.IMAGE -> imageScreen.render(state)
            else -> Unit
        }
    }
    private val scanner = registerForActivityResult(ScanContract()) { result ->
        scanLaunched = false
        result.contents?.let(::acceptPairingValue)
    }
    private val gallery = registerForActivityResult(ActivityResultContracts.GetContent()) { uri ->
        uri?.let(::showImagePreview)
    }
    private val camera = registerForActivityResult(ActivityResultContracts.TakePicture()) { saved ->
        val uri = cameraImage
        cameraImage = null
        if (saved && uri != null) showImagePreview(uri)
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        requestedOrientation = ActivityInfo.SCREEN_ORIENTATION_PORTRAIT
        onBackPressedDispatcher.addCallback(this, object : OnBackPressedCallback(true) {
            override fun handleOnBackPressed() {
                if (page == Screen.INPUT) moveTaskToBack(true) else showInput(focus = false)
            }
        })
        if (!handlePairingIntent(intent)) {
            showInput(focus = intent.action != ACTION_OPEN_IMAGE)
            if (intent.action == ACTION_OPEN_IMAGE) chooseImageSource()
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        if (!handlePairingIntent(intent) && intent.action == ACTION_OPEN_IMAGE) {
            showInput(focus = false)
            chooseImageSource()
        }
    }

    override fun onDestroy() {
        imageScreen.shutdown()
        super.onDestroy()
    }

    override fun onStart() {
        super.onStart()
        controller.observe(observer)
        FloatingInputService.hide(this)
    }

    override fun onStop() {
        controller.saveNow()
        controller.removeObserver(observer)
        if (controller.settings.floatingInput) FloatingInputService.show(this)
        super.onStop()
    }

    override fun onResume() {
        super.onResume()
        controller.ensureConnected()
        if (waitingForOverlayPermission) {
            waitingForOverlayPermission = false
            if (Settings.canDrawOverlays(this)) enableFloating() else controller.settings.floatingInput = false
            if (page == Screen.SETTINGS) showSettings()
        }
    }

    override fun onWindowFocusChanged(hasFocus: Boolean) {
        super.onWindowFocusChanged(hasFocus)
        if (hasFocus && page == Screen.INPUT && ::input.isInitialized && input.isEnabled) showKeyboard()
    }

    @Suppress("DEPRECATION")
    private fun showInput(focus: Boolean = true) {
        page = Screen.INPUT
        setContentView(R.layout.activity_main)
        applySystemInsets()
        window.setSoftInputMode(
            WindowManager.LayoutParams.SOFT_INPUT_ADJUST_RESIZE or
                WindowManager.LayoutParams.SOFT_INPUT_STATE_ALWAYS_VISIBLE,
        )
        input = findViewById(R.id.input)
        finish = findViewById(R.id.finish)
        abandonFinish = findViewById(R.id.abandonFinish)
        syncCurrentCursor = findViewById(R.id.syncCurrentCursor)
        syncCurrentCursorFinishing = findViewById(R.id.syncCurrentCursorFinishing)
        pair = findViewById(R.id.pair)
        syncFullText = findViewById(R.id.syncFullText)
        resetSession = findViewById(R.id.resetSession)
        status = findViewById(R.id.status)
        computerName = findViewById(R.id.computerName)
        input.addTextChangedListener(object : TextWatcher {
            override fun beforeTextChanged(value: CharSequence?, start: Int, count: Int, after: Int) = Unit
            override fun onTextChanged(value: CharSequence?, start: Int, before: Int, count: Int) = Unit
            override fun afterTextChanged(value: Editable?) {
                if (!suppressTextChange) controller.textChanged(value?.toString().orEmpty())
            }
        })
        finish.setOnClickListener { controller.finish() }
        abandonFinish.setOnClickListener { confirmAbandonSync() }
        syncCurrentCursor.setOnClickListener { controller.syncToCurrentCursor() }
        syncCurrentCursorFinishing.setOnClickListener { controller.syncToCurrentCursor() }
        pair.setOnClickListener { launchScanner() }
        syncFullText.setOnClickListener { controller.syncFullText() }
        resetSession.setOnClickListener { confirmResetSession() }
        findViewById<ImageButton>(R.id.openHistory).setOnClickListener { showHistory() }
        findViewById<ImageButton>(R.id.openComputers).setOnClickListener { showComputers() }
        findViewById<ImageButton>(R.id.toggleDim).setOnClickListener {
            controller.settings.extraDim = !controller.settings.extraDim
            applyInputWindowSettings()
        }
        findViewById<ImageButton>(R.id.openSettings).setOnClickListener { showSettings() }
        findViewById<ImageButton>(R.id.openImage).setOnClickListener { chooseImageSource() }
        renderInput(controller.state())
        applyInputWindowSettings()
        if (focus) showKeyboard()
    }

    private fun chooseImageSource() {
        hideKeyboard()
        AlertDialog.Builder(this)
            .setTitle(R.string.choose_image_source)
            .setItems(arrayOf(getString(R.string.take_photo), getString(R.string.choose_from_gallery))) { _, which ->
                if (which == 0) launchCamera() else gallery.launch("image/*")
            }
            .setNegativeButton(R.string.cancel, null)
            .show()
    }

    private fun launchCamera() {
        val directory = File(cacheDir, "camera").apply { mkdirs() }
        val file = File.createTempFile("flowtype-", ".jpg", directory)
        val uri = FileProvider.getUriForFile(this, "$packageName.files", file)
        cameraImage = uri
        camera.launch(uri)
    }

    private fun showImagePreview(uri: Uri) {
        page = Screen.IMAGE
        hideKeyboard()
        clearInputWindowSettings()
        imageScreen.show(uri)
    }

    private fun renderInput(state: FlowTypeApplication.UiState) {
        computerName.text = state.binding?.pcName ?: getString(R.string.default_computer)
        status.text = state.status
        if (input.text.toString() != state.text) {
            suppressTextChange = true
            input.setText(state.text)
            input.setSelection(state.text.length)
            suppressTextChange = false
        }
        val paired = state.binding != null
        input.isEnabled = paired && !state.finishing
        finish.isEnabled = state.activeSession && !state.finishing
        finish.text = if (state.finishing) getString(R.string.finish_waiting) else getString(R.string.finish)
        findViewById<View>(R.id.primaryActions).visibility = if (state.finishing) View.GONE else View.VISIBLE
        findViewById<View>(R.id.finishingActions).visibility = if (state.finishing) View.VISIBLE else View.GONE
        // Keep the action's slot stable while a session is active. Voice
        // recognition can alternate acknowledged and pending snapshots many
        // times per second; visibility must not follow that transient state.
        syncCurrentCursor.visibility = if (!state.finishing && state.activeSession) View.VISIBLE else View.GONE
        syncCurrentCursor.isEnabled = state.syncCurrentCursorAvailable
        syncCurrentCursorFinishing.visibility = if (state.finishing) View.VISIBLE else View.GONE
        syncCurrentCursorFinishing.isEnabled = state.syncCurrentCursorAvailable
        resetSession.isEnabled = state.activeSession || state.text.isNotEmpty()
        pair.visibility = if (paired) View.GONE else View.VISIBLE
        syncFullText.visibility = if (paired && state.showSyncFullText && !state.finishing) View.VISIBLE else View.GONE
        findViewById<ImageButton>(R.id.openImage).isEnabled =
            paired && state.imageTransfer != FlowTypeApplication.ImageTransferState.SENDING
    }

    private fun showHistory() {
        page = Screen.HISTORY
        historyScreen.show()
    }

    private fun showHistoryDetail(id: Long) {
        page = Screen.DETAIL
        historyScreen.showDetail(id)
    }

    private fun showComputers() {
        page = Screen.COMPUTERS
        computersScreen.show()
    }

    @Suppress("DEPRECATION")
    private fun showSettings() {
        page = Screen.SETTINGS
        settingsScreen.show()
    }

    private fun renameComputer(pcId: String, oldName: String) {
        val input = EditText(this).apply { setText(oldName); selectAll() }
        AlertDialog.Builder(this)
            .setTitle(R.string.rename_computer)
            .setView(input)
            .setNegativeButton(R.string.cancel, null)
            .setPositiveButton(R.string.save) { _, _ ->
                if (input.text.isNotBlank()) controller.renameComputer(pcId, input.text.toString())
                showComputers()
            }.show()
    }

    private fun confirmUnbind(pcId: String, name: String) {
        if (controller.state().activeSession && controller.state().binding?.pcId == pcId) {
            Toast.makeText(this, R.string.switch_after_finish, Toast.LENGTH_SHORT).show()
            return
        }
        AlertDialog.Builder(this)
            .setMessage(getString(R.string.confirm_unbind, name))
            .setNegativeButton(R.string.cancel, null)
            .setPositiveButton(R.string.unbind) { _, _ -> controller.unbindComputer(pcId); showComputers() }
            .show()
    }

    private fun confirmAbandonSync() {
        AlertDialog.Builder(this)
            .setTitle(R.string.abandon_sync_title)
            .setMessage(R.string.abandon_sync_message)
            .setNegativeButton(R.string.continue_waiting, null)
            .setPositiveButton(R.string.abandon_sync) { _, _ -> controller.abandonSyncAndFinish() }
            .show()
    }

    private fun confirmResetSession() {
        AlertDialog.Builder(this)
            .setTitle(R.string.reset_session_title)
            .setMessage(R.string.reset_session_message)
            .setNegativeButton(R.string.cancel, null)
            .setPositiveButton(R.string.reset_session) { _, _ -> controller.resetSession() }
            .show()
    }

    private fun requestFloatingPermission() {
        if (Settings.canDrawOverlays(this)) return enableFloating()
        AlertDialog.Builder(this)
            .setTitle(R.string.floating_permission_title)
            .setMessage(R.string.floating_permission_message)
            .setNegativeButton(R.string.cancel) { _, _ -> showSettings() }
            .setPositiveButton(R.string.open_settings) { _, _ ->
                waitingForOverlayPermission = true
                startActivity(Intent(Settings.ACTION_MANAGE_OVERLAY_PERMISSION, Uri.parse("package:$packageName")))
            }.show()
    }

    private fun enableFloating() {
        controller.settings.floatingInput = true
        if (Build.VERSION.SDK_INT >= 33 && checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != android.content.pm.PackageManager.PERMISSION_GRANTED) {
            requestPermissions(arrayOf(Manifest.permission.POST_NOTIFICATIONS), NOTIFICATION_PERMISSION)
        }
        FloatingInputService.start(this)
    }

    private fun launchScanner() {
        if (scanLaunched) return
        scanLaunched = true
        scanner.launch(
            ScanOptions()
                .setDesiredBarcodeFormats(ScanOptions.QR_CODE)
                .setPrompt(getString(R.string.scan_prompt))
                .setBeepEnabled(false)
                .setCaptureActivity(PortraitCaptureActivity::class.java)
                .setOrientationLocked(true),
        )
    }

    private fun acceptPairingValue(value: String) {
        runCatching { BindingStore.parse(value) }
            .onSuccess { controller.acceptBinding(it); showInput() }
            .onFailure {
                Toast.makeText(this, R.string.status_pair_failed, Toast.LENGTH_LONG).show()
                if (page == Screen.INPUT) renderInput(controller.state())
            }
    }

    private fun handlePairingIntent(intent: Intent): Boolean {
        val value = intent.dataString ?: return false
        if (!value.startsWith("flowtype://pair")) return false
        acceptPairingValue(value)
        return true
    }

    private fun applyInputWindowSettings() {
        if (controller.settings.keepScreenOn) window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        else window.clearFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        window.attributes = window.attributes.apply { screenBrightness = if (controller.settings.extraDim) 0.02f else -1f }
        findViewById<ImageButton>(R.id.toggleDim)?.alpha = if (controller.settings.extraDim) 1f else 0.65f
    }

    private fun clearInputWindowSettings() {
        window.clearFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        window.attributes = window.attributes.apply { screenBrightness = -1f }
    }

    private fun showKeyboard() {
        input.requestFocus()
        input.postDelayed({
            getSystemService(InputMethodManager::class.java).showSoftInput(input, InputMethodManager.SHOW_IMPLICIT)
        }, 250)
    }

    private fun hideKeyboard() {
        currentFocus?.let { getSystemService(InputMethodManager::class.java).hideSoftInputFromWindow(it.windowToken, 0) }
    }

    private fun applySystemInsets() {
        val root = findViewById<ViewGroup>(android.R.id.content).getChildAt(0)
        val start = root.paddingStart
        val top = root.paddingTop
        val end = root.paddingEnd
        val bottom = root.paddingBottom
        ViewCompat.setOnApplyWindowInsetsListener(root) { view, insets ->
            val bars = insets.getInsets(WindowInsetsCompat.Type.systemBars())
            val ime = insets.getInsets(WindowInsetsCompat.Type.ime())
            val bottomInset = maxOf(bars.bottom, ime.bottom)
            view.setPaddingRelative(start + bars.left, top + bars.top, end + bars.right, bottom + bottomInset)
            insets
        }
        ViewCompat.requestApplyInsets(root)
    }

    companion object {
        const val ACTION_OPEN_IMAGE = "app.flowtype.OPEN_IMAGE"
        private const val NOTIFICATION_PERMISSION = 10
    }
}
