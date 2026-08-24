package app.flowtype.imespike

import org.json.JSONObject

data class SnapshotEvent(
    val sequence: Long,
    val elapsedMs: Long,
    val kind: ChangeKind,
    val startUtf16: Int,
    val removedUtf16: Int,
    val addedUtf16: Int,
    val selectionStart: Int,
    val selectionEnd: Int,
    val composingStart: Int,
    val composingEnd: Int,
    val text: String,
) {
    fun displayLine(): String = buildString {
        append('#').append(sequence).append(' ')
        append(kind.label)
        append(" @").append(startUtf16)
        append(" -").append(removedUtf16)
        append(" +").append(addedUtf16)
        append(" len=").append(text.length)
        append(" sel=").append(selectionStart).append("..").append(selectionEnd)
        if (composingStart >= 0) {
            append(" comp=").append(composingStart).append("..").append(composingEnd)
        }
    }

    fun toJsonLine(): String = JSONObject()
        .put("sequence", sequence)
        .put("elapsed_ms", elapsedMs)
        .put("kind", kind.name.lowercase())
        .put("start_utf16", startUtf16)
        .put("removed_utf16", removedUtf16)
        .put("added_utf16", addedUtf16)
        .put("selection_start", selectionStart)
        .put("selection_end", selectionEnd)
        .put("composing_start", composingStart)
        .put("composing_end", composingEnd)
        .put("text", text)
        .toString()
}
