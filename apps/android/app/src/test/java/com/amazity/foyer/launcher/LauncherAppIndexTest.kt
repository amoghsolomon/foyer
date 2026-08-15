package com.amazity.foyer.launcher

import com.amazity.foyer.model.LauncherApp
import org.junit.Assert.assertEquals
import org.junit.Test

class LauncherAppIndexTest {
    private val apps = listOf(
        LauncherApp("Authenticator", "com.example.auth", "AuthActivity"),
        LauncherApp("Calendar", "com.android.calendar", "CalendarActivity"),
        LauncherApp("Camera", "com.android.camera", "CameraActivity"),
        LauncherApp("Settings", "com.android.settings", "SettingsActivity"),
    )

    @Test
    fun filtersByDisplayNameAndPackageName() {
        assertEquals(listOf("Calendar"), filterLauncherApps(apps, "calendar").map { it.name })
        assertEquals(listOf("Settings"), filterLauncherApps(apps, "android.settings").map { it.name })
    }

    @Test
    fun blankQueryKeepsTheExistingOrder() {
        assertEquals(apps, filterLauncherApps(apps, "  "))
    }

    @Test
    fun createsTheFirstIndexForEachVisibleSection() {
        assertEquals(mapOf('A' to 0, 'C' to 1, 'S' to 3), launcherSectionIndices(apps))
    }

    @Test
    fun nonAlphabeticNamesUseTheHashSection() {
        assertEquals('#', launcherSection("  1Password"))
        assertEquals('#', launcherSection(""))
    }
}
