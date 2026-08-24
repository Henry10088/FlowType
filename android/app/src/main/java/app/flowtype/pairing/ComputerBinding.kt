package app.flowtype.pairing

import android.content.ContentValues
import android.content.Context
import android.database.sqlite.SQLiteDatabase
import android.net.Uri
import android.util.Base64
import app.flowtype.data.AppDatabase
import app.flowtype.protocol.PROTOCOL_VERSION
import org.json.JSONObject
import org.json.JSONArray
import java.util.UUID

data class ComputerBinding(
    val pcId: String,
    val pcName: String,
    val endpoint: String,
    val tlsSpkiSha256: String,
    val pairingToken: String?,
    val endpoints: List<String> = listOf(endpoint),
) {
    fun withPreferredEndpoint(value: String): ComputerBinding = copy(
        endpoint = value,
        endpoints = (listOf(value) + endpoints).distinct(),
    )

    fun withCandidateEndpoints(values: List<String>): ComputerBinding = copy(
        endpoints = (listOf(endpoint) + values + endpoints).distinct(),
    )

    fun nextEndpoint(): ComputerBinding {
        val candidates = (listOf(endpoint) + endpoints).distinct()
        if (candidates.size < 2) return this
        val next = candidates[(candidates.indexOf(endpoint) + 1).mod(candidates.size)]
        return copy(endpoint = next, endpoints = candidates)
    }
}

class BindingStore(context: Context, private val database: AppDatabase = AppDatabase(context)) {
    private val preferences = context.getSharedPreferences("binding-v1", Context.MODE_PRIVATE)

    init {
        migrateLegacyBinding()
    }

    val phoneId: String
        get() = database.setting("phone_id") ?: UUID.randomUUID().toString().also {
            database.setSetting("phone_id", it)
        }

    fun load(): ComputerBinding? = query("selected = 1").firstOrNull()

    fun list(): List<ComputerBinding> = query(null)

    fun save(binding: ComputerBinding) {
        val db = database.writableDatabase
        val autoSelect = getAutoSelected(binding.pcId) ?: true
        db.beginTransaction()
        try {
            db.update("computers", ContentValues().apply { put("selected", 0) }, null, null)
            db.insertWithOnConflict(
                "computers",
                null,
                values(binding, selected = true, autoSelect = autoSelect),
                SQLiteDatabase.CONFLICT_REPLACE,
            )
            db.setTransactionSuccessful()
        } finally {
            db.endTransaction()
        }
    }

    fun select(pcId: String): ComputerBinding? {
        val binding = get(pcId) ?: return null
        save(binding)
        return binding
    }

    fun rename(pcId: String, name: String) {
        require(name.isNotBlank())
        database.writableDatabase.update(
            "computers",
            ContentValues().apply { put("pc_name", name.trim()) },
            "pc_id = ?",
            arrayOf(pcId),
        )
    }

    fun updateEndpoint(pcId: String, endpoint: String) {
        val binding = get(pcId) ?: return
        val updated = binding.withPreferredEndpoint(endpoint)
        database.writableDatabase.update(
            "computers",
            ContentValues().apply {
                put("endpoint", updated.endpoint)
                put("endpoint_candidates", JSONArray(updated.endpoints).toString())
            },
            "pc_id = ?",
            arrayOf(pcId),
        )
    }

    fun isAutoSelected(pcId: String): Boolean = getAutoSelected(pcId) ?: false

    fun autoSelectedIds(): Set<String> = database.readableDatabase.query(
        "computers",
        arrayOf("pc_id"),
        "auto_select = 1",
        null,
        null,
        null,
        null,
    ).use { cursor ->
        buildSet {
            while (cursor.moveToNext()) add(cursor.getString(0))
        }
    }

    fun setAutoSelected(pcId: String, selected: Boolean) {
        database.writableDatabase.update(
            "computers",
            ContentValues().apply { put("auto_select", if (selected) 1 else 0) },
            "pc_id = ?",
            arrayOf(pcId),
        )
    }

    fun remove(pcId: String) {
        val wasSelected = load()?.pcId == pcId
        database.writableDatabase.delete("computers", "pc_id = ?", arrayOf(pcId))
        if (wasSelected) list().firstOrNull()?.let(::save)
    }

    fun clear() = load()?.let { remove(it.pcId) } ?: Unit

    fun markPaired(binding: ComputerBinding): ComputerBinding = binding.copy(pairingToken = null).also(::save)

    private fun get(pcId: String): ComputerBinding? = query("pc_id = ?", arrayOf(pcId)).firstOrNull()

    private fun query(selection: String?, arguments: Array<String>? = null): List<ComputerBinding> =
        database.readableDatabase.query(
            "computers",
            arrayOf(
                "pc_id", "pc_name", "endpoint", "tls_spki_sha256", "pairing_token",
                "endpoint_candidates",
            ),
            selection,
            arguments,
            null,
            null,
            "selected DESC, added_at DESC",
        ).use { cursor ->
            buildList {
                while (cursor.moveToNext()) {
                    add(
                        ComputerBinding(
                            pcId = cursor.getString(0),
                            pcName = cursor.getString(1),
                            endpoint = cursor.getString(2),
                            tlsSpkiSha256 = cursor.getString(3),
                            pairingToken = if (cursor.isNull(4)) null else cursor.getString(4),
                            endpoints = decodeEndpoints(cursor.getString(5), cursor.getString(2)),
                        ),
                    )
                }
            }
        }

    private fun values(binding: ComputerBinding, selected: Boolean, autoSelect: Boolean) = ContentValues().apply {
        put("pc_id", binding.pcId)
        put("pc_name", binding.pcName)
        put("endpoint", binding.endpoint)
        put("endpoint_candidates", JSONArray((listOf(binding.endpoint) + binding.endpoints).distinct()).toString())
        put("tls_spki_sha256", binding.tlsSpkiSha256)
        binding.pairingToken?.let { put("pairing_token", it) } ?: putNull("pairing_token")
        put("selected", if (selected) 1 else 0)
        put("auto_select", if (autoSelect) 1 else 0)
        put("added_at", System.currentTimeMillis())
    }

    private fun getAutoSelected(pcId: String): Boolean? = database.readableDatabase.query(
        "computers",
        arrayOf("auto_select"),
        "pc_id = ?",
        arrayOf(pcId),
        null,
        null,
        null,
    ).use { cursor ->
        if (cursor.moveToFirst()) cursor.getInt(0) != 0 else null
    }

    private fun migrateLegacyBinding() {
        preferences.getString("computer", null)?.let { legacy ->
            if (load() == null) runCatching { save(decodeBinding(JSONObject(legacy))) }
            preferences.edit().remove("computer").apply()
        }
        preferences.getString("phone_id", null)?.let {
            if (database.setting("phone_id") == null) database.setSetting("phone_id", it)
            preferences.edit().remove("phone_id").apply()
        }
    }

    companion object {
        fun parse(uriText: String): ComputerBinding {
            val uri = Uri.parse(uriText)
            require(uri.scheme == "flowtype" && uri.host == "pair")
            val encoded = requireNotNull(uri.getQueryParameter("data"))
            val payload = String(Base64.decode(encoded, Base64.URL_SAFE or Base64.NO_WRAP or Base64.NO_PADDING))
            val value = JSONObject(payload)
            require(value.getInt("protocol_version") == PROTOCOL_VERSION)
            return decodeBinding(value)
        }

        private fun decodeBinding(value: JSONObject) = ComputerBinding(
            pcId = value.getString("pc_id"),
            pcName = value.getString("pc_name"),
            endpoint = value.getString("candidate_endpoint"),
            tlsSpkiSha256 = value.getString("tls_spki_sha256"),
            pairingToken = value.optString("one_time_pairing_token").ifEmpty { null },
            endpoints = value.optJSONArray("candidate_endpoints")?.let { array ->
                buildList {
                    for (index in 0 until array.length()) add(array.getString(index))
                }
            }.orEmpty().ifEmpty { listOf(value.getString("candidate_endpoint")) },
        )

        private fun decodeEndpoints(value: String, fallback: String): List<String> = runCatching {
            val array = JSONArray(value)
            buildList {
                for (index in 0 until array.length()) add(array.getString(index))
            }
        }.getOrDefault(emptyList()).let { (listOf(fallback) + it).distinct() }
    }
}
