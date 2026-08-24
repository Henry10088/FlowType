package app.flowtype.imespike

enum class ChangeKind(val label: String) {
    ADD("新增"),
    DELETE("删除"),
    REPLACE("替换"),
    UNCHANGED("未变化");

    companion object {
        fun from(removedUtf16: Int, addedUtf16: Int): ChangeKind = when {
            removedUtf16 == 0 && addedUtf16 > 0 -> ADD
            removedUtf16 > 0 && addedUtf16 == 0 -> DELETE
            removedUtf16 > 0 && addedUtf16 > 0 -> REPLACE
            else -> UNCHANGED
        }
    }
}
