package app.flowtype

import android.content.Context
import android.content.res.Configuration
import android.content.res.Resources
import java.util.Locale

object LanguageManager {
    enum class Language(val value: String) {
        SYSTEM("system"),
        CHINESE("zh"),
        ENGLISH("en"),
    }

    private const val PREFERENCES = "flowtype_language"
    private const val KEY_LANGUAGE = "language"

    fun current(context: Context): Language {
        val value = context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE)
            .getString(KEY_LANGUAGE, Language.SYSTEM.value)
        return Language.entries.firstOrNull { it.value == value } ?: Language.SYSTEM
    }

    fun set(context: Context, language: Language) {
        context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE)
            .edit()
            .putString(KEY_LANGUAGE, language.value)
            .apply()
        applyTo(context.applicationContext, language)
    }

    fun wrap(context: Context): Context {
        val locale = localeFor(current(context)) ?: return context
        val configuration = Configuration(context.resources.configuration)
        configuration.setLocale(locale)
        return context.createConfigurationContext(configuration)
    }

    @Suppress("DEPRECATION")
    private fun applyTo(context: Context, language: Language) {
        val configuration = Configuration(context.resources.configuration)
        val locale = localeFor(language)
            ?: Resources.getSystem().configuration.locales[0]
        configuration.setLocale(locale)
        context.resources.updateConfiguration(configuration, context.resources.displayMetrics)
    }

    private fun localeFor(language: Language): Locale? = when (language) {
        Language.SYSTEM -> null
        Language.CHINESE -> Locale.SIMPLIFIED_CHINESE
        Language.ENGLISH -> Locale.ENGLISH
    }
}
