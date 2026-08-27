package app.flowtype.floating

import android.annotation.SuppressLint
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.res.Configuration
import android.graphics.PixelFormat
import android.graphics.Typeface
import android.graphics.drawable.GradientDrawable
import android.os.IBinder
import android.provider.Settings
import android.text.Editable
import android.text.TextWatcher
import android.util.TypedValue
import android.view.GestureDetector
import android.view.Gravity
import android.view.MotionEvent
import android.view.View
import android.view.WindowManager
import android.view.inputmethod.InputMethodManager
import android.widget.Button
import android.widget.EditText
import android.widget.FrameLayout
import android.widget.ImageButton
import android.widget.ImageView
import android.widget.HorizontalScrollView
import android.widget.LinearLayout
import android.widget.TextView
import androidx.core.app.NotificationCompat
import androidx.core.content.ContextCompat
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import app.flowtype.MainActivity
import app.flowtype.R
import app.flowtype.FlowTypeApplication
import kotlin.math.abs

class FloatingInputService : Service() {
    private lateinit var windowManager: WindowManager
    private lateinit var controller: FlowTypeApplication
    private var ball: View? = null
    private var panel: View? = null
    private var closeTarget: View? = null
    private var ballParams: WindowManager.LayoutParams? = null
    private var hiddenByActivity = false
    private var suppressTextChange = false
    private var panelInput: EditText? = null
    private var panelStatus: TextView? = null
    private var panelSyncStatus: TextView? = null
    private var panelComputer: TextView? = null
    private var panelSync: Button? = null
    private var panelNewSession: Button? = null
    private var panelOpenImage: ImageButton? = null
    private var panelChooser: HorizontalScrollView? = null
    private var panelImeVisible = false
    private val observer: (FlowTypeApplication.UiState) -> Unit = { render(it) }

    override fun onCreate() {
        super.onCreate()
        controller = application as FlowTypeApplication
        windowManager = getSystemService(WindowManager::class.java)
        createNotificationChannel()
        controller.observe(observer)
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        val action = intent?.action ?: ACTION_START
        if (action != ACTION_STOP) controller.ensureConnected()
        if (action != ACTION_STOP && controller.settings.floatingInput) {
            startForeground(NOTIFICATION_ID, notification())
        }
        when (action) {
            ACTION_STOP -> {
                controller.settings.floatingInput = false
                stopSelf()
                return START_NOT_STICKY
            }
            ACTION_HIDE -> {
                hiddenByActivity = true
                removeOverlays()
            }
            ACTION_SHOW -> {
                hiddenByActivity = false
                if (controller.settings.floatingInput) showBall()
            }
            else -> {
                if (!hiddenByActivity) showBall()
            }
        }
        return START_STICKY
    }

    override fun onDestroy() {
        controller.removeObserver(observer)
        removeOverlays()
        super.onDestroy()
    }

    override fun onBind(intent: Intent?): IBinder? = null

    private fun showBall() {
        if (ball != null || hiddenByActivity || !Settings.canDrawOverlays(this)) return
        val size = dp(56)
        val view = FrameLayout(this).apply {
            alpha = 0.58f
            background = GradientDrawable().apply {
                shape = GradientDrawable.OVAL
                setColor(getColor(R.color.surface))
                setStroke(dp(1), getColor(R.color.divider))
            }
            addView(ImageView(context).apply {
                setImageResource(android.R.drawable.ic_menu_edit)
                imageTintList = getColorStateList(R.color.text_primary)
                contentDescription = getString(R.string.floating_input)
                setPadding(dp(14), dp(14), dp(14), dp(14))
            }, FrameLayout.LayoutParams(size, size))
            addView(View(context).apply {
                background = GradientDrawable().apply {
                    shape = GradientDrawable.OVAL
                    setColor(getColor(R.color.accent))
                }
            }, FrameLayout.LayoutParams(dp(9), dp(9), Gravity.END or Gravity.BOTTOM).apply {
                marginEnd = dp(6)
                bottomMargin = dp(6)
            })
        }
        val params = WindowManager.LayoutParams(
            size,
            size,
            WindowManager.LayoutParams.TYPE_APPLICATION_OVERLAY,
            WindowManager.LayoutParams.FLAG_NOT_FOCUSABLE or WindowManager.LayoutParams.FLAG_LAYOUT_NO_LIMITS,
            PixelFormat.TRANSLUCENT,
        ).apply {
            gravity = Gravity.TOP or Gravity.START
            val portrait = resources.configuration.orientation == Configuration.ORIENTATION_PORTRAIT
            x = preferences().getInt(if (portrait) "ball_x_portrait" else "ball_x_landscape", dp(8))
            y = preferences().getInt(if (portrait) "ball_y_portrait" else "ball_y_landscape", dp(180))
        }
        attachBallGestures(view, params)
        runCatching { windowManager.addView(view, params) }.onSuccess {
            ball = view
            ballParams = params
        }.onFailure { stopSelf() }
    }

    @SuppressLint("ClickableViewAccessibility")
    private fun attachBallGestures(view: View, params: WindowManager.LayoutParams) {
        val gestures = GestureDetector(this, object : GestureDetector.SimpleOnGestureListener() {
            override fun onDown(event: MotionEvent): Boolean = true
            override fun onSingleTapConfirmed(event: MotionEvent): Boolean { view.performClick(); return true }
            override fun onDoubleTap(event: MotionEvent): Boolean { openFullApp(); return true }
        })
        view.setOnClickListener { showPanel() }
        var downX = 0f
        var downY = 0f
        var startX = 0
        var startY = 0
        var dragging = false
        view.setOnTouchListener { _, event ->
            gestures.onTouchEvent(event)
            when (event.actionMasked) {
                MotionEvent.ACTION_DOWN -> {
                    downX = event.rawX
                    downY = event.rawY
                    startX = params.x
                    startY = params.y
                    view.alpha = 0.92f
                }
                MotionEvent.ACTION_MOVE -> {
                    if (abs(event.rawX - downX) > dp(6) || abs(event.rawY - downY) > dp(6)) dragging = true
                    if (dragging) {
                        showCloseTarget()
                        params.x = startX + (event.rawX - downX).toInt()
                        params.y = startY + (event.rawY - downY).toInt()
                        windowManager.updateViewLayout(view, params)
                    }
                }
                MotionEvent.ACTION_UP, MotionEvent.ACTION_CANCEL -> {
                    view.alpha = 0.58f
                    if (dragging) {
                        val metrics = resources.displayMetrics
                        if (event.rawY > metrics.heightPixels - dp(120)) {
                            controller.settings.floatingInput = false
                            stopSelf()
                        } else {
                            params.x = if (params.x + view.width / 2 < metrics.widthPixels / 2) dp(8) else metrics.widthPixels - view.width - dp(8)
                            params.y = params.y.coerceIn(dp(32), metrics.heightPixels - view.height - dp(48))
                            windowManager.updateViewLayout(view, params)
                            saveBallPosition(params)
                        }
                        removeCloseTarget()
                    }
                    dragging = false
                }
            }
            true
        }
    }

    @Suppress("DEPRECATION")
    private fun showPanel() {
        removeBall()
        if (panel != null) return
        val state = controller.state()
        val root = object : LinearLayout(this) {
            override fun onWindowFocusChanged(hasWindowFocus: Boolean) {
                super.onWindowFocusChanged(hasWindowFocus)
                if (!hasWindowFocus) {
                    postDelayed({
                        if (panel === this && !hasWindowFocus()) collapsePanel()
                    }, WINDOW_FOCUS_LOSS_DELAY_MS)
                }
            }
        }.apply {
            orientation = LinearLayout.VERTICAL
            setPadding(dp(16), dp(12), dp(16), dp(10))
            background = GradientDrawable().apply {
                setColor(getColor(R.color.surface))
                cornerRadius = dp(8).toFloat()
                setStroke(dp(1), getColor(R.color.divider))
            }
            setOnTouchListener { _, event ->
                if (event.actionMasked == MotionEvent.ACTION_OUTSIDE) {
                    collapsePanel()
                    true
                } else {
                    false
                }
            }
        }
        val heading = LinearLayout(this).apply { orientation = LinearLayout.HORIZONTAL; gravity = Gravity.CENTER_VERTICAL }
        val labels = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL }
        panelComputer = TextView(this).apply {
            text = state.binding?.pcName ?: getString(R.string.default_computer)
            setTextColor(getColor(R.color.text_primary)); textSize = 18f; setTypeface(typeface, Typeface.BOLD)
        }
        panelStatus = TextView(this).apply { text = state.status; setTextColor(getColor(R.color.text_secondary)); textSize = 13f }
        panelSyncStatus = TextView(this).apply { text = state.syncState; setTextColor(getColor(R.color.text_secondary)); textSize = 12f }
        labels.addView(panelComputer); labels.addView(panelStatus)
        labels.addView(panelSyncStatus)
        heading.addView(labels, LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f))
        heading.addView(ImageButton(this).apply {
            setImageResource(R.drawable.icon_collapse)
            contentDescription = getString(R.string.collapse)
            val selectable = TypedValue()
            theme.resolveAttribute(android.R.attr.selectableItemBackgroundBorderless, selectable, true)
            setBackgroundResource(selectable.resourceId)
            setPadding(dp(12), dp(12), dp(12), dp(12))
            setOnClickListener { collapsePanel() }
        }, LinearLayout.LayoutParams(dp(48), dp(48)))
        root.addView(heading)
        panelChooser = HorizontalScrollView(this).apply {
            isHorizontalScrollBarEnabled = false
            overScrollMode = View.OVER_SCROLL_NEVER
            addView(LinearLayout(this@FloatingInputService).apply {
                orientation = LinearLayout.HORIZONTAL
                gravity = Gravity.CENTER_VERTICAL
            }, FrameLayout.LayoutParams(FrameLayout.LayoutParams.WRAP_CONTENT, FrameLayout.LayoutParams.MATCH_PARENT))
        }
        root.addView(panelChooser, LinearLayout.LayoutParams(LinearLayout.LayoutParams.MATCH_PARENT, dp(44)))
        renderPanelChooser(state)
        panelInput = EditText(this).apply {
            setText(state.text); setSelection(state.text.length)
            gravity = Gravity.TOP or Gravity.START
            setTextColor(getColor(R.color.text_primary)); setHintTextColor(getColor(R.color.text_secondary)); textSize = 16f
            hint = getString(R.string.input_hint); background = null; inputType = android.text.InputType.TYPE_CLASS_TEXT or android.text.InputType.TYPE_TEXT_FLAG_MULTI_LINE
            addTextChangedListener(object : TextWatcher {
                override fun beforeTextChanged(s: CharSequence?, start: Int, count: Int, after: Int) = Unit
                override fun onTextChanged(s: CharSequence?, start: Int, before: Int, count: Int) = Unit
                override fun afterTextChanged(s: Editable?) { if (!suppressTextChange) controller.textChanged(s?.toString().orEmpty()) }
            })
        }
        root.addView(panelInput, LinearLayout.LayoutParams(LinearLayout.LayoutParams.MATCH_PARENT, 0, 1f))
        val primaryActions = LinearLayout(this).apply { orientation = LinearLayout.HORIZONTAL }
        panelOpenImage = ImageButton(this).apply {
            setImageResource(R.drawable.icon_image)
            contentDescription = getString(R.string.image)
            background = getDrawable(R.drawable.button_secondary)
            setPadding(dp(12), dp(12), dp(12), dp(12))
            isEnabled = state.binding != null && state.imageTransfer != FlowTypeApplication.ImageTransferState.SENDING
            setOnClickListener { openImagePicker() }
        }
        primaryActions.addView(
            panelOpenImage,
            LinearLayout.LayoutParams(dp(48), dp(48)).apply { marginEnd = dp(8) },
        )
        panelSync = Button(this).apply {
            setText(R.string.sync)
            isAllCaps = false
            textSize = 14f
            setTextColor(getColor(R.color.text_primary))
            background = getDrawable(R.drawable.button_secondary)
            isEnabled = state.syncAvailable
            setOnClickListener { controller.sync() }
        }
        primaryActions.addView(
            panelSync,
            LinearLayout.LayoutParams(0, dp(48), 1f).apply { marginEnd = dp(8) },
        )
        panelNewSession = Button(this).apply {
            setText(R.string.new_session); isAllCaps = false; setTextColor(getColor(R.color.button_primary_text)); background = getDrawable(R.drawable.button_primary)
            isEnabled = state.activeSession || state.text.isNotEmpty()
            setOnClickListener { controller.startNewSession() }
        }
        primaryActions.addView(panelNewSession, LinearLayout.LayoutParams(0, dp(48), 1f))
        root.addView(primaryActions, LinearLayout.LayoutParams(LinearLayout.LayoutParams.MATCH_PARENT, dp(48)))
        val screenWidth = resources.displayMetrics.widthPixels
        val edgeGestureMargin = dp(32)
        val params = WindowManager.LayoutParams(
            (screenWidth - edgeGestureMargin * 2).coerceAtLeast(dp(280)),
            dp(270),
            WindowManager.LayoutParams.TYPE_APPLICATION_OVERLAY,
            WindowManager.LayoutParams.FLAG_NOT_TOUCH_MODAL,
            PixelFormat.TRANSLUCENT,
        ).apply {
            gravity = Gravity.BOTTOM or Gravity.CENTER_HORIZONTAL
            flags = WindowManager.LayoutParams.FLAG_NOT_TOUCH_MODAL or
                WindowManager.LayoutParams.FLAG_WATCH_OUTSIDE_TOUCH
            softInputMode = WindowManager.LayoutParams.SOFT_INPUT_ADJUST_RESIZE
            y = dp(8)
        }
        ViewCompat.setOnApplyWindowInsetsListener(root) { view, insets ->
            val visible = insets.isVisible(WindowInsetsCompat.Type.ime())
            if (panelImeVisible && !visible) {
                view.post { if (panel === view) collapsePanel() }
            }
            panelImeVisible = visible
            val systemGestures = insets.getInsets(WindowInsetsCompat.Type.systemGestures())
            val navigationBars = insets.getInsets(WindowInsetsCompat.Type.navigationBars())
            val safeBottom = maxOf(systemGestures.bottom, navigationBars.bottom) + dp(8)
            if (params.y != safeBottom) {
                params.y = safeBottom
                if (view.isAttachedToWindow) windowManager.updateViewLayout(view, params)
            }
            insets
        }
        runCatching { windowManager.addView(root, params) }.onSuccess {
            panel = root
            panelInput?.requestFocus()
            panelInput?.postDelayed({
                getSystemService(InputMethodManager::class.java).showSoftInput(panelInput, InputMethodManager.SHOW_IMPLICIT)
            }, 250)
        }.onFailure {
            showBall()
        }
    }

    private fun renderPanelChooser(state: FlowTypeApplication.UiState) {
        val chooser = panelChooser?.getChildAt(0) as? LinearLayout ?: return
        chooser.removeAllViews()
        controller.bindings.list().forEach { binding ->
            val selected = binding.pcId == state.binding?.pcId
            val online = binding.pcId in state.onlinePcIds || (selected && state.connected)
            val active = binding.pcId == state.recentActivityPcId
            val chip = LinearLayout(this).apply {
                orientation = LinearLayout.HORIZONTAL
                gravity = Gravity.CENTER_VERTICAL
                setPadding(dp(10), 0, dp(10), 0)
                background = GradientDrawable().apply {
                    cornerRadius = dp(6).toFloat()
                    setColor(getColor(R.color.surface))
                    setStroke(dp(if (selected) 2 else 1), getColor(if (selected) R.color.accent else R.color.divider))
                }
                contentDescription = buildString {
                    append(binding.pcName)
                    if (selected) append(getString(R.string.a11y_selected))
                    if (active) append(getString(R.string.a11y_recently_used))
                    append(getString(if (online) R.string.a11y_connected else R.string.a11y_disconnected))
                }
                setOnClickListener { controller.selectComputer(binding.pcId) }
            }
            chip.addView(View(this).apply {
                background = GradientDrawable().apply {
                    shape = GradientDrawable.OVAL
                    setColor(getColor(if (online) R.color.accent else R.color.status_warning))
                }
            }, LinearLayout.LayoutParams(dp(8), dp(8)).apply { marginEnd = dp(6) })
            chip.addView(TextView(this).apply {
                text = binding.pcName
                setTextColor(getColor(R.color.text_primary))
                textSize = 13f
                setTypeface(typeface, if (selected) Typeface.BOLD else Typeface.NORMAL)
            })
            if (active) {
                chip.addView(TextView(this).apply {
                    text = " •"
                    setTextColor(getColor(R.color.status_activity))
                    textSize = 16f
                })
            }
            chooser.addView(chip, LinearLayout.LayoutParams(LinearLayout.LayoutParams.WRAP_CONTENT, dp(36)).apply {
                marginEnd = dp(6)
            })
        }
    }

    private fun render(state: FlowTypeApplication.UiState) {
        panelComputer?.text = state.binding?.pcName ?: getString(R.string.default_computer)
        panelStatus?.text = state.status
        panelStatus?.setTextColor(getColor(
            when {
                state.binding == null -> R.color.status_neutral
                state.connected -> R.color.accent
                else -> R.color.status_warning
            },
        ))
        panelSyncStatus?.text = state.syncState
        renderPanelChooser(state)
        panelInput?.let {
            if (it.text.toString() != state.text) {
                suppressTextChange = true
                it.setText(state.text); it.setSelection(state.text.length)
                suppressTextChange = false
            }
            it.isEnabled = state.binding != null && !state.finishing
        }
        panelOpenImage?.isEnabled = state.binding != null && state.imageTransfer != FlowTypeApplication.ImageTransferState.SENDING
        panelSync?.isEnabled = state.syncAvailable
        panelNewSession?.isEnabled = state.activeSession || state.text.isNotEmpty()
    }

    private fun collapsePanel() {
        panelInput?.let { getSystemService(InputMethodManager::class.java).hideSoftInputFromWindow(it.windowToken, 0) }
        panel?.let { windowManager.removeView(it) }
        panel = null; panelInput = null; panelStatus = null; panelSyncStatus = null; panelComputer = null; panelSync = null; panelNewSession = null; panelOpenImage = null; panelChooser = null; panelImeVisible = false
        showBall()
    }

    private fun openFullApp() {
        removeOverlays()
        startActivity(Intent(this, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP))
    }

    private fun openImagePicker() {
        removeOverlays()
        startActivity(
            Intent(this, MainActivity::class.java)
                .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP)
                .setAction(MainActivity.ACTION_OPEN_IMAGE),
        )
    }

    private fun showCloseTarget() {
        if (closeTarget != null) return
        val view = TextView(this).apply {
            text = getString(R.string.close_floating); gravity = Gravity.CENTER
            setTextColor(getColor(R.color.text_primary)); textSize = 16f
            background = GradientDrawable().apply { setColor(0xE6101214.toInt()); cornerRadius = dp(8).toFloat() }
        }
        val params = WindowManager.LayoutParams(
            WindowManager.LayoutParams.MATCH_PARENT, dp(72), WindowManager.LayoutParams.TYPE_APPLICATION_OVERLAY,
            WindowManager.LayoutParams.FLAG_NOT_FOCUSABLE or WindowManager.LayoutParams.FLAG_NOT_TOUCHABLE, PixelFormat.TRANSLUCENT,
        ).apply { gravity = Gravity.BOTTOM; horizontalMargin = 0.08f; y = dp(16) }
        windowManager.addView(view, params)
        closeTarget = view
    }

    private fun removeCloseTarget() { closeTarget?.let { windowManager.removeView(it) }; closeTarget = null }
    private fun removeBall() { ball?.let { windowManager.removeView(it) }; ball = null; ballParams = null }
    private fun removeOverlays() {
        removeBall()
        panel?.let { windowManager.removeView(it) }
        panel = null; panelInput = null; panelStatus = null; panelSyncStatus = null; panelComputer = null; panelSync = null; panelNewSession = null; panelOpenImage = null; panelChooser = null; panelImeVisible = false
        removeCloseTarget()
    }

    private fun saveBallPosition(params: WindowManager.LayoutParams) {
        val portrait = resources.configuration.orientation == Configuration.ORIENTATION_PORTRAIT
        preferences().edit().putInt(if (portrait) "ball_x_portrait" else "ball_x_landscape", params.x)
            .putInt(if (portrait) "ball_y_portrait" else "ball_y_landscape", params.y).apply()
    }

    private fun preferences() = getSharedPreferences("floating-v1", Context.MODE_PRIVATE)

    private fun notification() = NotificationCompat.Builder(this, CHANNEL_ID)
        .setSmallIcon(android.R.drawable.ic_menu_edit)
        .setContentTitle(getString(R.string.app_name))
        .setContentText(getString(R.string.floating_notification))
        .setContentIntent(PendingIntent.getActivity(this, 1, Intent(this, MainActivity::class.java), PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT))
        .addAction(0, getString(R.string.close), PendingIntent.getService(this, 2, Intent(this, FloatingInputService::class.java).setAction(ACTION_STOP), PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT))
        .setOngoing(true).setSilent(true).setPriority(NotificationCompat.PRIORITY_LOW).build()

    private fun createNotificationChannel() {
        getSystemService(NotificationManager::class.java).createNotificationChannel(
            NotificationChannel(CHANNEL_ID, getString(R.string.floating_input), NotificationManager.IMPORTANCE_LOW).apply {
                setShowBadge(false); setSound(null, null)
            },
        )
    }

    private fun dp(value: Int): Int = (value * resources.displayMetrics.density).toInt()

    companion object {
        private const val CHANNEL_ID = "floating-input"
        private const val NOTIFICATION_ID = 101
        private const val ACTION_START = "app.flowtype.floating.START"
        private const val ACTION_STOP = "app.flowtype.floating.STOP"
        private const val ACTION_HIDE = "app.flowtype.floating.HIDE"
        private const val ACTION_SHOW = "app.flowtype.floating.SHOW"
        private const val WINDOW_FOCUS_LOSS_DELAY_MS = 300L

        fun start(context: Context) = ContextCompat.startForegroundService(context, intent(context, ACTION_START))
        fun stop(context: Context) { context.startService(intent(context, ACTION_STOP)) }
        fun hide(context: Context) { if (isEnabled(context)) context.startService(intent(context, ACTION_HIDE)) }
        fun show(context: Context) { if (isEnabled(context)) ContextCompat.startForegroundService(context, intent(context, ACTION_SHOW)) }
        private fun intent(context: Context, action: String) = Intent(context, FloatingInputService::class.java).setAction(action)
        private fun isEnabled(context: Context) = (context.applicationContext as? FlowTypeApplication)?.settings?.floatingInput == true
    }
}
