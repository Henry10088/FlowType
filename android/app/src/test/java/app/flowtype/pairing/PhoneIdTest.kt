package app.flowtype.pairing

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNull
import org.junit.Test

class PhoneIdTest {
    @Test
    fun androidIdProducesStableOpaquePhoneId() {
        val first = phoneIdForAndroidId("android-id-123")
        val second = phoneIdForAndroidId("android-id-123")

        assertEquals(first, second)
        checkNotNull(first)
        assertEquals(64, first!!.length)
        assertNotEquals("android-id-123", first)
    }

    @Test
    fun differentAndroidIdsDoNotSharePhoneId() {
        assertNotEquals(phoneIdForAndroidId("phone-a"), phoneIdForAndroidId("phone-b"))
    }

    @Test
    fun missingAndroidIdUsesRandomFallbackAtCallSite() {
        assertNull(phoneIdForAndroidId(null))
        assertNull(phoneIdForAndroidId("  "))
    }
}
