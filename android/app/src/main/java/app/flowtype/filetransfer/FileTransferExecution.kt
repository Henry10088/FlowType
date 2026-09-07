package app.flowtype.filetransfer

import android.app.job.JobInfo
import android.app.job.JobScheduler
import android.content.ComponentName
import android.content.Context
import android.os.Build

object FileTransferExecution {
    fun start(context: Context) {
        if (Build.VERSION.SDK_INT < 34) {
            FileTransferService.start(context)
            return
        }
        val scheduler = context.getSystemService(JobScheduler::class.java)
        if (scheduler.getPendingJob(JOB_ID) != null) return
        val job = JobInfo.Builder(
            JOB_ID,
            ComponentName(context, FileTransferJobService::class.java),
        )
            .setRequiredNetworkType(JobInfo.NETWORK_TYPE_ANY)
            .setUserInitiated(true)
            .build()
        scheduler.schedule(job)
    }

    fun stop(context: Context) {
        if (Build.VERSION.SDK_INT < 34) {
            FileTransferService.stop(context)
        }
        // Android 14+ jobs finish themselves after observing the inactive
        // transfer snapshot. Cancelling here invokes onStopJob and turns a
        // successful completion into an artificial interruption.
    }

    const val JOB_ID = 0x4654
}
