package app.flowtype

import android.app.Application
import android.os.Handler
import android.os.Looper
import app.flowtype.data.AppDatabase
import app.flowtype.data.HistoryStore
import app.flowtype.data.SettingsStore
import app.flowtype.network.SyncClient
import app.flowtype.image.PreparedImage
import app.flowtype.network.ControlClient
import app.flowtype.network.ComputerDiscovery
import app.flowtype.network.TargetSelector
import app.flowtype.pairing.BindingStore
import app.flowtype.pairing.ComputerBinding
import app.flowtype.protocol.AckMessage
import app.flowtype.protocol.ErrorCode
import app.flowtype.protocol.ErrorMessage
import app.flowtype.protocol.TargetMessage
import app.flowtype.protocol.TargetState
import app.flowtype.protocol.SnapshotMessage
import app.flowtype.protocol.SnapshotType
import app.flowtype.security.PhoneIdentity
import app.flowtype.security.SecureDraftStore
import app.flowtype.session.InputSession
import java.util.UUID
import java.util.concurrent.CopyOnWriteArraySet

class FlowTypeApplication : Application(), SyncClient.Listener {
    enum class ImageTransferState { IDLE, SENDING, SUCCESS, FAILED }

    data class UiState(
        val text: String,
        val finishing: Boolean,
        val activeSession: Boolean,
        val binding: ComputerBinding?,
        val status: String,
        val connected: Boolean,
        val showSyncFullText: Boolean,
        val syncCurrentCursorAvailable: Boolean,
        val onlinePcIds: Set<String>,
        val imageTransfer: ImageTransferState,
        val autoSelecting: Boolean,
    )

    lateinit var database: AppDatabase
        private set
    lateinit var bindings: BindingStore
        private set
    lateinit var history: HistoryStore
        private set
    lateinit var settings: SettingsStore
        private set

    private lateinit var drafts: SecureDraftStore
    private lateinit var session: InputSession
    private lateinit var syncClient: SyncClient
    private lateinit var targetProbeClient: app.flowtype.network.TargetProbeClient
    private lateinit var discovery: ComputerDiscovery
    private val controlClients = mutableMapOf<String, ControlClient>()
    private val mainHandler = Handler(Looper.getMainLooper())
    private val observers = CopyOnWriteArraySet<(UiState) -> Unit>()
    private var currentBinding: ComputerBinding? = null
    private var statusText = "尚未绑定电脑"
    private var connected = false
    private var showSyncFullText = false
    private var targetState: TargetState? = null
    private val onlinePcIds = mutableSetOf<String>()
    private val saveDraft = Runnable { saveDraftNow() }
    private var imageTransfer = ImageTransferState.IDLE
    private var autoSelecting = false
    private var autoSelectionGeneration = 0L
    private var autoSelectionTargetPcId: String? = null
    private var pendingAutoStart: SnapshotMessage? = null
    private var pendingAutoLatest: SnapshotMessage? = null
    private var autoSelectionError: String? = null
    private var manualStartPending = false

    override fun onCreate() {
        super.onCreate()
        database = AppDatabase(this)
        bindings = BindingStore(this, database)
        history = HistoryStore(database)
        settings = SettingsStore(database)
        drafts = SecureDraftStore(this)
        session = InputSession(bindings.phoneId) { UUID.randomUUID().toString() }
        syncClient = SyncClient(bindings.phoneId, PhoneIdentity(), this)
        targetProbeClient = app.flowtype.network.TargetProbeClient(bindings.phoneId, PhoneIdentity())
        discovery = ComputerDiscovery(this, bindings, ::onComputerDiscovered)

        drafts.load()?.let { stored ->
            session.restore(stored.session)
            session.recoverySnapshot()?.let {
                syncClient.restore(it, stored.session.acknowledgedSequence, stored.remoteStarted)
            }
        }
        currentBinding = bindings.load()
        currentBinding?.let {
            statusText = getString(R.string.status_connecting, it.pcName)
            syncClient.connect(it)
        }
        refreshControlClients()
        discovery.start()
    }

    fun observe(observer: (UiState) -> Unit) {
        observers += observer
        observer(state())
    }

    fun removeObserver(observer: (UiState) -> Unit) {
        observers -= observer
    }

    fun state(): UiState = UiState(
        text = session.currentText,
        finishing = session.finishing,
        activeSession = session.sessionId != null,
        binding = currentBinding,
        status = statusText,
        connected = connected,
        showSyncFullText = showSyncFullText ||
            (session.sessionId == null && session.currentText.isNotEmpty()),
        syncCurrentCursorAvailable = session.sessionId != null && connected &&
            (session.acknowledgedSequence < session.latestSequence || targetState == TargetState.NOT_FOREGROUND),
        onlinePcIds = onlinePcIds.toSet(),
        imageTransfer = imageTransfer,
        autoSelecting = autoSelecting,
    )

    fun resetImageTransfer() {
        if (imageTransfer == ImageTransferState.SENDING) return
        imageTransfer = ImageTransferState.IDLE
        notifyChanged()
    }

    fun sendImage(image: PreparedImage): Boolean {
        if (!connected || !syncClient.sendImage(image)) return false
        imageTransfer = ImageTransferState.SENDING
        notifyChanged()
        return true
    }

    fun textChanged(text: String) {
        if (session.sessionId != null && syncClient.requiresExplicitStart()) {
            resetFailedSession()
        }
        if (session.sessionId == null && text.isNotEmpty()) targetState = null
        session.onTextChanged(text)?.let { snapshot ->
            if (settings.autoSelectComputer && !manualStartPending && snapshot.type == SnapshotType.START) {
                beginAutoSelection(snapshot)
            } else if (autoSelecting || pendingAutoStart != null) {
                pendingAutoLatest = snapshot
            } else {
                if (snapshot.type == SnapshotType.START) manualStartPending = false
                syncClient.send(snapshot)
            }
        }
        scheduleDraftSave()
        notifyChanged()
    }

    fun finish() {
        session.finish()?.let {
            statusText = getString(R.string.finish_waiting)
            saveDraftNow()
            if (autoSelecting || pendingAutoStart != null) pendingAutoLatest = it else syncClient.send(it)
            notifyChanged()
        }
    }

    fun abandonSyncAndFinish() {
        val sessionId = session.sessionId ?: return
        if (!session.finishing) return
        val text = session.currentText
        clearAutoSelection()
        syncClient.abandonSession(sessionId)
        currentBinding?.let { history.add(it, text) }
        session.reset()
        targetState = null
        drafts.clear()
        showSyncFullText = false
        statusText = currentBinding?.let {
            if (connected) getString(R.string.status_connected, it.pcName)
            else getString(R.string.status_reconnecting, it.pcName)
        } ?: getString(R.string.status_unpaired)
        notifyChanged()
    }

    fun resetSession() {
        val sessionId = session.sessionId
        val text = session.currentText
        clearAutoSelection()
        if (sessionId != null) syncClient.abandonSession(sessionId)
        if (text.isNotEmpty()) currentBinding?.let { history.add(it, text) }
        session.reset()
        targetState = null
        drafts.clear()
        showSyncFullText = false
        statusText = currentBinding?.let {
            if (connected) getString(R.string.status_connected, it.pcName)
            else getString(R.string.status_reconnecting, it.pcName)
        } ?: getString(R.string.status_unpaired)
        notifyChanged()
    }

    fun syncToCurrentCursor() {
        val oldSessionId = session.sessionId ?: return
        syncClient.abandonSession(oldSessionId)
        targetState = null
        val snapshots = session.restartAtCurrentCursor()
        if (snapshots.isEmpty()) {
            drafts.clear()
            statusText = currentBinding?.let { getString(R.string.status_connected, it.pcName) }
                ?: getString(R.string.status_unpaired)
        } else {
            snapshots.forEach(syncClient::send)
            statusText = getString(R.string.status_syncing_current_cursor)
            saveDraftNow()
        }
        notifyChanged()
    }

    fun syncFullText() {
        if (session.sessionId != null && syncClient.requiresExplicitStart()) {
            resetFailedSession()
        }
        if (settings.autoSelectComputer && !manualStartPending) {
            val localStart = session.startLocalDraft()
            if (localStart != null) {
                clearAutoSelection()
                beginAutoSelection(localStart)
                showSyncFullText = false
                scheduleDraftSave()
                notifyChanged()
            }
            return
        }
        targetState = null
        val localStart = session.startLocalDraft()
        if (localStart != null) {
            syncClient.send(localStart)
        } else {
            syncClient.startOfflineDraft()
        }
        manualStartPending = false
        showSyncFullText = false
        statusText = currentBinding?.let { getString(R.string.status_connecting, it.pcName) }
            ?: getString(R.string.status_unpaired)
        scheduleDraftSave()
        notifyChanged()
    }

    fun replaceWithHistory(text: String): Boolean {
        if (session.sessionId != null) return false
        session.replaceLocalDraft(text)
        saveDraftNow()
        notifyChanged()
        return true
    }

    fun acceptBinding(binding: ComputerBinding) {
        bindings.save(binding)
        switchToComputer(binding)
    }

    fun setAutoSelectComputer(enabled: Boolean) {
        if (session.sessionId != null) return
        settings.autoSelectComputer = enabled
        if (!enabled) {
            clearAutoSelection()
            manualStartPending = session.currentText.isNotEmpty()
            showSyncFullText = manualStartPending
        }
        notifyChanged()
    }

    fun isComputerAutoSelected(pcId: String): Boolean = bindings.isAutoSelected(pcId)

    fun setComputerAutoSelected(pcId: String, selected: Boolean) {
        bindings.setAutoSelected(pcId, selected)
        notifyChanged()
    }

    fun selectComputer(pcId: String): Boolean {
        val binding = bindings.select(pcId) ?: return false
        switchToComputer(binding)
        return true
    }

    fun renameComputer(pcId: String, name: String) {
        bindings.rename(pcId, name)
        currentBinding = bindings.load()
        refreshControlClients()
        notifyChanged()
    }

    fun unbindComputer(pcId: String) {
        val selected = currentBinding?.pcId == pcId
        bindings.remove(pcId)
        PhoneIdentity().delete(pcId)
        if (selected) {
            syncClient.shutdown()
            currentBinding = bindings.load()
            connected = false
            currentBinding?.let(::connect) ?: run {
                refreshControlClients()
                statusText = getString(R.string.status_unpaired)
                notifyChanged()
            }
        } else {
            refreshControlClients()
            notifyChanged()
        }
    }

    fun ensureConnected() = syncClient.ensureConnected()

    private fun refreshControlClients() {
        val stored = bindings.list()
            .filter { it.pairingToken == null }
            .associateBy { it.pcId }
        controlClients.keys.toList()
            .filter { it !in stored }
            .forEach { pcId ->
                controlClients.remove(pcId)?.shutdown()
            }
        stored.values.forEach { binding ->
            controlClients.getOrPut(binding.pcId) {
                ControlClient(bindings.phoneId, PhoneIdentity(), object : ControlClient.Listener {
                    override fun onSwitchComputer(pcId: String) = onMain {
                        this@FlowTypeApplication.onSwitchComputer(pcId)
                    }
                })
            }.update(binding)
        }
    }

    fun saveNow() = saveDraftNow()

    override fun onReady(binding: ComputerBinding) = onMain {
        currentBinding = bindings.markPaired(binding)
        refreshControlClients()
        connected = true
        targetState = null
        showSyncFullText = manualStartPending || autoSelectionError != null || syncClient.requiresExplicitStart()
        statusText = autoSelectionError ?: if (showSyncFullText) {
            getString(R.string.status_place_cursor)
        } else {
            getString(R.string.status_connected, binding.pcName)
        }
        if (pendingAutoStart != null && autoSelectionTargetPcId == binding.pcId) {
            val start = pendingAutoStart
            val latest = pendingAutoLatest
            pendingAutoStart = null
            pendingAutoLatest = null
            autoSelecting = false
            autoSelectionTargetPcId = null
            autoSelectionError = null
            manualStartPending = false
            showSyncFullText = false
            statusText = getString(R.string.status_connected, binding.pcName)
            start?.let(syncClient::send)
            if (latest != null && latest != start) syncClient.send(latest)
        }
        notifyChanged()
    }

    override fun onAck(ack: AckMessage) = onMain {
        session.acknowledge(ack)
        if (session.finished) {
            currentBinding?.let { history.add(it, session.currentText) }
            session.reset()
            drafts.clear()
            showSyncFullText = false
            statusText = currentBinding?.let { getString(R.string.status_connected, it.pcName) }
                ?: getString(R.string.status_unpaired)
        } else {
            scheduleDraftSave()
        }
        notifyChanged()
    }

    override fun onTarget(target: TargetMessage) = onMain {
        targetState = target.targetState
        if (target.targetState == TargetState.INVALID) {
            resetFailedSession()
        }
        statusText = when (target.targetState) {
            TargetState.ACTIVE -> getString(
                R.string.status_target,
                target.targetName ?: getString(R.string.default_computer),
            )
            TargetState.NOT_FOREGROUND -> getString(
                R.string.status_target_waiting,
                target.targetName ?: getString(R.string.default_computer),
            )
            TargetState.INVALID -> getString(R.string.status_target_invalid)
        }
        showSyncFullText = syncClient.requiresExplicitStart()
        scheduleDraftSave()
        notifyChanged()
    }

    override fun onSwitchComputer(pcId: String) = onMain {
        val binding = bindings.select(pcId) ?: return@onMain
        if (currentBinding?.pcId == binding.pcId) {
            ensureConnected()
        } else {
            switchToComputer(binding)
        }
        notifyChanged()
    }

    override fun onDisconnected(binding: ComputerBinding) = onMain {
        connected = false
        targetState = null
        statusText = getString(R.string.status_reconnecting, binding.pcName)
        notifyChanged()
    }

    override fun onFailure(binding: ComputerBinding) = onMain {
        connected = false
        targetState = null
        statusText = getString(R.string.status_reconnecting, binding.pcName)
        notifyChanged()
    }

    override fun onServerError(binding: ComputerBinding, error: ErrorMessage) = onMain {
        resetFailedSession()
        connected = true
        targetState = null
        showSyncFullText = true
        statusText = when (error.code) {
            ErrorCode.INJECTOR_UNAVAILABLE -> getString(R.string.status_input_service_unavailable)
            ErrorCode.TARGET_MODIFIED -> getString(R.string.status_target_modified)
            else -> getString(R.string.status_sync_stopped)
        }
        scheduleDraftSave()
        notifyChanged()
    }

    override fun onPairingInvalid(binding: ComputerBinding) = onMain {
        connected = false
        targetState = null
        bindings.remove(binding.pcId)
        PhoneIdentity().delete(binding.pcId)
        currentBinding = bindings.load()
        refreshControlClients()
        statusText = getString(R.string.status_binding_invalid)
        currentBinding?.let(::connect) ?: notifyChanged()
    }

    override fun onImageTransferred(transferId: String) = onMain {
        imageTransfer = ImageTransferState.SUCCESS
        notifyChanged()
    }

    override fun onImageTransferFailed(transferId: String) = onMain {
        imageTransfer = ImageTransferState.FAILED
        notifyChanged()
    }

    private fun connect(binding: ComputerBinding) {
        currentBinding = binding
        refreshControlClients()
        connected = false
        showSyncFullText = false
        statusText = getString(R.string.status_connecting, binding.pcName)
        syncClient.connect(binding)
        notifyChanged()
    }

    private fun switchToComputer(binding: ComputerBinding) {
        val text = session.currentText
        val oldSessionId = session.sessionId
        oldSessionId?.let(syncClient::abandonSession)
        syncClient.resetForTargetSelection()
        if (oldSessionId != null || session.finishing) {
            session.reset()
            session.replaceLocalDraft(text)
        }
        clearAutoSelection()
        manualStartPending = text.isNotEmpty()
        connect(binding)
    }

    /**
     * A rejected update leaves the remote injector unable to accept more
     * updates for this session. Keep the text, but start the next edit as a
     * fresh session so recovery does not require clearing app data.
     */
    private fun resetFailedSession() {
        val text = session.currentText
        val oldSessionId = session.sessionId
        clearAutoSelection()
        oldSessionId?.let(syncClient::abandonSession)
        if (oldSessionId != null || session.finishing) {
            session.reset()
            session.replaceLocalDraft(text)
        }
        targetState = null
        manualStartPending = false
        showSyncFullText = text.isNotEmpty()
    }

    private fun beginAutoSelection(initial: SnapshotMessage) {
        if (autoSelecting) {
            pendingAutoLatest = initial
            return
        }
        val candidates = bindings.list().filter { bindings.isAutoSelected(it.pcId) }
        if (candidates.isEmpty()) {
            failAutoSelection(getString(R.string.status_auto_no_computer), currentBinding)
            return
        }
        val generation = ++autoSelectionGeneration
        autoSelecting = true
        autoSelectionError = null
        manualStartPending = false
        pendingAutoStart = initial
        pendingAutoLatest = initial
        autoSelectionTargetPcId = null
        statusText = getString(R.string.status_auto_selecting)
        val fallback = currentBinding
        // Closing the socket alone leaves the old Windows injector session
        // active. Release it before selecting the next target so a later
        // automatic switch can start a clean session.
        syncClient.abandonSession(initial.sessionId)
        syncClient.resetForTargetSelection()
        if (candidates.size == 1) {
            // With one selected computer there is no ambiguity to resolve.
            // Probing can fail while this PC's FlowType window is foreground,
            // even though the normal input connection can still find the
            // external editor behind it.
            autoSelectionTargetPcId = candidates.single().pcId
            connect(candidates.single())
            return
        }
        targetProbeClient.probe(candidates) { results ->
            onMain {
                if (generation != autoSelectionGeneration || !autoSelecting) return@onMain
                val winner = TargetSelector.choose(results, fallback?.pcId)
                if (winner == null) {
                    failAutoSelection(getString(R.string.status_auto_ambiguous), fallback)
                } else {
                    autoSelectionTargetPcId = winner.binding.pcId
                    connect(winner.binding)
                }
            }
        }
    }

    private fun failAutoSelection(message: String, fallback: ComputerBinding?) {
        autoSelecting = false
        autoSelectionTargetPcId = null
        pendingAutoStart = null
        pendingAutoLatest = null
        val text = session.currentText
        val hadSession = session.sessionId != null
        if (hadSession || session.finishing) {
            session.reset()
            session.replaceLocalDraft(text)
        }
        autoSelectionError = message
        showSyncFullText = text.isNotEmpty()
        fallback?.let(::connect)
        statusText = message
        notifyChanged()
    }

    private fun clearAutoSelection() {
        autoSelectionGeneration += 1
        autoSelecting = false
        autoSelectionTargetPcId = null
        pendingAutoStart = null
        pendingAutoLatest = null
        autoSelectionError = null
    }

    private fun onComputerDiscovered(pcId: String, endpoint: String?) = onMain {
        if (endpoint == null) {
            onlinePcIds -= pcId
        } else {
            onlinePcIds += pcId
            val selected = currentBinding
            val stored = bindings.list().firstOrNull { it.pcId == pcId }
            if (stored != null && stored.endpoint != endpoint) {
                bindings.updateEndpoint(pcId, endpoint)
                if (selected?.pcId == pcId && !session.finishing) {
                    connect(stored.withPreferredEndpoint(endpoint))
                }
            }
        }
        refreshControlClients()
        notifyChanged()
    }

    private fun scheduleDraftSave() {
        mainHandler.removeCallbacks(saveDraft)
        mainHandler.postDelayed(saveDraft, 200)
    }

    private fun saveDraftNow() {
        mainHandler.removeCallbacks(saveDraft)
        drafts.save(session.state(), syncClient.remoteStarted())
    }

    private fun notifyChanged() {
        val state = state()
        observers.forEach { it(state) }
    }

    private fun onMain(action: () -> Unit) {
        if (Looper.myLooper() == Looper.getMainLooper()) action() else mainHandler.post(action)
    }
}
