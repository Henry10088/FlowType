package app.flowtype.data

import android.content.ContentValues
import android.content.Context
import android.database.sqlite.SQLiteDatabase
import android.database.sqlite.SQLiteOpenHelper

class AppDatabase(context: Context, name: String = NAME) : SQLiteOpenHelper(context, name, null, VERSION) {
    override fun onConfigure(db: SQLiteDatabase) {
        db.setForeignKeyConstraintsEnabled(true)
    }

    override fun onCreate(db: SQLiteDatabase) {
        db.execSQL(
            """CREATE TABLE settings (
                key TEXT PRIMARY KEY NOT NULL,
                value TEXT NOT NULL
            )""",
        )
        db.execSQL(
            """CREATE TABLE computers (
                pc_id TEXT PRIMARY KEY NOT NULL,
                pc_name TEXT NOT NULL,
                endpoint TEXT NOT NULL,
                endpoint_candidates TEXT NOT NULL DEFAULT '[]',
                tls_spki_sha256 TEXT NOT NULL,
                pairing_token TEXT,
                selected INTEGER NOT NULL DEFAULT 0,
                auto_select INTEGER NOT NULL DEFAULT 1,
                added_at INTEGER NOT NULL
            )""",
        )
        db.execSQL(
            """CREATE TABLE history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                pc_id TEXT NOT NULL,
                pc_name TEXT NOT NULL,
                completed_at INTEGER NOT NULL,
                encrypted_text BLOB NOT NULL
            )""",
        )
        db.execSQL("CREATE INDEX history_completed_at ON history(completed_at DESC)")
    }

    override fun onUpgrade(db: SQLiteDatabase, oldVersion: Int, newVersion: Int) {
        if (oldVersion < 2) {
            db.execSQL("ALTER TABLE computers ADD COLUMN auto_select INTEGER NOT NULL DEFAULT 1")
        }
        if (oldVersion < 3) {
            db.execSQL("ALTER TABLE computers ADD COLUMN endpoint_candidates TEXT NOT NULL DEFAULT '[]'")
        }
    }

    fun setting(key: String): String? = readableDatabase.query(
        "settings",
        arrayOf("value"),
        "key = ?",
        arrayOf(key),
        null,
        null,
        null,
    ).use { if (it.moveToFirst()) it.getString(0) else null }

    fun setSetting(key: String, value: String) {
        writableDatabase.insertWithOnConflict(
            "settings",
            null,
            ContentValues().apply {
                put("key", key)
                put("value", value)
            },
            SQLiteDatabase.CONFLICT_REPLACE,
        )
    }

    companion object {
        private const val NAME = "flowtype-v1.db"
        private const val VERSION = 3
    }
}
