package app.flowtype

import android.content.pm.ActivityInfo
import android.os.Bundle
import android.view.MotionEvent
import android.widget.ImageButton
import com.journeyapps.barcodescanner.CaptureActivity
import com.journeyapps.barcodescanner.DecoratedBarcodeView

/** ZXing capture surface kept portrait to match the rest of the phone workflow. */
class PortraitCaptureActivity : CaptureActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        requestedOrientation = ActivityInfo.SCREEN_ORIENTATION_PORTRAIT
        super.onCreate(savedInstanceState)
        findViewById<ImageButton>(R.id.closeScanner).setOnClickListener {
            setResult(RESULT_CANCELED)
            finish()
        }
    }

    override fun initializeContent(): DecoratedBarcodeView {
        setContentView(R.layout.activity_scanner)
        return findViewById(R.id.scannerView)
    }

    override fun dispatchTouchEvent(event: MotionEvent): Boolean {
        if (event.actionMasked == MotionEvent.ACTION_UP) {
            val close = findViewById<ImageButton>(R.id.closeScanner)
            val location = IntArray(2)
            close?.getLocationOnScreen(location)
            if (close != null &&
                event.rawX >= location[0] && event.rawX < location[0] + close.width &&
                event.rawY >= location[1] && event.rawY < location[1] + close.height
            ) {
                close.performClick()
                return true
            }
        }
        return super.dispatchTouchEvent(event)
    }
}
