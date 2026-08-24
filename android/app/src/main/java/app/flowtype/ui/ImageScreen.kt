package app.flowtype.ui

import android.net.Uri
import android.view.View
import android.widget.Button
import android.widget.CheckBox
import android.widget.ImageView
import android.widget.TextView
import androidx.activity.ComponentActivity
import app.flowtype.R
import app.flowtype.FlowTypeApplication
import app.flowtype.image.ImagePreparer
import java.util.concurrent.Executors

/** Owns image preview/prepare UI and keeps expensive bitmap work off the main thread. */
class ImageScreen(
    private val activity: ComponentActivity,
    private val controller: FlowTypeApplication,
    private val applyInsets: () -> Unit,
    private val onBack: () -> Unit,
    private val onCompleted: () -> Unit,
    private val isVisible: () -> Boolean,
) {
    private val executor = Executors.newSingleThreadExecutor()
    private var selectedImage: Uri? = null
    private var processing = false
    private var successHandled = false

    fun show(uri: Uri) {
        selectedImage = uri
        processing = true
        successHandled = false
        controller.resetImageTransfer()
        activity.setContentView(R.layout.page_image_preview)
        applyInsets()
        activity.findViewById<View>(R.id.back).setOnClickListener { onBack() }
        activity.findViewById<CheckBox>(R.id.sendOriginal).isChecked = false
        activity.findViewById<Button>(R.id.sendImage).apply {
            isEnabled = false
            setOnClickListener { prepareAndSend() }
        }
        activity.findViewById<TextView>(R.id.imageStatus).setText(R.string.image_processing)
        executor.execute {
            runCatching { ImagePreparer.preview(activity.contentResolver, uri) }
                .onSuccess { bitmap -> activity.runOnUiThread {
                    if (!isVisible() || selectedImage != uri) return@runOnUiThread
                    activity.findViewById<ImageView>(R.id.imagePreview).setImageBitmap(bitmap)
                    processing = false
                    activity.findViewById<TextView>(R.id.imageStatus).text = ""
                    activity.findViewById<Button>(R.id.sendImage).isEnabled = controller.state().connected
                } }
                .onFailure { activity.runOnUiThread {
                    if (!isVisible() || selectedImage != uri) return@runOnUiThread
                    processing = false
                    activity.findViewById<TextView>(R.id.imageStatus).setText(R.string.image_failed)
                } }
        }
    }

    fun render(state: FlowTypeApplication.UiState) {
        if (processing) return
        val status = activity.findViewById<TextView>(R.id.imageStatus) ?: return
        val send = activity.findViewById<Button>(R.id.sendImage)
        val original = activity.findViewById<CheckBox>(R.id.sendOriginal)
        when (state.imageTransfer) {
            FlowTypeApplication.ImageTransferState.IDLE -> {
                status.text = if (state.connected) "" else activity.getString(R.string.computer_not_connected)
                send.isEnabled = state.connected
                original.isEnabled = true
            }
            FlowTypeApplication.ImageTransferState.SENDING -> {
                status.setText(R.string.image_transferring)
                send.isEnabled = false
                original.isEnabled = false
            }
            FlowTypeApplication.ImageTransferState.SUCCESS -> {
                status.setText(R.string.image_sent)
                send.isEnabled = false
                if (!successHandled) {
                    successHandled = true
                    status.postDelayed({ if (isVisible()) onCompleted() }, 900)
                }
            }
            FlowTypeApplication.ImageTransferState.FAILED -> {
                status.setText(R.string.image_failed)
                send.isEnabled = state.connected
                original.isEnabled = true
            }
        }
    }

    fun shutdown() {
        executor.shutdownNow()
    }

    private fun prepareAndSend() {
        val uri = selectedImage ?: return
        val original = activity.findViewById<CheckBox>(R.id.sendOriginal).isChecked
        processing = true
        activity.findViewById<Button>(R.id.sendImage).isEnabled = false
        activity.findViewById<CheckBox>(R.id.sendOriginal).isEnabled = false
        activity.findViewById<TextView>(R.id.imageStatus).setText(R.string.image_processing)
        executor.execute {
            runCatching { ImagePreparer.prepare(activity.contentResolver, uri, original) }
                .onSuccess { image -> activity.runOnUiThread {
                    if (!isVisible() || selectedImage != uri) return@runOnUiThread
                    processing = false
                    if (!controller.sendImage(image)) {
                        activity.findViewById<TextView>(R.id.imageStatus).setText(R.string.computer_not_connected)
                        activity.findViewById<Button>(R.id.sendImage).isEnabled = controller.state().connected
                        activity.findViewById<CheckBox>(R.id.sendOriginal).isEnabled = true
                    }
                } }
                .onFailure { activity.runOnUiThread {
                    if (!isVisible() || selectedImage != uri) return@runOnUiThread
                    processing = false
                    activity.findViewById<TextView>(R.id.imageStatus).setText(
                        if (original) R.string.image_too_large else R.string.image_failed,
                    )
                    activity.findViewById<Button>(R.id.sendImage).isEnabled = controller.state().connected
                    activity.findViewById<CheckBox>(R.id.sendOriginal).isEnabled = true
                } }
        }
    }
}
