package app.flowtype

import android.content.Context
import android.os.Handler
import android.os.Looper
import app.flowtype.data.AppDatabase
import app.flowtype.data.HistoryEntry
import app.flowtype.data.HistoryStore
import app.flowtype.data.SettingsStore
import app.flowtype.data.StorageDispatcher
import app.flowtype.connectivity.ConnectivityDemand
import app.flowtype.image.PreparedImage
import app.flowtype.network.AutoSelectionCoordinator
import app.flowtype.network.ComputerDiscovery
import app.flowtype.network.ControlClientPool
import app.flowtype.network.SyncClient
import app.flowtype.pairing.BindingStore
import app.flowtype.pairing.ComputerBinding
import app.flowtype.protocol.AckMessage
import app.flowtype.protocol.ErrorCode
import app.flowtype.protocol.ErrorMessage
import app.flowtype.protocol.SnapshotMessage
import app.flowtype.protocol.SnapshotType
import app.flowtype.protocol.TargetMessage
import app.flowtype.protocol.TargetState
import app.flowtype.security.DraftRepository
import app.flowtype.security.PhoneIdentity
import app.flowtype.security.SecureDraftStore
import app.flowtype.session.ComputerSessions
import app.flowtype.session.InputSession
import app.flowtype.update.UpdateManager
import java.util.UUID
import java.util.concurrent.CopyOnWriteArraySet

class FlowTypeController(private val application: FlowTypeApplication) : SyncClient.Listener {
    enum class ImageTransferState { IDLE, SENDING, SUCCESS, FAILED }

    private data class LocalizedText(val resourceId: Int, val arguments: List<Any> = emptyList()) {
        fun resolve(context: Context): String =
            context.getString(resourceId, *arguments.toTypedArray())
    }

    data class UiState(
        val text: String,
        val finishing: Boolean,
        val activeSession: Boolean,
        val storageReady: Boolean,
        val binding: ComputerBinding?,
        val computers: List<ComputerBinding>,
        val status: String,
        val syncState: String,
        val connected: Boolean,
        val showSyncFullText: Boolean,
        val syncAvailable: Boolean,
        val onlinePcIds: Set<String>,
        val recentActivityPcId: String?,
        val imageTransfer: ImageTransferState,
        val autoSelecting: Boolean,
    )

    private lateinit var database: AppDatabase
    private lateinit var bindings: BindingStore
    private lateinit var history: HistoryStore
    private lateinit var settings: SettingsStore
    lateinit var updates: UpdateManager
        private set

    private val storage = StorageDispatcher()
    private lateinit var drafts: DraftRepository
    private lateinit var sessions: ComputerSessions
    private val session: InputSession
        get() = sessions.current
    private lateinit var syncClient: SyncClient
    private lateinit var autoSelection: AutoSelectionCoordinator
    private lateinit var discovery: ComputerDiscovery
    private lateinit var controlClients: ControlClientPool
    private val mainHandler = Handler(Looper.getMainLooper())
    private val connectivityDemand = ConnectivityDemand()
    private val observers = CopyOnWriteArraySet<(UiState) -> Unit>()
    private var currentBinding: ComputerBinding? = null
    private var computers: List<ComputerBinding> = emptyList()
    private var storageReady = false
    private var statusText = LocalizedText(R.string.status_unpaired)
    private var connected = false
    private var showSyncFullText = false
    private var targetState: TargetState? = null
    private val onlinePcIds = mutableSetOf<String>()
    private var recentActivityPcId: String? = null
    private val saveDraft = Runnable { saveDraftNow() }
    private var imageTransfer = ImageTransferState.IDLE
    private var autoSelecting = false
    private var autoSelectionTargetPcId: String? = null
    private var pendingAutoStart: SnapshotMessage? = null
    private var pendingAutoLatest: SnapshotMessage? = null
    private var autoSelectionError: LocalizedText? = null
    private var manualStartPending = false
    private var connectivityRunning = false
    private val disconnectInBackground = Runnable {
        if (!connectivityDemand.required) suspendConnectivity()
    }

    fun start() {
        database = AppDatabase(application)
        bindings = BindingStore(application, database)
        history = HistoryStore(database)
        settings = SettingsStore(database)
        drafts = DraftRepository(SecureDraftStore(application), storage)
        sessions = ComputerSessions(
            phoneId = bindings.phoneId,
            sessionIdFactory = { UUID.randomUUID().toString() },
            load = drafts::load,
            save = drafts::save,
            clear = drafts::clear,
        )
        updates = UpdateManager(application) {
            session.sessionId != null || imageTransfer == ImageTransferState.SENDING
        }
        syncClient = SyncClient(bindings.phoneId, PhoneIdentity(), this)
        controlClients = ControlClientPool(
            phoneId = bindings.phoneId,
            onSwitchComputer = { pcId, completion ->
                onMain { switchFromWindows(pcId, completion) }
            },
        )
        autoSelection = AutoSelectionCoordinator(
            app.flowtype.network.TargetProbeClient(bindings.phoneId, PhoneIdentity()),
        )
        discovery = ComputerDiscovery(application, bindings, ::onComputerDiscovered)

        refreshBindings()
        currentBinding = computers.firstOrNull()
        currentBinding?.let {
            statusText = text(R.string.status_connecting, it.pcName)
        }
        drafts.preload(computers.map(ComputerBinding::pcId)) {
            storageReady = true
            currentBinding?.let(::restoreSessionFor)
            if (connectivityRunning) {
                activateCurrentConnectivity()
            } else {
                notifyChanged()
            }
        }
    }

    fun observe(observer: (UiState) -> Unit) {
        observers += observer
        observer(state())
    }

    val autoSelectComputerEnabled: Boolean
        get() = settings.autoSelectComputer

    val keepScreenOnEnabled: Boolean
        get() = settings.keepScreenOn

    val extraDimEnabled: Boolean
        get() = settings.extraDim

    val floatingInputEnabled: Boolean
        get() = settings.floatingInput

    fun setKeepScreenOn(enabled: Boolean) {
        settings.keepScreenOn = enabled
    }

    fun setExtraDim(enabled: Boolean) {
        settings.extraDim = enabled
    }

    fun setFloatingInput(enabled: Boolean) {
        settings.floatingInput = enabled
    }

    fun removeObserver(observer: (UiState) -> Unit) {
        observers -= observer
    }

    fun state(): UiState = UiState(
        text = session.currentText,
        finishing = session.finishing,
        activeSession = session.sessionId != null,
        storageReady = storageReady,
        binding = currentBinding,
        computers = computers,
        status = statusText.resolve(application),
        connected = connected,
        showSyncFullText = showSyncFullText ||
            (session.sessionId == null && session.currentText.isNotEmpty()),
        syncAvailable = storageReady && !autoSelecting && session.currentText.isNotEmpty() && (
            session.sessionId == null || manualStartPending || syncClient.requiresExplicitStart() ||
                targetState == TargetState.NOT_FOREGROUND || targetState == TargetState.INVALID
            ),
        syncState = when {
            session.currentText.isEmpty() -> ""
            autoSelecting -> application.getString(R.string.status_auto_selecting)
            session.sessionId == null || manualStartPending || syncClient.requiresExplicitStart() ||
                targetState == TargetState.NOT_FOREGROUND -> application.getString(R.string.sync_status_pending)
            session.acknowledgedSequence >= session.latestSequence -> application.getString(R.string.sync_status_synced)
            else -> application.getString(R.string.sync_status_syncing)
        },
        onlinePcIds = onlinePcIds.toSet(),
        recentActivityPcId = recentActivityPcId,
        imageTransfer = imageTransfer,
        autoSelecting = autoSelecting,
    )

    fun loadHistory(completion: (List<HistoryEntry>) -> Unit) {
        storage.query(history::list) { result -> completion(result.getOrDefault(emptyList())) }
    }

    fun loadHistoryEntry(id: Long, completion: (HistoryEntry?) -> Unit) {
        storage.query({ history.get(id) }) { result -> completion(result.getOrNull()) }
    }

    fun deleteHistory(ids: Set<Long>, completion: () -> Unit) {
        storage.execute({ history.delete(ids) }, completion)
    }

    fun deleteHistory(id: Long, completion: () -> Unit) {
        storage.execute({ history.delete(id) }, completion)
    }

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
        session.onTextChanged(text, startIfNeeded = !manualStartPending)?.let { snapshot ->
            if (autoSelecting) {
                if (snapshot.type == SnapshotType.START && pendingAutoStart == null) {
                    pendingAutoStart = snapshot
                }
                pendingAutoLatest = snapshot
            } else {
                if (snapshot.type == SnapshotType.START) manualStartPending = false
                syncClient.send(snapshot)
            }
        }
        scheduleDraftSave()
        notifyChanged()
    }

    /** Clear the editable draft without ending its session. An active session
     * emits an empty full-text snapshot so Windows removes the remote text too.
     */
    fun clearCurrentInput() {
        if (session.finishing || session.currentText.isEmpty()) return
        textChanged("")
    }

    /** One user-visible command for initial, recovered, or retargeted full sync. */
    fun sync() {
        if (session.currentText.isEmpty()) return
        when {
            session.sessionId == null || manualStartPending || syncClient.requiresExplicitStart() -> syncFullText()
            targetState == TargetState.NOT_FOREGROUND -> syncToCurrentCursor()
        }
    }

    fun finish() {
        session.finish()?.let {
            statusText = text(R.string.sync_status_syncing)
            saveDraftNow()
            if (autoSelecting) pendingAutoLatest = it else syncClient.send(it)
            notifyChanged()
        }
    }

    fun abandonSyncAndFinish() {
        val sessionId = session.sessionId ?: return
        if (!session.finishing) return
        val text = session.currentText
        clearAutoSelection()
        syncClient.abandonSession(sessionId)
        currentBinding?.let { addHistory(it, text) }
        sessions.prepareNewSession()
        targetState = null
        showSyncFullText = false
        statusText = currentBinding?.let {
            if (connected) text(R.string.status_connected, it.pcName)
            else text(R.string.status_reconnecting, it.pcName)
        } ?: text(R.string.status_unpaired)
        if (settings.autoSelectComputer) beginAutoSelection()
        notifyChanged()
    }

    fun startNewSession() {
        val sessionId = session.sessionId
        val text = session.currentText
        clearAutoSelection()
        if (sessionId != null) syncClient.abandonSession(sessionId)
        if (text.isNotEmpty()) currentBinding?.let { addHistory(it, text) }
        if (sessionId != null) sessions.prepareNewSession() else sessions.clearCurrent()
        manualStartPending = false
        targetState = null
        showSyncFullText = false
        statusText = currentBinding?.let {
            if (connected) text(R.string.status_connected, it.pcName)
            else text(R.string.status_reconnecting, it.pcName)
        } ?: text(R.string.status_unpaired)
        if (settings.autoSelectComputer) beginAutoSelection()
        notifyChanged()
    }

    fun syncToCurrentCursor() {
        val oldSessionId = session.sessionId ?: return
        syncClient.abandonSession(oldSessionId)
        targetState = null
        val snapshots = session.restartAtCurrentCursor()
        if (snapshots.isEmpty()) {
            sessions.clearCurrent()
            statusText = currentBinding?.let { text(R.string.status_connected, it.pcName) }
                ?: text(R.string.status_unpaired)
        } else {
            snapshots.forEach(syncClient::send)
            statusText = text(R.string.sync_status_syncing)
            saveDraftNow()
        }
        notifyChanged()
    }

    fun syncFullText() {
        if (session.sessionId != null && syncClient.requiresExplicitStart()) {
            resetFailedSession()
        }
        targetState = null
        val localStart = session.startLocalDraft(attachExistingAtCursor = true)
        if (localStart != null) {
            syncClient.send(localStart)
        } else {
            syncClient.startOfflineDraft()
        }
        manualStartPending = false
        showSyncFullText = false
        statusText = currentBinding?.let { text(R.string.status_connecting, it.pcName) }
            ?: text(R.string.status_unpaired)
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
        refreshBindings()
        switchToComputer(binding)
    }

    fun setAutoSelectComputer(enabled: Boolean) {
        if (session.sessionId != null) return
        settings.autoSelectComputer = enabled
        if (enabled) {
            beginAutoSelection()
        } else {
            clearAutoSelection()
            manualStartPending = session.currentText.isNotEmpty()
            showSyncFullText = manualStartPending
        }
        notifyChanged()
    }

    fun selectComputer(pcId: String): Boolean {
        val binding = bindings.select(pcId) ?: return false
        refreshBindings()
        if (currentBinding?.pcId == binding.pcId) {
            connected = false
            targetState = null
            statusText = text(R.string.status_connecting, binding.pcName)
            syncClient.forceReconnect()
            notifyChanged()
            return true
        }
        switchToComputer(binding)
        return true
    }

    fun renameComputer(pcId: String, name: String) {
        bindings.rename(pcId, name)
        refreshBindings()
        currentBinding = computers.firstOrNull { it.pcId == currentBinding?.pcId }
        refreshControlClients()
        notifyChanged()
    }

    fun unbindComputer(pcId: String) {
        val selected = currentBinding?.pcId == pcId
        if (selected) {
            session.sessionId?.let(syncClient::abandonSession)
            syncClient.shutdown()
        }
        sessions.remove(pcId)
        bindings.remove(pcId)
        refreshBindings()
        PhoneIdentity().delete(pcId)
        if (selected) {
            currentBinding = computers.firstOrNull()
            connected = false
            currentBinding?.let { binding ->
                syncClient.resetForTargetSelection()
                restoreSessionFor(binding)
                connect(binding)
            } ?: run {
                refreshControlClients()
                statusText = text(R.string.status_unpaired)
                notifyChanged()
            }
        } else {
            refreshControlClients()
            notifyChanged()
        }
    }

    fun ensureConnected() {
        if (!connectivityDemand.required) return
        resumeConnectivity()
        syncClient.ensureConnected()
        controlClients.ensureConnected()
    }

    fun onUiStarted() {
        connectivityDemand.activityStarted()
        resumeConnectivity()
    }

    fun onUiStopped() {
        connectivityDemand.activityStopped()
        scheduleBackgroundDisconnect()
    }

    fun onFloatingInputOpened() {
        connectivityDemand.setFloatingInputVisible(true)
        resumeConnectivity()
    }

    fun onFloatingInputClosed() {
        connectivityDemand.setFloatingInputVisible(false)
        scheduleBackgroundDisconnect()
    }

    private fun refreshControlClients() {
        if (connectivityRunning) {
            controlClients.update(computers, currentBinding?.pcId)
        } else {
            controlClients.shutdown()
        }
    }

    private fun resumeConnectivity() {
        mainHandler.removeCallbacks(disconnectInBackground)
        if (connectivityRunning) return
        connectivityRunning = true
        discovery.start()
        refreshControlClients()
        if (storageReady) activateCurrentConnectivity()
    }

    private fun activateCurrentConnectivity() {
        currentBinding?.let(::connect)
        if (shouldBeginAutoSelection(
                settings.autoSelectComputer,
                session.sessionId != null,
                manualStartPending,
            )
        ) {
            beginAutoSelection()
        } else {
            notifyChanged()
        }
    }

    private fun scheduleBackgroundDisconnect() {
        mainHandler.removeCallbacks(disconnectInBackground)
        if (!connectivityDemand.required) {
            mainHandler.postDelayed(disconnectInBackground, BACKGROUND_DISCONNECT_DELAY_MS)
        }
    }

    private fun suspendConnectivity() {
        if (!connectivityRunning) return
        connectivityRunning = false
        clearAutoSelection()
        syncClient.shutdown()
        controlClients.shutdown()
        discovery.stop()
        onlinePcIds.clear()
        connected = false
        targetState = null
    }

    fun saveNow() = saveDraftNow()

    override fun onReady(binding: ComputerBinding) = onMain {
        currentBinding = bindings.markPaired(binding)
        refreshBindings()
        refreshControlClients()
        connected = true
        targetState = null
        showSyncFullText = manualStartPending || autoSelectionError != null || syncClient.requiresExplicitStart()
        statusText = autoSelectionError ?: if (showSyncFullText) {
            text(R.string.status_place_cursor)
        } else {
            text(R.string.status_connected, binding.pcName)
        }
        if (autoSelecting && autoSelectionTargetPcId == binding.pcId) {
            val start = pendingAutoStart
            val latest = pendingAutoLatest
            pendingAutoStart = null
            pendingAutoLatest = null
            autoSelecting = false
            autoSelectionTargetPcId = null
            autoSelectionError = null
            manualStartPending = false
            showSyncFullText = false
            statusText = text(R.string.status_connected, binding.pcName)
            start?.let(syncClient::send)
            if (latest != null && latest != start) syncClient.send(latest)
        }
        notifyChanged()
    }

    override fun onAck(ack: AckMessage) = onMain {
        session.acknowledge(ack)
        if (session.finished) {
            if (session.currentText.isNotEmpty()) {
                currentBinding?.let { addHistory(it, session.currentText) }
            }
            sessions.clearCurrent()
            showSyncFullText = false
            statusText = currentBinding?.let { text(R.string.status_connected, it.pcName) }
                ?: text(R.string.status_unpaired)
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
            TargetState.ACTIVE -> text(
                R.string.status_target,
                target.targetName ?: application.getString(R.string.default_computer),
            )
            TargetState.NOT_FOREGROUND -> text(
                R.string.status_target_waiting,
                target.targetName ?: application.getString(R.string.default_computer),
            )
            TargetState.INVALID -> text(R.string.status_target_invalid)
        }
        showSyncFullText = syncClient.requiresExplicitStart()
        scheduleDraftSave()
        notifyChanged()
    }

    override fun onSwitchComputer(pcId: String, completion: (Boolean) -> Unit) = onMain {
        switchFromWindows(pcId, completion)
    }

    private fun switchFromWindows(pcId: String, completion: (Boolean) -> Unit) {
        val binding = bindings.select(pcId) ?: run {
            completion(false)
            return
        }
        refreshBindings()
        // Confirm acceptance before switching tears down the control socket that carried the request.
        completion(true)
        recentActivityPcId = pcId
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
        statusText = text(R.string.status_reconnecting, binding.pcName)
        notifyChanged()
    }

    override fun onFailure(binding: ComputerBinding) = onMain {
        connected = false
        targetState = null
        statusText = text(R.string.status_reconnecting, binding.pcName)
        notifyChanged()
    }

    override fun onServerError(binding: ComputerBinding, error: ErrorMessage) = onMain {
        if (error.code == ErrorCode.TARGET_SUBMITTED) {
            val submittedText = session.currentText
            if (submittedText.isNotEmpty()) addHistory(binding, submittedText)
            sessions.clearCurrent()
            clearAutoSelection()
            connected = true
            targetState = null
            showSyncFullText = false
            statusText = text(R.string.status_connected, binding.pcName)
            notifyChanged()
            return@onMain
        }
        resetFailedSession()
        connected = true
        targetState = null
        showSyncFullText = true
        statusText = when (error.code) {
            ErrorCode.INJECTOR_UNAVAILABLE -> text(R.string.status_input_service_unavailable)
            ErrorCode.RECOVERY_REQUIRED -> text(R.string.status_input_service_recovered)
            ErrorCode.TARGET_MODIFIED -> text(R.string.status_target_modified)
            else -> text(R.string.status_sync_stopped)
        }
        scheduleDraftSave()
        notifyChanged()
    }

    override fun onPairingInvalid(binding: ComputerBinding) = onMain {
        connected = false
        targetState = null
        sessions.remove(binding.pcId)
        bindings.remove(binding.pcId)
        refreshBindings()
        PhoneIdentity().delete(binding.pcId)
        currentBinding = computers.firstOrNull()
        refreshControlClients()
        statusText = text(R.string.status_binding_invalid)
        currentBinding?.let {
            syncClient.resetForTargetSelection()
            restoreSessionFor(it)
            connect(it)
        } ?: notifyChanged()
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
        statusText = text(R.string.status_connecting, binding.pcName)
        syncClient.connect(binding)
        notifyChanged()
    }

    private fun switchToComputer(binding: ComputerBinding) {
        saveDraftNow()
        syncClient.resetForTargetSelection()
        restoreSessionFor(binding)
        clearAutoSelection()
        targetState = null
        manualStartPending = session.sessionId == null && session.currentText.isNotEmpty()
        connect(binding)
    }

    /** Keep a rejected session as a local draft until the user explicitly
     * chooses a new Windows insertion point and synchronizes the full text.
     */
    private fun resetFailedSession() {
        val text = session.currentText
        val oldSessionId = session.sessionId
        clearAutoSelection()
        oldSessionId?.let(syncClient::abandonSession)
        if (oldSessionId != null || session.finishing) session.resetForReplacement(text)
        targetState = null
        manualStartPending = true
        showSyncFullText = text.isNotEmpty()
    }

    private fun beginAutoSelection() {
        if (autoSelecting || !shouldBeginAutoSelection(
                settings.autoSelectComputer,
                session.sessionId != null,
                manualStartPending,
            )
        ) {
            return
        }
        val candidates = computers.filter { it.pairingToken == null }
        if (candidates.isEmpty()) {
            failAutoSelection(R.string.status_auto_no_computer, currentBinding)
            return
        }
        autoSelecting = true
        autoSelectionError = null
        manualStartPending = false
        pendingAutoStart = null
        pendingAutoLatest = null
        autoSelectionTargetPcId = null
        statusText = text(R.string.status_auto_selecting)
        val fallback = currentBinding
        saveDraftNow()
        syncClient.resetForTargetSelection()
        autoSelection.choose(candidates, fallback?.pcId) { winner ->
            onMain {
                if (!autoSelecting) return@onMain
                if (winner == null) {
                    failAutoSelection(R.string.status_auto_ambiguous, fallback)
                } else {
                    autoSelectionTargetPcId = winner.pcId
                    recentActivityPcId = winner.pcId
                    if (!connectAutoSelected(winner)) {
                        failAutoSelection(R.string.status_auto_ambiguous, fallback)
                    }
                }
            }
        }
    }

    private fun failAutoSelection(messageResource: Int, fallback: ComputerBinding?) {
        autoSelecting = false
        autoSelectionTargetPcId = null
        pendingAutoStart = null
        pendingAutoLatest = null
        val message = text(messageResource)
        autoSelectionError = message
        showSyncFullText = session.currentText.isNotEmpty()
        fallback?.let {
            restoreSessionFor(it)
            connect(it)
        }
        statusText = message
        notifyChanged()
    }

    private fun text(resourceId: Int, vararg arguments: Any) =
        LocalizedText(resourceId, arguments.toList())

    private fun clearAutoSelection() {
        autoSelection.cancel()
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
        }
        notifyChanged()
    }

    private fun refreshBindings() {
        computers = bindings.list()
    }

    private fun addHistory(binding: ComputerBinding, text: String) {
        if (text.isNotEmpty()) storage.execute(action = { history.add(binding, text) })
    }

    private fun scheduleDraftSave() {
        mainHandler.removeCallbacks(saveDraft)
        mainHandler.postDelayed(saveDraft, 200)
    }

    private fun saveDraftNow() {
        mainHandler.removeCallbacks(saveDraft)
        sessions.saveCurrent(syncClient.remoteStarted())
    }

    private fun restoreSessionFor(binding: ComputerBinding) {
        val restored = sessions.activate(binding.pcId)
        manualStartPending = restoredDraftRequiresExplicitStart(
            session.sessionId != null,
            session.currentText.isNotEmpty(),
        )
        restored?.let(::restoreQueue)
    }

    private fun restoreQueue(restored: ComputerSessions.ParkedSession) {
        session.recoverySnapshot()?.let {
            syncClient.restore(it, restored.state.acknowledgedSequence, restored.remoteStarted)
        }
    }

    private fun connectAutoSelected(binding: ComputerBinding): Boolean {
        when (val activation = sessions.activateForAutomaticSelection(binding.pcId)) {
            ComputerSessions.AutomaticActivation.Conflict -> return false
            is ComputerSessions.AutomaticActivation.Activated -> {
                activation.parked?.let(::restoreQueue)
                if (activation.parked == null) sessions.saveCurrent(remoteStarted = false)
            }
        }
        connect(binding)
        return true
    }

    private fun notifyChanged() {
        val state = state()
        observers.forEach { it(state) }
    }

    private fun onMain(action: () -> Unit) {
        if (Looper.myLooper() == Looper.getMainLooper()) action() else mainHandler.post(action)
    }

    private companion object {
        const val BACKGROUND_DISCONNECT_DELAY_MS = 500L
    }
}

internal fun shouldBeginAutoSelection(
    enabled: Boolean,
    hasActiveSession: Boolean,
    explicitStartRequired: Boolean,
): Boolean = enabled && !hasActiveSession && !explicitStartRequired

internal fun restoredDraftRequiresExplicitStart(
    hasActiveSession: Boolean,
    hasText: Boolean,
): Boolean = !hasActiveSession && hasText
