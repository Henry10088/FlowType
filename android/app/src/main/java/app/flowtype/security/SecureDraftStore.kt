package app.flowtype.security

import android.content.Context
import android.util.Base64
import app.flowtype.session.InputSession
import app.flowtype.session.ComputerSessions
import org.json.JSONObject

class SecureDraftStore(
    context: Context,
    preferencesName: String = "draft-v1",
    keyAlias: String = KEY_ALIAS,
) {
    private val preferences = context.getSharedPreferences(preferencesName, Context.MODE_PRIVATE)
    private val cipher = SecureTextCipher(keyAlias)

    fun save(pcId: String, draft: ComputerSessions.ParkedSession) {
        val state = draft.state
        if (state.sessionId == null && state.text.isEmpty()) {
            clear(pcId)
            return
        }
        val plaintext = JSONObject()
            .put("session_id", state.sessionId)
            .put("text", state.text)
            .put("latest_sequence", state.latestSequence)
            .put("acknowledged_sequence", state.acknowledgedSequence)
            .put("finishing", state.finishing)
            .put("remote_started", draft.remoteStarted)
            .toString()
            .toByteArray(Charsets.UTF_8)
        val encrypted = cipher.encrypt(String(plaintext, Charsets.UTF_8))
        preferences.edit()
            .putString(storageKey(pcId), Base64.encodeToString(encrypted, Base64.NO_WRAP))
            .commit()
    }

    fun load(pcId: String): ComputerSessions.ParkedSession? = runCatching {
        val key = storageKey(pcId)
        val stored = preferences.getString(key, null)
            ?: preferences.getString(LEGACY_KEY, null)?.also {
                preferences.edit().putString(key, it).remove(LEGACY_KEY).commit()
            }
            ?: return null
        val encrypted = Base64.decode(stored, Base64.NO_WRAP)
        val value = JSONObject(cipher.decrypt(encrypted))
        ComputerSessions.ParkedSession(
            state = InputSession.State(
                sessionId = value.optString("session_id").ifEmpty { null },
                text = value.getString("text"),
                latestSequence = value.getLong("latest_sequence"),
                acknowledgedSequence = value.getLong("acknowledged_sequence"),
                finishing = value.getBoolean("finishing"),
            ),
            remoteStarted = value.optBoolean("remote_started", false),
        )
    }.getOrElse {
        clear(pcId)
        null
    }

    fun clear(pcId: String) {
        preferences.edit().remove(storageKey(pcId)).commit()
    }

    private fun storageKey(pcId: String): String = "encrypted_state_" + Base64.encodeToString(
        pcId.toByteArray(Charsets.UTF_8),
        Base64.NO_WRAP or Base64.URL_SAFE,
    )

    companion object {
        private const val KEY_ALIAS = "flowtype-draft-aes-v1"
        private const val LEGACY_KEY = "encrypted_state"
    }
}
