package app.flowtype.filetransfer

import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.os.IBinder
import androidx.core.app.NotificationCompat
import androidx.core.content.ContextCompat
import app.flowtype.FlowTypeApplication
import app.flowtype.FlowTypeController
import app.flowtype.MainActivity
import app.flowtype.R

/** Keeps the already-persisted transfer state machine alive while the UI is
 * backgrounded. It owns no second queue; the application controller remains
 * the single transfer executor and source of progress truth. */
class FileTransferService : Service() {
    private lateinit var controller: FlowTypeController
    private val observer: (FlowTypeController.UiState) -> Unit = { state ->
        val transfer = state.fileTransfer
        if (!transfer.active) {
            stopSelf()
        } else {
            getSystemService(NotificationManager::class.java).notify(
                NOTIFICATION_ID,
                notification(transfer.completedBytes, transfer.totalBytes),
            )
        }
    }

    override fun onCreate() {
        super.onCreate()
        controller = (application as FlowTypeApplication).controller
        getSystemService(NotificationManager::class.java).createNotificationChannel(
            NotificationChannel(
                CHANNEL_ID,
                getString(R.string.file_transfer_channel),
                NotificationManager.IMPORTANCE_LOW,
            ).apply {
                setShowBadge(false)
                setSound(null, null)
            },
        )
        startForeground(NOTIFICATION_ID, notification(0, 0))
        controller.observe(observer)
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (intent?.action == ACTION_CANCEL) controller.cancelFileTransfer()
        return START_STICKY
    }

    override fun onDestroy() {
        controller.removeObserver(observer)
        super.onDestroy()
    }

    override fun onBind(intent: Intent?): IBinder? = null

    private fun notification(completed: Long, total: Long) =
        NotificationCompat.Builder(this, CHANNEL_ID)
            .setSmallIcon(android.R.drawable.stat_sys_upload)
            .setContentTitle(getString(R.string.file_transfer_channel))
            .setContentText(getString(R.string.file_transfer_notification))
            .setContentIntent(
                PendingIntent.getActivity(
                    this,
                    11,
                    Intent(this, MainActivity::class.java),
                    PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
                ),
            )
            .addAction(
                0,
                getString(R.string.cancel),
                PendingIntent.getService(
                    this,
                    12,
                    Intent(this, FileTransferService::class.java).setAction(ACTION_CANCEL),
                    PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
                ),
            )
            .setProgress(
                1000,
                if (total > 0) completed.coerceAtMost(total).toDouble()
                    .div(total.toDouble()).times(1000).toInt() else 0,
                total <= 0,
            )
            .setOngoing(true)
            .setSilent(true)
            .setPriority(NotificationCompat.PRIORITY_LOW)
            .build()

    companion object {
        private const val CHANNEL_ID = "file-transfers"
        private const val NOTIFICATION_ID = 102
        private const val ACTION_CANCEL = "app.flowtype.filetransfer.CANCEL"

        fun start(context: Context) {
            ContextCompat.startForegroundService(context, Intent(context, FileTransferService::class.java))
        }

        fun stop(context: Context) {
            context.stopService(Intent(context, FileTransferService::class.java))
        }
    }
}
