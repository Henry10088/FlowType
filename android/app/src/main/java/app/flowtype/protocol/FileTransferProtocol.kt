package app.flowtype.protocol

import org.json.JSONArray
import org.json.JSONObject

const val FILE_TRANSFER_VERSION: Int = 2
const val MAX_PHONE_ID_BYTES: Int = 128
const val MAX_BATCH_ID_BYTES: Int = 64
const val MAX_ENTRY_ID_BYTES: Int = 64
const val MAX_RELATIVE_PATH_BYTES: Int = 4096
const val MAX_ROOT_SUMMARIES: Int = 8
const val MAX_ROOT_SUMMARY_BYTES: Int = 255
const val MAX_MANIFEST_LINE_BYTES: Int = 16 * 1024
const val MIN_TICKET_BYTES: Int = 32
const val MAX_TICKET_TEXT_BYTES: Int = 128

enum class TransferDirection(val wireName: String) {
    PHONE_TO_COMPUTER("phone_to_computer"),
    COMPUTER_TO_PHONE("computer_to_phone"),
}

enum class TransferEntryKind(val wireName: String) {
    FILE("file"),
    DIRECTORY("directory"),
}

enum class TransferBatchState(val wireName: String) {
    WAITING_CONFIRMATION("waiting_confirmation"),
    QUEUED("queued"),
    PREPARING("preparing"),
    TRANSFERRING("transferring"),
    WAITING_NETWORK("waiting_network"),
    NEEDS_RESELECTION("needs_reselection"),
    COMPLETED("completed"),
    PARTIAL_FAILURE("partial_failure"),
    CANCELED("canceled");

    val terminal: Boolean
        get() = this == COMPLETED || this == PARTIAL_FAILURE || this == CANCELED

    fun canTransitionTo(next: TransferBatchState): Boolean {
        if (this == next) return true
        return when (this) {
            WAITING_CONFIRMATION -> next == QUEUED || next == CANCELED
            QUEUED -> next == PREPARING || next == WAITING_NETWORK || next == CANCELED
            PREPARING -> next in setOf(
                TRANSFERRING,
                WAITING_NETWORK,
                NEEDS_RESELECTION,
                PARTIAL_FAILURE,
                CANCELED,
            )
            TRANSFERRING -> next in setOf(
                WAITING_NETWORK,
                NEEDS_RESELECTION,
                COMPLETED,
                PARTIAL_FAILURE,
                CANCELED,
            )
            WAITING_NETWORK -> next in setOf(PREPARING, TRANSFERRING, NEEDS_RESELECTION, CANCELED)
            NEEDS_RESELECTION -> next == PREPARING || next == CANCELED
            COMPLETED, PARTIAL_FAILURE, CANCELED -> false
        }
    }
}

enum class TransferEntryState(val wireName: String) {
    PENDING("pending"),
    TRANSFERRING("transferring"),
    WAITING_NETWORK("waiting_network"),
    NEEDS_RESELECTION("needs_reselection"),
    COMPLETED("completed"),
    FAILED("failed"),
    CANCELED("canceled"),
}

data class FileOfferMessage(
    val phoneId: String,
    val batchId: String,
    val direction: TransferDirection,
    val fileCount: Long,
    val directoryCount: Long,
    val totalBytes: Long,
    val roots: List<String>,
    val protocolVersion: Int = PROTOCOL_VERSION,
    val transferVersion: Int = FILE_TRANSFER_VERSION,
) {
    fun validate(authenticatedPhoneId: String) {
        validateVersions(protocolVersion, transferVersion)
        require(phoneId == authenticatedPhoneId && validIdentifier(phoneId, MAX_PHONE_ID_BYTES)) {
            "invalid phone"
        }
        require(validIdentifier(batchId, MAX_BATCH_ID_BYTES)) { "invalid batch" }
        require(fileCount >= 0 && directoryCount >= 0 && (fileCount > 0 || directoryCount > 0)) {
            "empty batch"
        }
        require(totalBytes >= 0) { "invalid total size" }
        require(roots.size <= MAX_ROOT_SUMMARIES && roots.all(::validRootSummary)) {
            "invalid roots"
        }
    }
}

data class FileDecisionMessage(
    val batchId: String,
    val accepted: Boolean,
    val protocolVersion: Int = PROTOCOL_VERSION,
    val transferVersion: Int = FILE_TRANSFER_VERSION,
) {
    fun validate() {
        validateVersions(protocolVersion, transferVersion)
        require(validIdentifier(batchId, MAX_BATCH_ID_BYTES)) { "invalid batch" }
    }
}

data class FileTicketMessage(
    val batchId: String,
    val dataPort: Int,
    val token: String,
    val expiresAtEpochSeconds: Long,
    val protocolVersion: Int = PROTOCOL_VERSION,
    val transferVersion: Int = FILE_TRANSFER_VERSION,
) {
    fun validate(nowEpochSeconds: Long) {
        validateVersions(protocolVersion, transferVersion)
        require(validIdentifier(batchId, MAX_BATCH_ID_BYTES)) { "invalid batch" }
        require(dataPort in 1..65535) { "invalid port" }
        val decodedBytes = base64UrlDecodedLength(token)
        require(
            decodedBytes != null &&
                decodedBytes >= MIN_TICKET_BYTES &&
                token.toByteArray(Charsets.UTF_8).size <= MAX_TICKET_TEXT_BYTES &&
                expiresAtEpochSeconds > nowEpochSeconds,
        ) { "invalid ticket" }
    }
}

data class FileCancelMessage(
    val batchId: String,
    val protocolVersion: Int = PROTOCOL_VERSION,
    val transferVersion: Int = FILE_TRANSFER_VERSION,
) {
    fun validate() {
        validateVersions(protocolVersion, transferVersion)
        require(validIdentifier(batchId, MAX_BATCH_ID_BYTES)) { "invalid batch" }
    }
}

data class TransferManifestEntry(
    val ordinal: Long,
    val entryId: String,
    val relativePath: String,
    val kind: TransferEntryKind,
    val size: Long,
    val modifiedAtEpochMillis: Long?,
    val mimeType: String?,
    val sha256: String?,
) {
    fun validate() {
        require(ordinal >= 0) { "invalid ordinal" }
        require(validIdentifier(entryId, MAX_ENTRY_ID_BYTES)) { "invalid entry" }
        validateRelativeTransferPath(relativePath)
        require(size >= 0 && (kind != TransferEntryKind.DIRECTORY || size == 0L)) {
            "invalid size"
        }
        require(mimeType == null || (
            mimeType.isNotEmpty() &&
                mimeType.length <= 255 &&
                mimeType.none(Char::isISOControl)
            )) { "invalid MIME type" }
        require(sha256 == null || sha256.matches(Regex("[0-9a-fA-F]{64}"))) {
            "invalid hash"
        }
        require(FileTransferCodec.encodeManifestEntry(this).toByteArray(Charsets.UTF_8).size <= MAX_MANIFEST_LINE_BYTES) {
            "manifest line too large"
        }
    }
}

data class TransferResumeEntry(
    val entryId: String,
    val committedOffset: Long,
    val state: TransferEntryState,
) {
    fun validate(expectedSize: Long) {
        require(validIdentifier(entryId, MAX_ENTRY_ID_BYTES)) { "invalid entry" }
        require(committedOffset in 0..expectedSize) { "invalid offset" }
        require(state != TransferEntryState.COMPLETED || committedOffset == expectedSize) {
            "completed entry has incomplete offset"
        }
    }
}

object FileTransferCodec {
    fun encodeOffer(message: FileOfferMessage): String = JSONObject()
        .put("protocol_version", message.protocolVersion)
        .put("transfer_version", message.transferVersion)
        .put("type", "file_offer")
        .put("phone_id", message.phoneId)
        .put("batch_id", message.batchId)
        .put("direction", message.direction.wireName)
        .put("file_count", message.fileCount)
        .put("directory_count", message.directoryCount)
        .put("total_bytes", message.totalBytes)
        .put("roots", JSONArray(message.roots))
        .toString()

    fun decodeOffer(json: String): FileOfferMessage {
        val value = JSONObject(json)
        require(value.getString("type") == "file_offer") { "wrong message type" }
        val roots = value.getJSONArray("roots")
        return FileOfferMessage(
            phoneId = value.getString("phone_id"),
            batchId = value.getString("batch_id"),
            direction = enumByWireName(value.getString("direction")),
            fileCount = value.getLong("file_count"),
            directoryCount = value.getLong("directory_count"),
            totalBytes = value.getLong("total_bytes"),
            roots = (0 until roots.length()).map(roots::getString),
            protocolVersion = value.getInt("protocol_version"),
            transferVersion = value.getInt("transfer_version"),
        )
    }

    fun encodeDecision(message: FileDecisionMessage): String = JSONObject()
        .put("protocol_version", message.protocolVersion)
        .put("transfer_version", message.transferVersion)
        .put("type", "file_decision")
        .put("batch_id", message.batchId)
        .put("accepted", message.accepted)
        .toString()

    fun decodeTicket(json: String): FileTicketMessage {
        val value = JSONObject(json)
        require(value.getString("type") == "file_ticket") { "wrong message type" }
        return FileTicketMessage(
            batchId = value.getString("batch_id"),
            dataPort = value.getInt("data_port"),
            token = value.getString("token"),
            expiresAtEpochSeconds = value.getLong("expires_at_epoch_seconds"),
            protocolVersion = value.getInt("protocol_version"),
            transferVersion = value.getInt("transfer_version"),
        )
    }

    fun encodeCancel(message: FileCancelMessage): String = JSONObject()
        .put("protocol_version", message.protocolVersion)
        .put("transfer_version", message.transferVersion)
        .put("type", "file_cancel")
        .put("batch_id", message.batchId)
        .toString()

    fun encodeManifestEntry(entry: TransferManifestEntry): String = JSONObject()
        .put("ordinal", entry.ordinal)
        .put("entry_id", entry.entryId)
        .put("relative_path", entry.relativePath)
        .put("kind", entry.kind.wireName)
        .put("size", entry.size)
        .apply {
            entry.modifiedAtEpochMillis?.let { put("modified_at_epoch_millis", it) }
            entry.mimeType?.let { put("mime_type", it) }
            entry.sha256?.let { put("sha256", it) }
        }
        .toString()

    fun decodeManifestEntry(json: String): TransferManifestEntry {
        require(json.toByteArray(Charsets.UTF_8).size <= MAX_MANIFEST_LINE_BYTES) {
            "manifest line too large"
        }
        val value = JSONObject(json)
        return TransferManifestEntry(
            ordinal = value.getLong("ordinal"),
            entryId = value.getString("entry_id"),
            relativePath = value.getString("relative_path"),
            kind = enumByWireName(value.getString("kind")),
            size = value.getLong("size"),
            modifiedAtEpochMillis = value.optLongOrNull("modified_at_epoch_millis"),
            mimeType = value.optString("mime_type").ifEmpty { null },
            sha256 = value.optString("sha256").ifEmpty { null },
        ).also(TransferManifestEntry::validate)
    }

    private inline fun <reified T : Enum<T>> enumByWireName(name: String): T =
        enumValues<T>().single {
            (it as? TransferDirection)?.wireName == name ||
                (it as? TransferEntryKind)?.wireName == name ||
                (it as? TransferEntryState)?.wireName == name ||
                (it as? TransferBatchState)?.wireName == name
        }
}

fun validateRelativeTransferPath(path: String) {
    val bytes = path.toByteArray(Charsets.UTF_8)
    require(
        path.isNotEmpty() &&
            bytes.size <= MAX_RELATIVE_PATH_BYTES &&
            !path.startsWith('/') &&
            !path.endsWith('/') &&
            '\\' !in path &&
            path.none { it == '\u0000' || it.isISOControl() },
    ) { "invalid relative path" }
    val segments = path.split('/')
    require(segments.size <= 128 && segments.all {
        it.isNotEmpty() && it != "." && it != ".." && it.length <= 255
    }) { "invalid relative path" }
}

private fun validateVersions(protocolVersion: Int, transferVersion: Int) {
    require(protocolVersion == PROTOCOL_VERSION && transferVersion == FILE_TRANSFER_VERSION) {
        "unsupported protocol"
    }
}

private fun validIdentifier(value: String, maxBytes: Int): Boolean {
    val bytes = value.toByteArray(Charsets.UTF_8)
    return bytes.isNotEmpty() && bytes.size <= maxBytes && bytes.all {
        val byte = it.toInt() and 0xff
        byte in 'a'.code..'z'.code ||
            byte in 'A'.code..'Z'.code ||
            byte in '0'.code..'9'.code ||
            byte == '-'.code ||
            byte == '_'.code
    }
}

private fun validRootSummary(value: String): Boolean =
    value.isNotEmpty() &&
        value.toByteArray(Charsets.UTF_8).size <= MAX_ROOT_SUMMARY_BYTES &&
        value.none(Char::isISOControl)

private fun base64UrlDecodedLength(value: String): Int? {
    if (value.isEmpty() || value.any { !(it.isLetterOrDigit() || it == '-' || it == '_' || it == '=') }) {
        return null
    }
    val padding = value.takeLastWhile { it == '=' }.length
    if (padding > 2 || '=' in value.dropLast(padding)) return null
    return value.length * 3 / 4 - padding
}

private fun JSONObject.optLongOrNull(name: String): Long? =
    if (has(name) && !isNull(name)) getLong(name) else null
