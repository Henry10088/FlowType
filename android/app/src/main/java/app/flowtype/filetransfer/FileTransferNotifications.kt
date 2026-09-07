package app.flowtype.filetransfer

import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import androidx.core.app.NotificationCompat
import app.flowtype.MainActivity
import app.flowtype.R

object FileTransferNotifications {
    private const val CHANNEL_ID = "file-transfer-events"
    private const val INCOMING_ID = 104
    private const val RESULT_ID = 105

    fun showIncoming(context: Context, computerName: String) {
        manager(context).notify(
            INCOMING_ID,
            builder(context)
                .setContentTitle(context.getString(R.string.file_incoming_title))
                .setContentText(context.getString(R.string.file_incoming_message, computerName))
                .setOngoing(false)
                .setAutoCancel(true)
                .build(),
        )
    }

    fun showCompleted(context: Context) {
        manager(context).notify(
            RESULT_ID,
            builder(context)
                .setContentTitle(context.getString(R.string.file_transfer_complete))
                .setContentText(context.getString(R.string.file_transfer_complete_message))
                .setOngoing(false)
                .setAutoCancel(true)
                .build(),
        )
    }

    fun showFailed(context: Context) {
        manager(context).notify(
            RESULT_ID,
            builder(context)
                .setContentTitle(context.getString(R.string.file_transfer_failed_title))
                .setContentText(context.getString(R.string.file_transfer_failed_message))
                .setOngoing(false)
                .setAutoCancel(true)
                .build(),
        )
    }

    fun clearIncoming(context: Context) {
        manager(context).cancel(INCOMING_ID)
    }

    private fun builder(context: Context): NotificationCompat.Builder {
        ensureChannel(context)
        return NotificationCompat.Builder(context, CHANNEL_ID)
            .setSmallIcon(android.R.drawable.stat_sys_upload_done)
            .setContentIntent(
                PendingIntent.getActivity(
                    context,
                    31,
                    Intent(context, MainActivity::class.java)
                        .addFlags(Intent.FLAG_ACTIVITY_CLEAR_TOP or Intent.FLAG_ACTIVITY_SINGLE_TOP),
                    PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
                ),
            )
            .setPriority(NotificationCompat.PRIORITY_DEFAULT)
    }

    private fun manager(context: Context): NotificationManager =
        context.getSystemService(NotificationManager::class.java)

    private fun ensureChannel(context: Context) {
        manager(context).createNotificationChannel(
            NotificationChannel(
                CHANNEL_ID,
                context.getString(R.string.file_transfer_events_channel),
                NotificationManager.IMPORTANCE_DEFAULT,
            ),
        )
    }
}
