package app.flowtype

import android.Manifest
import android.app.AlertDialog
import android.content.Intent
import android.content.pm.ActivityInfo
import android.graphics.Typeface
import android.graphics.drawable.GradientDrawable
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.SystemClock
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
import android.widget.LinearLayout
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
    private var imeWasVisible = false
    private var ignoreImeDismissUntil = 0L
    private lateinit var input: EditText
    private lateinit var sync: Button
    private lateinit var newSession: Button
    private lateinit var pair: Button
    private lateinit var status: TextView
    private lateinit var syncStatus: TextView
    private lateinit var computerName: TextView
    private var cameraImage: Uri? = null
    private val imageScreen by lazy {
        ImageScreen(
            activity = this,
            controller = controller,
            applyInsets = ::applySystemInsets,
            onBack = { showInput() },
            onCompleted = { showInput() },
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
        if (uri != null) showImagePreview(uri) else if (page == Screen.INPUT) showKeyboard()
    }
    private val camera = registerForActivityResult(ActivityResultContracts.TakePicture()) { saved ->
        val uri = cameraImage
        cameraImage = null
        if (saved && uri != null) showImagePreview(uri) else showKeyboard()
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
        sync = findViewById(R.id.sync)
        newSession = findViewById(R.id.newSession)
        pair = findViewById(R.id.pair)
        status = findViewById(R.id.status)
        syncStatus = findViewById(R.id.syncStatus)
        computerName = findViewById(R.id.computerName)
        input.addTextChangedListener(object : TextWatcher {
            override fun beforeTextChanged(value: CharSequence?, start: Int, count: Int, after: Int) = Unit
            override fun onTextChanged(value: CharSequence?, start: Int, before: Int, count: Int) = Unit
            override fun afterTextChanged(value: Editable?) {
                if (!suppressTextChange) controller.textChanged(value?.toString().orEmpty())
            }
        })
        sync.setOnClickListener { controller.sync() }
        newSession.setOnClickListener { controller.startNewSession() }
        pair.setOnClickListener { launchScanner() }
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
            .setNegativeButton(R.string.cancel) { _, _ -> showKeyboard() }
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
        syncStatus.text = state.syncState
        if (input.text.toString() != state.text) {
            suppressTextChange = true
            input.setText(state.text)
            input.setSelection(state.text.length)
            suppressTextChange = false
        }
        val paired = state.binding != null
        input.isEnabled = paired && !state.finishing
        sync.isEnabled = state.syncAvailable
        newSession.isEnabled = state.activeSession || state.text.isNotEmpty()
        pair.visibility = if (paired) View.GONE else View.VISIBLE
        findViewById<ImageButton>(R.id.openImage).isEnabled =
            paired && state.imageTransfer != FlowTypeApplication.ImageTransferState.SENDING
        status.setTextColor(getColor(
            when {
                state.binding == null -> R.color.status_neutral
                state.connected -> R.color.accent
                else -> R.color.status_warning
            },
        ))
        renderComputerChooser(state)
    }

    private fun renderComputerChooser(state: FlowTypeApplication.UiState) {
        val chooser = findViewById<LinearLayout>(R.id.computerChooser) ?: return
        chooser.removeAllViews()
        controller.bindings.list().forEach { binding ->
            val selected = binding.pcId == state.binding?.pcId
            val online = binding.pcId in state.onlinePcIds || (selected && state.connected)
            val active = binding.pcId == state.recentActivityPcId
            val chip = LinearLayout(this).apply {
                orientation = LinearLayout.HORIZONTAL
                gravity = android.view.Gravity.CENTER_VERTICAL
                setPadding(dp(12), 0, dp(12), 0)
                background = GradientDrawable().apply {
                    cornerRadius = dp(6).toFloat()
                    setColor(getColor(R.color.surface))
                    setStroke(dp(if (selected) 2 else 1), getColor(if (selected) R.color.accent else R.color.divider))
                }
                contentDescription = buildString {
                    append(binding.pcName)
                    append(if (selected) "，已选择" else "")
                    append(if (active) "，最近有鼠标活动" else "")
                    append(if (online) "，已连接" else "，未连接")
                }
                setOnClickListener { controller.selectComputer(binding.pcId) }
            }
            chip.addView(View(this).apply {
                background = GradientDrawable().apply {
                    shape = GradientDrawable.OVAL
                    setColor(getColor(if (online) R.color.accent else R.color.status_warning))
                }
            }, LinearLayout.LayoutParams(dp(8), dp(8)).apply { marginEnd = dp(8) })
            chip.addView(TextView(this).apply {
                text = binding.pcName
                setTextColor(getColor(R.color.text_primary))
                textSize = 14f
                setTypeface(typeface, if (selected) Typeface.BOLD else Typeface.NORMAL)
            })
            if (active) {
                chip.addView(TextView(this).apply {
                    text = "  •"
                    setTextColor(getColor(R.color.status_activity))
                    textSize = 16f
                    contentDescription = "最近鼠标活动"
                })
            }
            chooser.addView(chip, LinearLayout.LayoutParams(LinearLayout.LayoutParams.WRAP_CONTENT, dp(40)).apply {
                marginEnd = dp(8)
            })
        }
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
            Toast.makeText(this, R.string.operation_requires_new_session, Toast.LENGTH_SHORT).show()
            return
        }
        AlertDialog.Builder(this)
            .setMessage(getString(R.string.confirm_unbind, name))
            .setNegativeButton(R.string.cancel, null)
            .setPositiveButton(R.string.unbind) { _, _ -> controller.unbindComputer(pcId); showComputers() }
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
        imeWasVisible = false
        input.postDelayed({
            getSystemService(InputMethodManager::class.java).showSoftInput(input, InputMethodManager.SHOW_IMPLICIT)
        }, 250)
    }

    private fun hideKeyboard() {
        ignoreImeDismissUntil = SystemClock.uptimeMillis() + 1_000L
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
            val imeVisible = insets.isVisible(WindowInsetsCompat.Type.ime())
            if (page == Screen.INPUT && imeWasVisible && !imeVisible &&
                SystemClock.uptimeMillis() > ignoreImeDismissUntil
            ) {
                exitInputSurface()
            }
            imeWasVisible = imeVisible
            val bottomInset = maxOf(bars.bottom, ime.bottom)
            view.setPaddingRelative(start + bars.left, top + bars.top, end + bars.right, bottom + bottomInset)
            insets
        }
        ViewCompat.requestApplyInsets(root)
    }

    private fun exitInputSurface() {
        if (page != Screen.INPUT || isFinishing || isChangingConfigurations) return
        controller.saveNow()
        clearInputWindowSettings()
        if (controller.settings.floatingInput && Settings.canDrawOverlays(this)) {
            FloatingInputService.show(this)
        }
        finish()
    }

    private fun dp(value: Int): Int = (value * resources.displayMetrics.density).toInt()

    companion object {
        const val ACTION_OPEN_IMAGE = "app.flowtype.OPEN_IMAGE"
        private const val NOTIFICATION_PERMISSION = 10
    }
}
