package app.flowtype

import android.app.Application
import android.content.Context

class FlowTypeApplication : Application() {
    lateinit var controller: FlowTypeController
        private set

    override fun attachBaseContext(base: Context) {
        super.attachBaseContext(LanguageManager.wrap(base))
    }

    override fun onCreate() {
        super.onCreate()
        controller = FlowTypeController(this)
        controller.start()
    }
}
