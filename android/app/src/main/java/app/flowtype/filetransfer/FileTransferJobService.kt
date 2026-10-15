package app.flowtype.filetransfer

import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.job.JobParameters
import android.app.job.JobService
import android.content.Intent
import androidx.annotation.RequiresApi
import androidx.core.app.NotificationCompat
import app.flowtype.FlowTypeApplication
import app.flowtype.FlowTypeController
import app.flowtype.MainActivity
import app.flowtype.R

@RequiresApi(34)
class FileTransferJobService : JobService() {
    private lateinit var controller: FlowTypeController
    private var parameters: JobParameters? = null
    private val observer: (FlowTypeController.UiState) -> Unit = { state ->
        val transfer = state.fileTransfer
        val params = parameters
        if (!transfer.active && params != null) {
            parameters = null
            controller.removeObserver(observer)
            jobFinished(params, false)
        } else if (params != null) {
            setNotification(
                params,
                NOTIFICATION_ID,
                notification(transfer.completedBytes, transfer.totalBytes),
                JOB_END_NOTIFICATION_POLICY_REMOVE,
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
    }

    override fun onStartJob(params: JobParameters): Boolean {
        parameters = params
        controller.onFileExecutionStarted()
        setNotification(
            params,
            NOTIFICATION_ID,
            notification(0, 0),
            JOB_END_NOTIFICATION_POLICY_REMOVE,
        )
        controller.observe(observer)
        return true
    }

    override fun onStopJob(params: JobParameters): Boolean {
        parameters = null
        controller.removeObserver(observer)
        controller.onFileExecutionStopped()
        // The scheduler may stop a job for resource reasons. The durable
        // transfer state machine must remain recoverable instead of treating
        // that event as an explicit user cancellation.
        return controller.state().fileTransfer.active
    }

    override fun onDestroy() {
        controller.removeObserver(observer)
        super.onDestroy()
    }

    private fun notification(completed: Long, total: Long) =
        NotificationCompat.Builder(this, CHANNEL_ID)
            .setSmallIcon(android.R.drawable.stat_sys_upload)
            .setContentTitle(getString(R.string.file_transfer_channel))
            .setContentText(getString(R.string.file_transfer_notification))
            .setContentIntent(
                PendingIntent.getActivity(
                    this,
                    21,
                    Intent(this, MainActivity::class.java),
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
        private const val NOTIFICATION_ID = 103
    }
}
