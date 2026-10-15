package app.flowtype.filetransfer

import java.util.concurrent.ExecutorCompletionService
import java.util.concurrent.ExecutionException
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit

/** Returns only after every writer is stopped, including on the first failure. */
internal fun <T> runTransferWorkers(items: List<T>, count: Int, cancelIo: () -> Unit, action: (T) -> Unit) {
    if (items.isEmpty()) return
    val executor = Executors.newFixedThreadPool(count.coerceIn(1, 2)) { runnable ->
        Thread(runnable, "flowtype-file-worker").apply { isDaemon = true }
    }
    val completions = ExecutorCompletionService<Unit>(executor)
    val futures = items.map { item -> completions.submit(java.util.concurrent.Callable { action(item) }) }
    var interrupted = false
    try {
        repeat(items.size) { completions.take().get() }
    } catch (error: Throwable) {
        cancelIo()
        futures.forEach { it.cancel(true) }
        if (error is InterruptedException) interrupted = true
        throw if (error is ExecutionException) error.cause ?: error else error
    } finally {
        executor.shutdownNow()
        while (!executor.isTerminated) {
            try { executor.awaitTermination(1, TimeUnit.SECONDS) }
            catch (_: InterruptedException) { interrupted = true }
        }
        if (interrupted) Thread.currentThread().interrupt()
    }
}
