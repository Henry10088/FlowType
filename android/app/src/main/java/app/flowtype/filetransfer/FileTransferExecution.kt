package app.flowtype.filetransfer

import android.app.job.JobInfo
import android.app.job.JobScheduler
import android.content.ComponentName
import android.content.Context
import android.os.Build
import android.util.Log

object FileTransferExecution {
    fun start(context: Context): Boolean = try {
        if (Build.VERSION.SDK_INT < 34) {
            FileTransferService.start(context)
            true
        } else {
            val scheduler = context.getSystemService(JobScheduler::class.java)
            if (scheduler.getPendingJob(JOB_ID) != null) {
                true
            } else {
                val job = JobInfo.Builder(
                    JOB_ID,
                    ComponentName(context, FileTransferJobService::class.java),
                )
                    .setRequiredNetworkType(JobInfo.NETWORK_TYPE_ANY)
                    .setUserInitiated(true)
                    .build()
                scheduler.schedule(job) == JobScheduler.RESULT_SUCCESS
            }
        }
    } catch (error: IllegalStateException) {
        // System background-start restrictions leave the durable batch intact.
        Log.w("FlowTypeFiles", "Background transfer cannot start yet", error)
        false
    } catch (error: SecurityException) {
        Log.w("FlowTypeFiles", "Background transfer is not allowed", error)
        false
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
