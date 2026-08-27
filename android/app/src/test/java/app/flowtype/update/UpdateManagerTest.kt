package app.flowtype.update

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class UpdateManagerTest {
    @Test
    fun parsesStrictReleaseManifest() {
        val manifest = UpdateManager.parseAndValidateManifest(manifestJson().toByteArray())

        assertEquals("0.2.0", manifest.version)
        assertEquals(20, manifest.versionCode)
        assertEquals(64, manifest.android.sha256.length)
    }

    @Test(expected = IllegalArgumentException::class)
    fun rejectsAssetOutsideMatchingReleaseTag() {
        UpdateManager.parseAndValidateManifest(
            manifestJson().replace("download/android-v0.2.0/", "download/android-v0.1.9/").toByteArray(),
        )
    }

    @Test
    fun acceptsOnlyStrictThreePartVersions() {
        assertNotNull(UpdateManager.parseVersion("0.2.0"))
        assertNotNull(UpdateManager.parseVersion("12.34.56"))
        assertNull(UpdateManager.parseVersion("v0.2.0"))
        assertNull(UpdateManager.parseVersion("0.2"))
        assertNull(UpdateManager.parseVersion("00.2.0"))
    }

    @Test
    fun comparesCertificateSetsWithoutDependingOnOrder() {
        val first = byteArrayOf(1, 2, 3)
        val second = byteArrayOf(4, 5, 6)

        assertTrue(UpdateManager.sameCertificates(listOf(first, second), listOf(second, first)))
        assertFalse(UpdateManager.sameCertificates(listOf(first), listOf(second)))
        assertFalse(UpdateManager.sameCertificates(emptyList(), emptyList()))
    }

    private fun manifestJson() = """
        {
          "schema": 2,
          "key_id": "flowtype-update-2026-v2",
          "platform": "android",
          "version": "0.2.0",
          "published_at": "2026-08-26T10:00:00Z",
          "release_url": "https://github.com/Henry10088/FlowType/releases/tag/android-v0.2.0",
          "notes_zh_cn": "测试更新",
          "windows": {
            "url": "",
            "sha256": "",
            "size": 0
          },
          "android": {
            "version_code": 20,
            "url": "https://github.com/Henry10088/FlowType/releases/download/android-v0.2.0/FlowType-0.2.0-android-release.apk",
            "sha256": "${"b".repeat(64)}",
            "size": 200
          }
        }
    """.trimIndent()
}
