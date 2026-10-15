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
        createFileTransferTables(db)
    }

    override fun onUpgrade(db: SQLiteDatabase, oldVersion: Int, newVersion: Int) {
        if (oldVersion < 2) {
            db.execSQL("ALTER TABLE computers ADD COLUMN auto_select INTEGER NOT NULL DEFAULT 1")
        }
        if (oldVersion < 3) {
            db.execSQL("ALTER TABLE computers ADD COLUMN endpoint_candidates TEXT NOT NULL DEFAULT '[]'")
        }
        if (oldVersion < 4) {
            createFileTransferTables(db)
        }
        if (oldVersion in 4 until 5) {
            db.execSQL("ALTER TABLE file_transfer_entries ADD COLUMN target_name TEXT")
        }
        if (oldVersion in 4 until 6) {
            db.execSQL("ALTER TABLE file_transfer_entries ADD COLUMN target_parent_uri TEXT")
        }
        if (oldVersion in 4 until 7) {
            db.execSQL("ALTER TABLE file_transfer_entries ADD COLUMN commit_sha256 TEXT")
            db.execSQL("ALTER TABLE file_transfer_entries ADD COLUMN final_uri TEXT")
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
        private const val VERSION = 7

        private fun createFileTransferTables(db: SQLiteDatabase) {
            db.execSQL(
                """CREATE TABLE file_transfer_batches (
                    batch_id TEXT PRIMARY KEY NOT NULL,
                    pc_id TEXT NOT NULL,
                    direction TEXT NOT NULL,
                    state TEXT NOT NULL,
                    created_at INTEGER NOT NULL,
                    updated_at INTEGER NOT NULL,
                    file_count INTEGER NOT NULL,
                    directory_count INTEGER NOT NULL,
                    total_bytes INTEGER NOT NULL,
                    target_tree_uri TEXT
                )""",
            )
            db.execSQL(
                """CREATE TABLE file_transfer_entries (
                    batch_id TEXT NOT NULL,
                    ordinal INTEGER NOT NULL,
                    entry_id TEXT NOT NULL,
                    source_uri TEXT,
                    relative_path TEXT NOT NULL,
                    kind TEXT NOT NULL,
                    size INTEGER NOT NULL,
                    modified_at INTEGER,
                    mime_type TEXT,
                    sha256 TEXT,
                    target_uri TEXT,
                    target_name TEXT,
                    target_parent_uri TEXT,
                    commit_sha256 TEXT,
                    final_uri TEXT,
                    state TEXT NOT NULL,
                    committed_offset INTEGER NOT NULL DEFAULT 0,
                    error_code TEXT,
                    PRIMARY KEY (batch_id, ordinal),
                    UNIQUE (batch_id, entry_id),
                    FOREIGN KEY (batch_id) REFERENCES file_transfer_batches(batch_id) ON DELETE CASCADE
                )""",
            )
            db.execSQL("CREATE INDEX file_transfer_batches_state ON file_transfer_batches(state, created_at)")
            db.execSQL("CREATE INDEX file_transfer_entries_state ON file_transfer_entries(batch_id, state, ordinal)")
        }
    }
}
