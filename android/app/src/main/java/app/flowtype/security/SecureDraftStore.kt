package app.flowtype.security

import android.content.Context
import android.util.Base64
import app.flowtype.session.InputSession
import org.json.JSONObject

class SecureDraftStore(
    context: Context,
    preferencesName: String = "draft-v1",
    keyAlias: String = KEY_ALIAS,
) {
    data class StoredDraft(
        val session: InputSession.State,
        val remoteStarted: Boolean,
    )

    private val preferences = context.getSharedPreferences(preferencesName, Context.MODE_PRIVATE)
    private val cipher = SecureTextCipher(keyAlias)

    fun save(state: InputSession.State, remoteStarted: Boolean) {
        if (state.sessionId == null && state.text.isEmpty()) {
            clear()
            return
        }
        val plaintext = JSONObject()
            .put("session_id", state.sessionId)
            .put("text", state.text)
            .put("latest_sequence", state.latestSequence)
            .put("acknowledged_sequence", state.acknowledgedSequence)
            .put("finishing", state.finishing)
            .put("remote_started", remoteStarted)
            .toString()
            .toByteArray(Charsets.UTF_8)
        val encrypted = cipher.encrypt(String(plaintext, Charsets.UTF_8))
        preferences.edit()
            .putString("encrypted_state", Base64.encodeToString(encrypted, Base64.NO_WRAP))
            .commit()
    }

    fun load(): StoredDraft? = runCatching {
        val stored = preferences.getString("encrypted_state", null) ?: return null
        val encrypted = Base64.decode(stored, Base64.NO_WRAP)
        val value = JSONObject(cipher.decrypt(encrypted))
        StoredDraft(
            session = InputSession.State(
                sessionId = value.optString("session_id").ifEmpty { null },
                text = value.getString("text"),
                latestSequence = value.getLong("latest_sequence"),
                acknowledgedSequence = value.getLong("acknowledged_sequence"),
                finishing = value.getBoolean("finishing"),
            ),
            remoteStarted = value.optBoolean("remote_started", false),
        )
    }.getOrElse {
        clear()
        null
    }

    fun clear() {
        preferences.edit().remove("encrypted_state").commit()
    }

    companion object {
        private const val KEY_ALIAS = "flowtype-draft-aes-v1"
    }
}
