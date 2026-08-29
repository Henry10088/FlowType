package app.flowtype.data

import android.os.Handler
import android.os.Looper
import android.util.Log
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors

class StorageDispatcher(
    private val mainHandler: Handler = Handler(Looper.getMainLooper()),
    private val executor: ExecutorService = Executors.newSingleThreadExecutor { runnable ->
        Thread(runnable, "flowtype-storage").apply { isDaemon = true }
    },
) {
    fun execute(action: () -> Unit, completion: (() -> Unit)? = null) {
        executor.execute {
            runCatching(action).onFailure {
                Log.e(TAG, "Storage operation failed", it)
            }
            completion?.let { mainHandler.post(it) }
        }
    }

    fun <T> query(action: () -> T, completion: (Result<T>) -> Unit) {
        executor.execute {
            val result = runCatching(action)
            mainHandler.post { completion(result) }
        }
    }

    fun shutdown() {
        executor.shutdown()
    }

    private companion object {
        const val TAG = "StorageDispatcher"
    }
}
