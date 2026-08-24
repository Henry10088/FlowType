package app.flowtype.data

class SettingsStore(private val database: AppDatabase) {
    var keepScreenOn: Boolean
        get() = database.setting(KEEP_SCREEN_ON)?.toBooleanStrictOrNull() ?: true
        set(value) = database.setSetting(KEEP_SCREEN_ON, value.toString())

    var extraDim: Boolean
        get() = database.setting(EXTRA_DIM)?.toBooleanStrictOrNull() ?: false
        set(value) = database.setSetting(EXTRA_DIM, value.toString())

    var floatingInput: Boolean
        get() = database.setting(FLOATING_INPUT)?.toBooleanStrictOrNull() ?: false
        set(value) = database.setSetting(FLOATING_INPUT, value.toString())

    var autoSelectComputer: Boolean
        get() = database.setting(AUTO_SELECT_COMPUTER)?.toBooleanStrictOrNull() ?: false
        set(value) = database.setSetting(AUTO_SELECT_COMPUTER, value.toString())

    companion object {
        private const val KEEP_SCREEN_ON = "keep_screen_on"
        private const val EXTRA_DIM = "extra_dim"
        private const val FLOATING_INPUT = "floating_input"
        private const val AUTO_SELECT_COMPUTER = "auto_select_computer"
    }
}
