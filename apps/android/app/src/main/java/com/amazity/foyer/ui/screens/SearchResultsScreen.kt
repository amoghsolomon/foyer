package com.amazity.foyer.ui.screens

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import com.amazity.foyer.model.BookmarksCatalog
import com.amazity.foyer.model.CalendarCatalog
import com.amazity.foyer.model.ContactsCatalog
import com.amazity.foyer.model.FoyerUiState
import com.amazity.foyer.model.LauncherApp
import com.amazity.foyer.model.NotesCatalog
import com.amazity.foyer.model.TasksCatalog
import com.amazity.foyer.ui.components.ContentStatePanel
import com.amazity.foyer.ui.components.FoyerScreen
import com.amazity.foyer.ui.components.HairlineDivider
import com.amazity.foyer.ui.components.NestedScreenHeader
import com.amazity.foyer.ui.components.SectionLabel
import com.amazity.foyer.ui.theme.FoyerText
import com.amazity.foyer.ui.theme.FoyerTextDim

@Composable
fun SearchResultsScreen(
    query: String,
    state: FoyerUiState,
    notes: NotesCatalog,
    tasks: TasksCatalog,
    calendar: CalendarCatalog,
    contacts: ContactsCatalog,
    bookmarks: BookmarksCatalog,
    onBack: () -> Unit,
    onOpenApp: (LauncherApp) -> Unit,
    onOpenNote: (String) -> Unit,
    onOpenActivity: (String) -> Unit,
    onOpenTask: (String) -> Unit,
    onOpenEvent: (String) -> Unit,
    onOpenContact: (String) -> Unit,
    onOpenBookmark: (String) -> Unit,
) {
    val normalized = query.trim()
    val apps = state.apps.filter { it.name.contains(normalized, ignoreCase = true) }
    val matchingNotes = notes.notes.filter {
        it.title.contains(normalized, true) || it.summary.contains(normalized, true) || it.body.contains(normalized, true)
    }
    val activities = state.tasks.filter { it.title.contains(normalized, true) || it.subtitle.contains(normalized, true) }
    val matchingTasks = tasks.tasks.filter {
        it.title.contains(normalized, true) || it.description.contains(normalized, true)
    }
    val matchingEvents = calendar.events.filter {
        it.summary.contains(normalized, true) ||
            it.description.contains(normalized, true) ||
            it.location.contains(normalized, true)
    }
    val matchingContacts = contacts.search(normalized)
    val matchingBookmarks = bookmarks.visibleBookmarks(normalized)
    val resultCount = apps.size + matchingNotes.size + activities.size +
        matchingTasks.size + matchingEvents.size + matchingContacts.size + matchingBookmarks.size

    FoyerScreen {
        Column(modifier = Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(horizontal = 24.dp)) {
            NestedScreenHeader(title = "Search", onBack = onBack)
            HairlineDivider()
            Spacer(Modifier.height(18.dp))
            Text("Results for “$normalized”", style = MaterialTheme.typography.headlineMedium, color = FoyerText)
            Spacer(Modifier.height(7.dp))
            Text("$resultCount results across Foyer", style = MaterialTheme.typography.bodySmall, color = FoyerTextDim)
            Spacer(Modifier.height(24.dp))
            if (resultCount == 0) {
                ContentStatePanel("No results", "Try an app name, a phrase from a note, or a task title.")
            }
            ResultSection("Apps", apps, { it.name }, { "Open app" }, onOpenApp)
            ResultSection("Notes", matchingNotes, { it.title }, { notes.folder(it.folderId)?.name ?: "Notes" }, { onOpenNote(it.id) })
            ResultSection("Activity", activities, { it.title }, { it.subtitle }, { onOpenActivity(it.id) })
            ResultSection("Tasks", matchingTasks, { it.title }, { tasks.list(it.listId)?.name ?: "Task" }, { onOpenTask(it.id) })
            ResultSection(
                "Calendar",
                matchingEvents,
                { it.summary },
                { calendar.calendar(it.calendarId)?.displayName ?: "Event" },
                { onOpenEvent(it.id) },
            )
            ResultSection("Contacts", matchingContacts, { it.displayName }, { it.subtitle().ifBlank { "Contact" } }, { onOpenContact(it.id) })
            ResultSection("Bookmarks", matchingBookmarks, { it.title }, { it.host }, { onOpenBookmark(it.id) })
            Spacer(Modifier.height(24.dp))
        }
    }
}

@Composable
private fun <T> ResultSection(
    label: String,
    items: List<T>,
    title: (T) -> String,
    subtitle: (T) -> String,
    onClick: (T) -> Unit,
) {
    if (items.isEmpty()) return
    SectionLabel(label, modifier = Modifier.padding(top = 16.dp, bottom = 5.dp))
    items.forEachIndexed { index, item ->
        Column(modifier = Modifier.fillMaxWidth().clickable { onClick(item) }.padding(vertical = 11.dp)) {
            Text(title(item), style = MaterialTheme.typography.bodyLarge, color = FoyerText)
            Text(subtitle(item), style = MaterialTheme.typography.bodySmall, color = FoyerTextDim)
        }
        if (index != items.lastIndex) HairlineDivider()
    }
}
