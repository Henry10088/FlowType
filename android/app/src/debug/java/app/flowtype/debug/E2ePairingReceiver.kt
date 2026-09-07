package app.flowtype.debug

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.util.Base64
import android.net.Uri
import app.flowtype.FlowTypeApplication
import app.flowtype.pairing.BindingStore

/** Debug-build-only bridge used by the cross-device integration test. */
class E2ePairingReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action == ACTION_CANCEL_FILES) {
            (context.applicationContext as FlowTypeApplication).controller.cancelFileTransfer()
            return
        }
        if (intent.action == ACTION_SEND_FILES) {
            val directory = context.cacheDir.resolve("e2e-files").apply { mkdirs() }
            directory.resolve("测试-😀.txt").writeText("FlowType 双向传输测试\nline 2")
            directory.resolve("empty.bin").writeBytes(byteArrayOf())
            directory.resolve("resume.bin").outputStream().use { output ->
                val block = ByteArray(1024 * 1024) { index -> (index % 251).toByte() }
                var remaining = intent.getLongExtra(EXTRA_RESUME_BYTES, 12L * block.size)
                    .coerceIn(0, 1024L * 1024 * 1024)
                while (remaining > 0) {
                    val count = minOf(remaining, block.size.toLong()).toInt()
                    output.write(block, 0, count)
                    remaining -= count
                }
            }
            val uris = directory.listFiles().orEmpty().sortedBy { it.name }.map { file ->
                Uri.parse("content://app.flowtype.e2e/files/${Uri.encode(file.name)}")
            }
            (context.applicationContext as FlowTypeApplication).controller.sendFiles(uris)
            return
        }
        if (intent.action != ACTION_PAIR) return
        val uri = intent.getStringExtra(EXTRA_URI)
            ?: intent.getStringExtra(EXTRA_URI_BASE64)?.let {
                String(Base64.decode(it, Base64.URL_SAFE or Base64.NO_WRAP or Base64.NO_PADDING))
            }
            ?: return
        runCatching {
            val binding = BindingStore.parse(uri)
            (context.applicationContext as FlowTypeApplication).controller.acceptBinding(binding)
            context.filesDir.resolve("e2e-pairing-result.txt").writeText("accepted")
        }.onFailure { error ->
            context.filesDir.resolve("e2e-pairing-result.txt")
                .writeText("failed: ${error::class.java.simpleName}: ${error.message}")
        }
    }

    private companion object {
        const val ACTION_PAIR = "app.flowtype.E2E_PAIR"
        const val EXTRA_URI = "uri"
        const val EXTRA_URI_BASE64 = "uri_base64"
        const val ACTION_SEND_FILES = "app.flowtype.E2E_SEND_FILES"
        const val ACTION_CANCEL_FILES = "app.flowtype.E2E_CANCEL_FILES"
        const val EXTRA_RESUME_BYTES = "resume_bytes"
    }
}
