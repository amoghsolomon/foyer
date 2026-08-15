package com.amazity.foyer.launcher

import com.amazity.foyer.model.LauncherApp

fun filterLauncherApps(
    apps: List<LauncherApp>,
    query: String,
): List<LauncherApp> {
    val trimmedQuery = query.trim()
    if (trimmedQuery.isEmpty()) return apps

    return apps.filter { app ->
        app.name.contains(trimmedQuery, ignoreCase = true) ||
            app.packageName.contains(trimmedQuery, ignoreCase = true)
    }
}

fun launcherSection(name: String): Char {
    val firstCharacter = name.trim().firstOrNull()?.uppercaseChar() ?: return '#'
    return if (firstCharacter.isLetter()) firstCharacter else '#'
}

fun launcherSectionIndices(apps: List<LauncherApp>): Map<Char, Int> = buildMap {
    apps.forEachIndexed { index, app ->
        putIfAbsent(launcherSection(app.name), index)
    }
}
