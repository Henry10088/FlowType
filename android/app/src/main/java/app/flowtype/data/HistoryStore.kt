package app.flowtype.data

import android.content.ContentValues
import app.flowtype.pairing.ComputerBinding
import app.flowtype.security.SecureTextCipher

data class HistoryEntry(
    val id: Long,
    val pcId: String,
    val pcName: String,
    val completedAt: Long,
    val text: String,
)

class HistoryStore(private val database: AppDatabase) {
    private val cipher = SecureTextCipher(KEY_ALIAS)

    fun add(binding: ComputerBinding, text: String, completedAt: Long = System.currentTimeMillis()) {
        if (text.isEmpty()) return
        database.writableDatabase.insertOrThrow(
            "history",
            null,
            ContentValues().apply {
                put("pc_id", binding.pcId)
                put("pc_name", binding.pcName)
                put("completed_at", completedAt)
                put("encrypted_text", cipher.encrypt(text))
            },
        )
    }

    fun list(): List<HistoryEntry> = database.readableDatabase.query(
        "history",
        arrayOf("id", "pc_id", "pc_name", "completed_at", "encrypted_text"),
        null,
        null,
        null,
        null,
        "completed_at DESC",
    ).use { cursor ->
        buildList {
            while (cursor.moveToNext()) {
                runCatching {
                    HistoryEntry(
                        id = cursor.getLong(0),
                        pcId = cursor.getString(1),
                        pcName = cursor.getString(2),
                        completedAt = cursor.getLong(3),
                        text = cipher.decrypt(cursor.getBlob(4)),
                    )
                }.getOrNull()?.let(::add)
            }
        }
    }

    fun get(id: Long): HistoryEntry? = list().firstOrNull { it.id == id }

    fun delete(id: Long) {
        database.writableDatabase.delete("history", "id = ?", arrayOf(id.toString()))
    }

    fun clear() {
        database.writableDatabase.delete("history", null, null)
        cipher.destroyKey()
    }

    companion object {
        private const val KEY_ALIAS = "flowtype-history-aes-v1"
    }
}
