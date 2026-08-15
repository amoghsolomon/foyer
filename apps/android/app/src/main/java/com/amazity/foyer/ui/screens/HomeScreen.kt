package com.amazity.foyer.ui.screens

import androidx.activity.compose.BackHandler
import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.Spring
import androidx.compose.animation.core.animateDpAsState
import androidx.compose.animation.core.spring
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectVerticalDragGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.pager.HorizontalPager
import androidx.compose.foundation.pager.rememberPagerState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import androidx.compose.ui.zIndex
import com.amazity.foyer.launcher.filterLauncherApps
import com.amazity.foyer.launcher.launcherSection
import com.amazity.foyer.launcher.launcherSectionIndices
import com.amazity.foyer.model.AgentTask
import com.amazity.foyer.model.BookmarksCatalog
import com.amazity.foyer.model.CalendarCatalog
import com.amazity.foyer.model.ContactsCatalog
import com.amazity.foyer.model.FoyerUiState
import com.amazity.foyer.model.HomePanel
import com.amazity.foyer.model.LauncherApp
import com.amazity.foyer.model.MomentInsight
import com.amazity.foyer.model.MomentTarget
import com.amazity.foyer.model.NotesCatalog
import com.amazity.foyer.model.TaskState
import com.amazity.foyer.model.TasksCatalog
import java.time.LocalDate
import com.amazity.foyer.ui.components.FoyerScreen
import com.amazity.foyer.ui.components.MicrophoneGlyph
import com.amazity.foyer.ui.components.SearchGlyph
import com.amazity.foyer.ui.theme.FoyerBlack
import com.amazity.foyer.ui.theme.FoyerLine
import com.amazity.foyer.ui.theme.FoyerLineSubtle
import com.amazity.foyer.ui.theme.FoyerSurface
import com.amazity.foyer.ui.theme.FoyerText
import com.amazity.foyer.ui.theme.FoyerTextDim
import com.amazity.foyer.ui.theme.FoyerTextMuted
import kotlinx.coroutines.launch

@Composable
fun HomeScreen(
    state: FoyerUiState,
    notes: NotesCatalog,
    tasks: TasksCatalog,
    calendar: CalendarCatalog,
    contacts: ContactsCatalog,
    bookmarks: BookmarksCatalog,
    homeRequestVersion: Int,
    selectedPanel: HomePanel,
    onPanelSelected: (HomePanel) -> Unit,
    onOpenActivity: (AgentTask) -> Unit,
    onOpenFolder: (String) -> Unit,
    onOpenNote: (String) -> Unit,
    onCreateNote: () -> Unit,
    onCreateFolder: (String) -> Unit = {},
    onRetryNotes: () -> Unit = {},
    onOpenTaskList: (String) -> Unit,
    onOpenTask: (String) -> Unit,
    onCreateTask: () -> Unit,
    onCreateTaskList: (String) -> Unit = {},
    onRetryTasks: () -> Unit = {},
    onSelectCalendar: (String?) -> Unit,
    onOpenEvent: (String) -> Unit,
    onCreateEvent: () -> Unit,
    onRetryCalendar: () -> Unit = {},
    onSelectAddressBook: (String?) -> Unit,
    contactsSearchQuery: String,
    onContactsSearchQueryChange: (String) -> Unit,
    selectedAddressBookId: String?,
    onOpenContact: (String) -> Unit,
    onCreateContact: () -> Unit,
    onCreateAddressBook: (String) -> Unit = {},
    onRetryContacts: () -> Unit = {},
    onOpenBookmarkFolder: (String) -> Unit,
    onOpenBookmark: (String) -> Unit,
    onCreateBookmark: () -> Unit,
    onCreateBookmarkFolder: (String) -> Unit = {},
    onRetryBookmarks: () -> Unit = {},
    onOpenSearch: (String) -> Unit,
    onOpenVoice: () -> Unit,
    onOpenSettings: () -> Unit,
    onAskAgent: (String) -> Unit,
    onCreateReminder: (String) -> Unit,
    appsLoading: Boolean,
    appsErrorMessage: String?,
    onLaunchApp: (LauncherApp) -> Unit,
) {
    val panels = HomePanel.entries
    val pagerState = rememberPagerState(
        initialPage = selectedPanel.ordinal,
        pageCount = { panels.size },
    )
    val currentOnPanelSelected by rememberUpdatedState(onPanelSelected)
    var query by rememberSaveable { mutableStateOf("") }
    var omnibarExpanded by rememberSaveable { mutableStateOf(false) }
    var selectedCalendarDate by rememberSaveable { mutableStateOf(LocalDate.now().toString()) }
    var visibleCalendarMonth by rememberSaveable {
        mutableStateOf(LocalDate.now().withDayOfMonth(1).toString())
    }
    val selectedDate = remember(selectedCalendarDate) {
        runCatching { LocalDate.parse(selectedCalendarDate) }.getOrDefault(LocalDate.now())
    }
    val visibleMonth = remember(visibleCalendarMonth) {
        runCatching { LocalDate.parse(visibleCalendarMonth) }.getOrDefault(LocalDate.now().withDayOfMonth(1))
    }
    val focusManager = LocalFocusManager.current
    val filteredApps = remember(state.apps, query) {
        filterLauncherApps(state.apps, query)
    }
    val sectionIndices = remember(filteredApps) {
        launcherSectionIndices(filteredApps)
    }
    val appListState = rememberLazyListState()
    val coroutineScope = rememberCoroutineScope()
    val dismissOmnibar = {
        query = ""
        omnibarExpanded = false
        focusManager.clearFocus()
    }

    BackHandler(enabled = omnibarExpanded) {
        dismissOmnibar()
    }

    LaunchedEffect(homeRequestVersion) {
        if (homeRequestVersion > 0) {
            dismissOmnibar()
        }
    }

    LaunchedEffect(selectedPanel) {
        if (pagerState.currentPage != selectedPanel.ordinal) {
            pagerState.animateScrollToPage(selectedPanel.ordinal)
        }
    }
    LaunchedEffect(pagerState) {
        snapshotFlow { pagerState.settledPage }.collect { page ->
            currentOnPanelSelected(panels[page])
        }
    }

    FoyerScreen {
        Box(modifier = Modifier.fillMaxSize()) {
            Column(
                modifier = Modifier
                    .fillMaxSize()
                    .padding(horizontal = 20.dp),
            ) {
                Spacer(Modifier.height(18.dp))
                if (state.dailyMessage.isNotBlank()) {
                    DailyMessageHeader(message = state.dailyMessage)
                    Spacer(Modifier.height(18.dp))
                }
                PillNavigation(
                    selectedPanel = selectedPanel,
                    activityCount = state.tasks.count {
                        it.state == TaskState.Running || it.state == TaskState.Queued
                    },
                    calendarCount = calendar.events.size,
                    taskCount = tasks.openTasks().size,
                    onPanelSelected = onPanelSelected,
                )
                Spacer(Modifier.height(4.dp))

                HorizontalPager(
                    state = pagerState,
                    key = { page -> panels[page].name },
                    modifier = Modifier
                        .fillMaxWidth()
                        .weight(1f),
                ) { page ->
                    when (panels[page]) {
                        HomePanel.Apps -> AppsPage(
                            state = state,
                            apps = filteredApps,
                            query = query,
                            appsLoading = appsLoading,
                            appsErrorMessage = appsErrorMessage,
                            listState = appListState,
                            sectionIndices = sectionIndices,
                            onLaunchApp = onLaunchApp,
                            onMomentClick = { moment ->
                                when (moment.target) {
                                    MomentTarget.Activity -> state.tasks
                                        .firstOrNull { it.id == moment.targetId }
                                        ?.let(onOpenActivity)
                                        ?: onPanelSelected(HomePanel.Activity)
                                    MomentTarget.Calendar -> {
                                        if (calendar.event(moment.targetId) != null) {
                                            onOpenEvent(moment.targetId)
                                        } else {
                                            onPanelSelected(HomePanel.Calendar)
                                        }
                                    }
                                    MomentTarget.Task -> {
                                        if (tasks.task(moment.targetId) != null) {
                                            onOpenTask(moment.targetId)
                                        } else {
                                            onPanelSelected(HomePanel.Tasks)
                                        }
                                    }
                                }
                            },
                            onSelectLetter = { letter ->
                                sectionIndices[letter]?.let { itemIndex ->
                                    coroutineScope.launch {
                                        appListState.scrollToItem(itemIndex)
                                    }
                                }
                            },
                        )

                        HomePanel.Activity -> ActivityPage(
                            state = state,
                            onOpenTask = onOpenActivity,
                        )
                        HomePanel.Calendar -> CalendarPage(
                            catalog = calendar,
                            selectedDate = selectedDate,
                            visibleMonth = visibleMonth,
                            onSelectDate = { selectedCalendarDate = it.toString() },
                            onShiftMonth = { months ->
                                visibleCalendarMonth = visibleMonth.plusMonths(months).withDayOfMonth(1).toString()
                            },
                            onSelectCalendar = onSelectCalendar,
                            onOpenEvent = onOpenEvent,
                            onCreateEvent = onCreateEvent,
                            onRetry = onRetryCalendar,
                        )
                        HomePanel.Tasks -> TasksPage(
                            catalog = tasks,
                            onOpenList = onOpenTaskList,
                            onOpenTask = onOpenTask,
                            onCreateTask = onCreateTask,
                            onCreateList = onCreateTaskList,
                            isLoading = tasks.status.loading,
                            errorMessage = tasks.status.lastError,
                            onRetry = onRetryTasks,
                        )
                        HomePanel.Notes -> NotesPage(
                            catalog = notes,
                            onOpenFolder = onOpenFolder,
                            onOpenNote = onOpenNote,
                            onCreateNote = onCreateNote,
                            onCreateFolder = onCreateFolder,
                            isLoading = notes.status.loading,
                            errorMessage = notes.status.lastError,
                            onRetry = onRetryNotes,
                        )
                        HomePanel.Contacts -> ContactsPage(
                            catalog = contacts,
                            selectedBookId = selectedAddressBookId,
                            searchQuery = contactsSearchQuery,
                            onSearchQueryChange = onContactsSearchQueryChange,
                            onSelectBook = onSelectAddressBook,
                            onOpenContact = onOpenContact,
                            onCreateContact = onCreateContact,
                            onCreateAddressBook = onCreateAddressBook,
                            isLoading = contacts.status.loading,
                            errorMessage = contacts.status.lastError,
                            onRetry = onRetryContacts,
                        )
                        HomePanel.Bookmarks -> BookmarksPage(
                            catalog = bookmarks,
                            onOpenFolder = onOpenBookmarkFolder,
                            onOpenBookmark = onOpenBookmark,
                            onCreateBookmark = onCreateBookmark,
                            onCreateFolder = onCreateBookmarkFolder,
                            isLoading = bookmarks.status.loading,
                            errorMessage = bookmarks.status.lastError,
                            onRetry = onRetryBookmarks,
                        )
                    }
                }
            }

            if (omnibarExpanded) {
                OmnibarResults(
                    query = query,
                    state = state,
                    notes = notes,
                    tasks = tasks,
                    calendar = calendar,
                    contacts = contacts,
                    bookmarks = bookmarks,
                    apps = filteredApps,
                    onLaunchApp = { app ->
                        dismissOmnibar()
                        onLaunchApp(app)
                    },
                    onOpenCalendar = {
                        dismissOmnibar()
                        onPanelSelected(HomePanel.Calendar)
                    },
                    onOpenEvent = { eventId ->
                        dismissOmnibar()
                        onOpenEvent(eventId)
                    },
                    onOpenTask = { taskId ->
                        dismissOmnibar()
                        onOpenTask(taskId)
                    },
                    onOpenContact = { contactId ->
                        dismissOmnibar()
                        onOpenContact(contactId)
                    },
                    onOpenBookmark = { bookmarkId ->
                        dismissOmnibar()
                        onOpenBookmark(bookmarkId)
                    },
                    onOpenNote = { noteId ->
                        dismissOmnibar()
                        onOpenNote(noteId)
                    },
                    onOpenActivity = { activity ->
                        dismissOmnibar()
                        onOpenActivity(activity)
                    },
                    onAskAgent = {
                        val submittedQuery = query.trim()
                        dismissOmnibar()
                        onAskAgent(submittedQuery)
                    },
                    onCreateReminder = {
                        val reminderText = query.trim()
                        dismissOmnibar()
                        onCreateReminder(reminderText)
                    },
                    onOpenVoice = {
                        dismissOmnibar()
                        onOpenVoice()
                    },
                    onOpenSettings = {
                        dismissOmnibar()
                        onOpenSettings()
                    },
                    onOpenFullSearch = {
                        val submittedQuery = query.trim()
                        dismissOmnibar()
                        onOpenSearch(submittedQuery)
                    },
                    modifier = Modifier
                        .align(Alignment.BottomCenter)
                        .padding(start = 20.dp, end = 20.dp, bottom = 72.dp)
                        .zIndex(1f),
                )
            }

            UniversalInputBar(
                query = query,
                onQueryChange = { newQuery -> query = newQuery },
                onFocusChanged = { focused -> omnibarExpanded = focused },
                onSubmit = {
                    val submittedQuery = query.trim()
                    if (submittedQuery.isNotEmpty()) {
                        dismissOmnibar()
                        onAskAgent(submittedQuery)
                    }
                },
                onOpenVoice = {
                    dismissOmnibar()
                    onOpenVoice()
                },
                modifier = Modifier
                    .align(Alignment.BottomCenter)
                    .padding(horizontal = 20.dp, vertical = 12.dp)
                    .zIndex(2f),
            )
        }
    }
}

@Composable
private fun DailyMessageHeader(message: String) {
    Text(
        text = message,
        style = MaterialTheme.typography.bodySmall,
        color = FoyerTextMuted,
        modifier = Modifier.fillMaxWidth(),
    )
}

@Composable
private fun PillNavigation(
    selectedPanel: HomePanel,
    activityCount: Int,
    calendarCount: Int,
    taskCount: Int,
    onPanelSelected: (HomePanel) -> Unit,
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .horizontalScroll(rememberScrollState()),
        horizontalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        HomePanel.entries.forEach { panel ->
            val selected = panel == selectedPanel
            val suffix = when (panel) {
                HomePanel.Apps, HomePanel.Notes, HomePanel.Contacts, HomePanel.Bookmarks -> ""
                HomePanel.Activity -> if (activityCount == 0) "" else " · $activityCount"
                HomePanel.Calendar -> if (calendarCount == 0) "" else " · $calendarCount"
                HomePanel.Tasks -> if (taskCount == 0) "" else " · $taskCount"
            }
            Surface(
                modifier = Modifier
                    .clip(RoundedCornerShape(18.dp))
                    .clickable { onPanelSelected(panel) },
                shape = RoundedCornerShape(18.dp),
                color = if (selected) FoyerText else FoyerBlack,
                contentColor = if (selected) FoyerBlack else FoyerTextMuted,
                border = BorderStroke(1.dp, if (selected) FoyerText else FoyerLine),
            ) {
                Text(
                    text = panel.label + suffix,
                    style = MaterialTheme.typography.labelMedium,
                    modifier = Modifier.padding(horizontal = 11.dp, vertical = 8.dp),
                )
            }
        }
    }
}

@Composable
private fun AppsPage(
    state: FoyerUiState,
    apps: List<LauncherApp>,
    query: String,
    appsLoading: Boolean,
    appsErrorMessage: String?,
    listState: LazyListState,
    sectionIndices: Map<Char, Int>,
    onLaunchApp: (LauncherApp) -> Unit,
    onMomentClick: (MomentInsight) -> Unit,
    onSelectLetter: (Char) -> Unit,
) {
    val currentSectionLetter by remember(listState, sectionIndices, apps.size) {
        derivedStateOf {
            if (apps.isEmpty()) {
                null
            } else {
                val firstVisibleAppIndex = listState.firstVisibleItemIndex
                    .coerceIn(0, apps.lastIndex)
                sectionIndices.entries
                    .lastOrNull { (_, itemIndex) -> itemIndex <= firstVisibleAppIndex }
                    ?.key
                    ?: sectionIndices.keys.firstOrNull()
            }
        }
    }

    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(bottom = 68.dp),
    ) {
        state.moment?.let { moment ->
            Spacer(Modifier.height(10.dp))
            MomentCard(
                moment = moment,
                onClick = { onMomentClick(moment) },
            )
            Spacer(Modifier.height(10.dp))
        }

        Box(modifier = Modifier.weight(1f)) {
            LazyColumn(
                modifier = Modifier.fillMaxSize(),
                state = listState,
                contentPadding = PaddingValues(top = 6.dp, end = 38.dp, bottom = 8.dp),
            ) {
                if (apps.isEmpty()) {
                    item(key = "empty-app-list") {
                        Text(
                            text = when {
                                appsLoading -> "Loading apps…"
                                appsErrorMessage != null -> appsErrorMessage
                                query.isNotBlank() -> "No apps match “${query.trim()}”"
                                else -> "No launchable apps found"
                            },
                            style = MaterialTheme.typography.bodyMedium,
                            color = FoyerTextDim,
                            modifier = Modifier.padding(top = 14.dp),
                        )
                    }
                } else {
                    itemsIndexed(apps, key = { _, app -> app.stableKey }) { index, app ->
                        val section = launcherSection(app.name)
                        if (index == 0 || section != launcherSection(apps[index - 1].name)) {
                            AppAlphabetDivider(section)
                        }
                        AppRow(app = app, onClick = { onLaunchApp(app) })
                    }
                }
            }
            if (sectionIndices.isNotEmpty()) {
                AlphabetScrubber(
                    letters = sectionIndices.keys.toList(),
                    currentLetter = currentSectionLetter,
                    onLetterSelected = onSelectLetter,
                    modifier = Modifier
                        .align(Alignment.CenterEnd)
                        .padding(end = 2.dp),
                )
            }
        }
    }
}

@Composable
private fun MomentCard(
    moment: MomentInsight,
    onClick: () -> Unit,
) {
    Surface(
        modifier = Modifier
            .fillMaxWidth()
            .clickable(onClick = onClick),
        shape = RoundedCornerShape(14.dp),
        color = FoyerSurface,
        border = BorderStroke(1.dp, FoyerLineSubtle),
    ) {
        Text(
            text = linkedMomentText(moment),
            style = MaterialTheme.typography.bodyMedium,
            color = FoyerTextMuted,
            modifier = Modifier.padding(horizontal = 16.dp, vertical = 14.dp),
        )
    }
}

private fun linkedMomentText(moment: MomentInsight) = buildAnnotatedString {
    val linkStart = moment.message.indexOf(moment.linkedText, ignoreCase = true)
    if (linkStart < 0) {
        append(moment.message)
        return@buildAnnotatedString
    }

    append(moment.message.substring(0, linkStart))
    withStyle(
        SpanStyle(
            color = FoyerText,
            textDecoration = TextDecoration.Underline,
        ),
    ) {
        append(moment.message.substring(linkStart, linkStart + moment.linkedText.length))
    }
    append(moment.message.substring(linkStart + moment.linkedText.length))
}

@Composable
private fun OmnibarResults(
    query: String,
    state: FoyerUiState,
    notes: NotesCatalog,
    tasks: TasksCatalog,
    calendar: CalendarCatalog,
    contacts: ContactsCatalog,
    bookmarks: BookmarksCatalog,
    apps: List<LauncherApp>,
    onLaunchApp: (LauncherApp) -> Unit,
    onOpenCalendar: () -> Unit,
    onOpenEvent: (String) -> Unit,
    onOpenTask: (String) -> Unit,
    onOpenContact: (String) -> Unit,
    onOpenBookmark: (String) -> Unit,
    onOpenNote: (String) -> Unit,
    onOpenActivity: (AgentTask) -> Unit,
    onAskAgent: () -> Unit,
    onCreateReminder: () -> Unit,
    onOpenVoice: () -> Unit,
    onOpenSettings: () -> Unit,
    onOpenFullSearch: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val normalizedQuery = query.trim()
    val matchingEvents = remember(calendar, normalizedQuery) {
        if (normalizedQuery.isBlank()) {
            emptyList()
        } else {
            calendar.events.filter { event ->
                event.summary.contains(normalizedQuery, ignoreCase = true) ||
                    event.description.contains(normalizedQuery, ignoreCase = true) ||
                    event.location.contains(normalizedQuery, ignoreCase = true)
            }
        }
    }
    val matchingTodos = remember(tasks, normalizedQuery) {
        if (normalizedQuery.isBlank()) {
            emptyList()
        } else {
            tasks.openTasks().filter { item ->
                item.title.contains(normalizedQuery, ignoreCase = true) ||
                    item.description.contains(normalizedQuery, ignoreCase = true)
            }
        }
    }
    val matchingContacts = remember(contacts, normalizedQuery) {
        if (normalizedQuery.isBlank()) emptyList() else contacts.search(normalizedQuery)
    }
    val matchingBookmarks = remember(bookmarks, normalizedQuery) {
        if (normalizedQuery.isBlank()) emptyList() else bookmarks.visibleBookmarks(normalizedQuery)
    }
    val matchingNote = remember(notes, normalizedQuery) {
        if (normalizedQuery.isBlank()) null else notes.notes.firstOrNull { note ->
            note.title.contains(normalizedQuery, ignoreCase = true) ||
                note.summary.contains(normalizedQuery, ignoreCase = true) ||
                note.body.contains(normalizedQuery, ignoreCase = true) ||
                notes.folder(note.folderId)?.name?.contains(normalizedQuery, ignoreCase = true) == true
        }
    }
    val fallbackNote = matchingNote ?: notes.recentNotes().firstOrNull()
    val matchingActivity = remember(state.tasks, normalizedQuery) {
        if (normalizedQuery.isBlank()) null else state.tasks.firstOrNull { activity ->
            activity.title.contains(normalizedQuery, ignoreCase = true) ||
                activity.subtitle.contains(normalizedQuery, ignoreCase = true) ||
                activity.result?.contains(normalizedQuery, ignoreCase = true) == true
        }
    }

    Surface(
        modifier = modifier
            .fillMaxWidth()
            .heightIn(max = 350.dp),
        shape = RoundedCornerShape(18.dp),
        color = FoyerSurface,
        border = BorderStroke(1.dp, FoyerLine),
        shadowElevation = 12.dp,
    ) {
        Column(
            modifier = Modifier
                .verticalScroll(rememberScrollState())
                .padding(horizontal = 14.dp, vertical = 12.dp),
        ) {
            if (normalizedQuery.isBlank()) {
                OmnibarSectionLabel("Quick actions")
                OmnibarResultRow(
                    title = "Voice capture",
                    subtitle = "Record a note",
                    onClick = onOpenVoice,
                )
                OmnibarResultRow(
                    title = "New reminder",
                    subtitle = "Add to Tasks",
                    onClick = onCreateReminder,
                )
                OmnibarResultRow(
                    title = "Foyer settings",
                    subtitle = "Connections and preferences",
                    onClick = onOpenSettings,
                )

                calendar.events.firstOrNull()?.let { nextItem ->
                    OmnibarSectionLabel("Up next", modifier = Modifier.padding(top = 8.dp))
                    OmnibarResultRow(
                        title = nextItem.summary,
                        subtitle = listOfNotNull(
                            calendar.calendar(nextItem.calendarId)?.displayName,
                            nextItem.location.takeIf { it.isNotBlank() },
                        ).joinToString(" · "),
                        onClick = { onOpenEvent(nextItem.id) },
                    )
                }
            } else {
                OmnibarResultRow(
                    title = "View all results for “$normalizedQuery”",
                    subtitle = "Search apps, notes, tasks, calendar, contacts, and bookmarks",
                    onClick = onOpenFullSearch,
                )
                val visibleApps = apps.take(4)
                if (visibleApps.isNotEmpty()) {
                    OmnibarSectionLabel("Apps")
                    visibleApps.forEach { app ->
                        OmnibarResultRow(
                            title = app.name,
                            subtitle = "Open app",
                            onClick = { onLaunchApp(app) },
                        )
                    }
                }

                if (matchingEvents.isNotEmpty() || matchingTodos.isNotEmpty()) {
                    OmnibarSectionLabel(
                        text = "Calendar & tasks",
                        modifier = Modifier.padding(top = if (visibleApps.isEmpty()) 0.dp else 8.dp),
                    )
                    matchingEvents.take(3).forEach { item ->
                        OmnibarResultRow(
                            title = item.summary,
                            subtitle = calendar.calendar(item.calendarId)?.displayName ?: "Calendar",
                            onClick = { onOpenEvent(item.id) },
                        )
                    }
                    matchingTodos.take(3).forEach { item ->
                        OmnibarResultRow(
                            title = item.title,
                            subtitle = tasks.list(item.listId)?.name ?: "Task",
                            onClick = { onOpenTask(item.id) },
                        )
                    }
                }
                if (matchingContacts.isNotEmpty()) {
                    OmnibarSectionLabel("Contacts", modifier = Modifier.padding(top = 8.dp))
                    matchingContacts.take(3).forEach { contact ->
                        OmnibarResultRow(
                            title = contact.displayName,
                            subtitle = contact.subtitle().ifBlank { "Contact" },
                            onClick = { onOpenContact(contact.id) },
                        )
                    }
                }
                if (matchingBookmarks.isNotEmpty()) {
                    OmnibarSectionLabel("Bookmarks", modifier = Modifier.padding(top = 8.dp))
                    matchingBookmarks.take(3).forEach { bookmark ->
                        OmnibarResultRow(
                            title = bookmark.title,
                            subtitle = bookmark.host,
                            onClick = { onOpenBookmark(bookmark.id) },
                        )
                    }
                }

                fallbackNote?.let { note ->
                    OmnibarSectionLabel("Notes", modifier = Modifier.padding(top = 8.dp))
                    OmnibarResultRow(
                        title = if (matchingNote != null) note.title else "Search notes for “$normalizedQuery”",
                        subtitle = notes.folder(note.folderId)?.name ?: "Notes vault",
                        onClick = { onOpenNote(note.id) },
                    )
                }

                matchingActivity?.let { activity ->
                    OmnibarSectionLabel("Activity", modifier = Modifier.padding(top = 8.dp))
                    OmnibarResultRow(
                        title = activity.title,
                        subtitle = activity.subtitle,
                        onClick = { onOpenActivity(activity) },
                    )
                }

                OmnibarSectionLabel("Actions", modifier = Modifier.padding(top = 8.dp))
                OmnibarResultRow(
                    title = "Ask agent about “$normalizedQuery”",
                    subtitle = "Start an agent task",
                    onClick = onAskAgent,
                )
                OmnibarResultRow(
                    title = "Create reminder “$normalizedQuery”",
                    subtitle = "Add to Tasks",
                    onClick = onCreateReminder,
                )
                if (
                    "settings".contains(normalizedQuery, ignoreCase = true) ||
                    "foyer settings".contains(normalizedQuery, ignoreCase = true)
                ) {
                    OmnibarResultRow(
                        title = "Foyer settings",
                        subtitle = "Connections and preferences",
                        onClick = onOpenSettings,
                    )
                }
            }
        }
    }
}

@Composable
private fun OmnibarSectionLabel(
    text: String,
    modifier: Modifier = Modifier,
) {
    Text(
        text = text.uppercase(),
        style = MaterialTheme.typography.labelSmall,
        color = FoyerTextDim,
        modifier = modifier.padding(start = 2.dp, end = 2.dp, bottom = 3.dp),
    )
}

@Composable
private fun OmnibarResultRow(
    title: String,
    subtitle: String,
    onClick: () -> Unit,
) {
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(10.dp))
            .clickable(onClick = onClick)
            .padding(horizontal = 8.dp, vertical = 8.dp),
    ) {
        Text(
            text = title,
            style = MaterialTheme.typography.bodyMedium,
            color = FoyerText,
            maxLines = 1,
        )
        Text(
            text = subtitle,
            style = MaterialTheme.typography.bodySmall,
            color = FoyerTextDim,
            maxLines = 1,
        )
    }
}

@Composable
private fun AppRow(
    app: LauncherApp,
    onClick: () -> Unit,
) {
    Box(
        modifier = Modifier
            .fillMaxWidth()
            .height(43.dp)
            .clickable(onClick = onClick),
        contentAlignment = Alignment.CenterStart,
    ) {
        Text(
            text = app.name,
            style = MaterialTheme.typography.titleMedium,
            color = if (app.emphasized) FoyerText else FoyerText.copy(alpha = 0.86f),
            fontWeight = if (app.emphasized) FontWeight.SemiBold else FontWeight.Normal,
        )
    }
}

@Composable
private fun AppAlphabetDivider(letter: Char) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .height(30.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(
            text = letter.toString(),
            style = MaterialTheme.typography.labelSmall,
            color = FoyerTextMuted,
            fontWeight = FontWeight.SemiBold,
        )
        Spacer(Modifier.width(9.dp))
        Box(
            modifier = Modifier
                .weight(1f)
                .height(1.dp)
                .background(FoyerLineSubtle),
        )
    }
}

@Composable
private fun AlphabetScrubber(
    letters: List<Char>,
    currentLetter: Char?,
    onLetterSelected: (Char) -> Unit,
    modifier: Modifier = Modifier,
) {
    val hapticFeedback = LocalHapticFeedback.current
    var draggedIndex by remember(letters) { mutableStateOf<Int?>(null) }
    val isScrubbing = draggedIndex != null
    val activeIndex = draggedIndex ?: letters.indexOf(currentLetter).takeIf { it >= 0 }
    val indicatorOffset by animateDpAsState(
        targetValue = if (isScrubbing) (-36).dp else 0.dp,
        animationSpec = spring(
            dampingRatio = Spring.DampingRatioNoBouncy,
            stiffness = Spring.StiffnessMedium,
        ),
        label = "alphabet indicator offset",
    )

    Column(
        modifier = modifier
            .fillMaxHeight()
            .width(34.dp)
            .padding(vertical = 6.dp)
            .pointerInput(letters, hapticFeedback) {
                var lastIndex = -1

                fun selectLetterAt(y: Float) {
                    if (letters.isEmpty() || size.height == 0) return
                    val index = ((y / size.height) * letters.size)
                        .toInt()
                        .coerceIn(0, letters.lastIndex)
                    if (index != lastIndex) {
                        lastIndex = index
                        draggedIndex = index
                        hapticFeedback.performHapticFeedback(HapticFeedbackType.SegmentFrequentTick)
                        onLetterSelected(letters[index])
                    }
                }

                detectVerticalDragGestures(
                    onDragStart = { position -> selectLetterAt(position.y) },
                    onDragEnd = {
                        lastIndex = -1
                        draggedIndex = null
                    },
                    onDragCancel = {
                        lastIndex = -1
                        draggedIndex = null
                    },
                    onVerticalDrag = { change, _ ->
                        change.consume()
                        selectLetterAt(change.position.y)
                    },
                )
            },
        verticalArrangement = Arrangement.SpaceEvenly,
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        letters.forEachIndexed { index, letter ->
            val isActive = index == activeIndex
            val letterColor by animateColorAsState(
                targetValue = when {
                    isActive && !isScrubbing -> Color.Transparent
                    isActive -> FoyerText
                    else -> FoyerTextDim
                },
                label = "alphabet letter color",
            )
            val highlightColor by animateColorAsState(
                targetValue = if (isActive) FoyerText else Color.Transparent,
                label = "alphabet letter highlight",
            )

            Box(
                modifier = Modifier
                    .weight(1f)
                    .fillMaxWidth()
                    .clickable {
                        hapticFeedback.performHapticFeedback(HapticFeedbackType.SegmentTick)
                        onLetterSelected(letter)
                    },
                contentAlignment = Alignment.Center,
            ) {
                Box(
                    modifier = Modifier
                        .size(22.dp)
                        .offset(x = indicatorOffset)
                        .background(highlightColor, CircleShape),
                    contentAlignment = Alignment.Center,
                ) {
                    Text(
                        text = letter.toString(),
                        style = MaterialTheme.typography.labelSmall,
                        color = FoyerBlack,
                        fontWeight = FontWeight.Bold,
                    )
                }
                Text(
                    text = letter.toString(),
                    style = MaterialTheme.typography.labelSmall,
                    color = letterColor,
                    fontWeight = if (isActive) FontWeight.Bold else FontWeight.Medium,
                )
            }
        }
    }
}

@Composable
private fun UniversalInputBar(
    query: String,
    onQueryChange: (String) -> Unit,
    onFocusChanged: (Boolean) -> Unit,
    onSubmit: () -> Unit,
    onOpenVoice: () -> Unit,
    modifier: Modifier = Modifier,
) {
    Row(
        modifier = modifier
            .fillMaxWidth()
            .height(50.dp)
            .clip(RoundedCornerShape(25.dp))
            .border(1.dp, FoyerLine, RoundedCornerShape(25.dp))
            .background(FoyerBlack)
            .padding(horizontal = 15.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        SearchGlyph()
        Spacer(Modifier.width(12.dp))
        BasicTextField(
            value = query,
            onValueChange = onQueryChange,
            textStyle = MaterialTheme.typography.bodyMedium.copy(color = FoyerText),
            singleLine = true,
            keyboardOptions = KeyboardOptions(imeAction = ImeAction.Done),
            keyboardActions = KeyboardActions(onDone = { onSubmit() }),
            cursorBrush = SolidColor(FoyerText),
            modifier = Modifier
                .weight(1f)
                .onFocusChanged { state -> onFocusChanged(state.isFocused) },
            decorationBox = { innerTextField ->
                Box(contentAlignment = Alignment.CenterStart) {
                    if (query.isEmpty()) {
                        Text(
                            text = "Ask, remind, or search",
                            style = MaterialTheme.typography.bodyMedium,
                            color = FoyerTextDim,
                        )
                    }
                    innerTextField()
                }
            },
        )
        Box(
            modifier = Modifier
                .size(36.dp)
                .clip(CircleShape)
                .clickable(onClick = onOpenVoice),
            contentAlignment = Alignment.Center,
        ) {
            MicrophoneGlyph()
        }
    }
}
