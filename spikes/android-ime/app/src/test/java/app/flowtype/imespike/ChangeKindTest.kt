package app.flowtype.imespike

import org.junit.Assert.assertEquals
import org.junit.Test

class ChangeKindTest {
    @Test
    fun classifiesTextWatcherChanges() {
        assertEquals(ChangeKind.ADD, ChangeKind.from(0, 3))
        assertEquals(ChangeKind.DELETE, ChangeKind.from(2, 0))
        assertEquals(ChangeKind.REPLACE, ChangeKind.from(2, 3))
        assertEquals(ChangeKind.UNCHANGED, ChangeKind.from(0, 0))
    }
}
