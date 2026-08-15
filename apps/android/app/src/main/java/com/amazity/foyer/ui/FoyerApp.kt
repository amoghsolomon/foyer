package com.amazity.foyer.ui

import android.Manifest
import android.app.role.RoleManager
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.widget.Toast
import androidx.activity.BackEventCompat
import androidx.activity.compose.BackHandler
import androidx.activity.compose.PredictiveBackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.tooling.preview.Preview
import androidx.core.content.ContextCompat
import com.amazity.foyer.BuildConfig
import com.amazity.foyer.assistant.AppCommand
import com.amazity.foyer.assistant.AppCommandBus
import com.amazity.foyer.assistant.AssistantOverlayHost
import com.amazity.foyer.assistant.AssistantSessionController
import com.amazity.foyer.assistant.AssistantUiState
import com.amazity.foyer.auth.FoyerAccountCoordinator
import com.amazity.foyer.auth.GrokAccountCoordinator
import com.amazity.foyer.auth.GrokDevicePoll
import com.amazity.foyer.launcher.LauncherAppsRepository
import com.amazity.foyer.launcher.rememberInstalledApps
import com.amazity.foyer.data.FoyerRepository
import com.amazity.foyer.data.SyncStatusSnapshot
import com.amazity.foyer.data.agentTask
import com.amazity.foyer.data.chatMessage
import com.amazity.foyer.foyerApplication
import com.amazity.foyer.contacts.ContactDraft
import com.amazity.foyer.model.AddressBook
import com.amazity.foyer.model.BookmarkFolder
import com.amazity.foyer.model.BookmarkItem
import com.amazity.foyer.model.BookmarksCatalog
import com.amazity.foyer.model.CalendarCatalog
import com.amazity.foyer.model.Contact
import com.amazity.foyer.model.ContactsCatalog
import com.amazity.foyer.model.EventDraft
import com.amazity.foyer.model.FoyerCalendar
import com.amazity.foyer.model.FoyerDestination
import com.amazity.foyer.model.FoyerEvent
import com.amazity.foyer.model.FoyerUiState
import com.amazity.foyer.model.HomePanel
import com.amazity.foyer.model.LauncherApp
import com.amazity.foyer.model.NotesCatalog
import com.amazity.foyer.model.TaskDue
import com.amazity.foyer.model.TasksCatalog
import com.amazity.foyer.model.VaultNote
import com.amazity.foyer.model.VaultTask
import com.amazity.foyer.model.VaultTaskList
import com.amazity.foyer.ui.screens.BookmarkDetailScreen
import com.amazity.foyer.ui.screens.BookmarkEditorScreen
import com.amazity.foyer.ui.screens.BookmarkFolderScreen
import com.amazity.foyer.ui.screens.ContactDetailScreen
import com.amazity.foyer.ui.screens.ContactEditorScreen
import com.amazity.foyer.ui.screens.EventDetailScreen
import com.amazity.foyer.ui.screens.EventEditorScreen
import com.amazity.foyer.ui.screens.HomeScreen
import com.amazity.foyer.ui.screens.ActivityChatScreen
import com.amazity.foyer.ui.screens.FolderNotesScreen
import com.amazity.foyer.ui.screens.NoteDetailScreen
import com.amazity.foyer.ui.screens.NoteEditorScreen
import com.amazity.foyer.ui.screens.TaskDetailScreen
import com.amazity.foyer.ui.screens.TaskEditorScreen
import com.amazity.foyer.ui.screens.TaskListScreen
import com.amazity.foyer.ui.screens.OnboardingScreen
import com.amazity.foyer.ui.screens.MemoryProfileScreen
import com.amazity.foyer.ui.screens.NotificationContextScreen
import com.amazity.foyer.notifications.NotificationContextManager
import com.amazity.foyer.ui.screens.SearchResultsScreen
import com.amazity.foyer.ui.screens.SettingsScreen
import com.amazity.foyer.ui.screens.SignInScreen
import com.amazity.foyer.ui.screens.SyncStatusScreen
import com.amazity.foyer.ui.theme.FoyerTheme
import com.amazity.foyer.voice.MoonshineKokoroReadAloud
import com.amazity.foyer.voice.ReadAloudState
import com.amazity.foyer.voice.MoonshineVoiceInput
import java.time.ZoneId
import kotlinx.coroutines.launch
import kotlinx.coroutines.delay
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.flow.collect
import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.EnterTransition
import androidx.compose.animation.ExitTransition
import androidx.compose.animation.SizeTransform
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.FastOutSlowInEasing
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.slideInHorizontally
import androidx.compose.animation.slideOutHorizontally
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.ui.Modifier
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner

@Composable
fun FoyerApp(
    homeRequestVersion: Int = 0,
    deepLinkRequestVersion: Int = 0,
    deepLinkTargetType: String? = null,
    deepLinkTargetId: String? = null,
) {
    val context = LocalContext.current
    val repository = remember(context) { LauncherAppsRepository(context) }
    val foyerRepository = remember(context) { FoyerRepository(context) }
    val cachedActivities by foyerRepository.activities.collectAsState(initial = emptyList())
    val cachedNotes by foyerRepository.notes.collectAsState()
    val hostedTasks by foyerRepository.hostedTasks.collectAsState()
    val hostedCalendar by foyerRepository.hostedCalendar.collectAsState()
    val hostedContacts by foyerRepository.contacts.collectAsState()
    val hostedBookmarks by foyerRepository.bookmarks.collectAsState()
    val cachedHomeBriefing by foyerRepository.homeBriefing.collectAsState(initial = null)
    val syncStatus by foyerRepository.syncStatus.collectAsState(initial = SyncStatusSnapshot())
    val coroutineScope = rememberCoroutineScope()
    val accountCoordinator = remember(context) { FoyerAccountCoordinator(context) }
    val grokCoordinator = remember(context) { GrokAccountCoordinator(context) }
    var signedIn by remember { mutableStateOf(false) }
    var sessionChecked by remember { mutableStateOf(!accountCoordinator.hasPreviousAccess()) }
    var authLoading by remember { mutableStateOf(false) }
    var authError by rememberSaveable { mutableStateOf<String?>(null) }
    var grokConnectionStatus by rememberSaveable { mutableStateOf<String?>(null) }
    var grokConnectionEnabled by rememberSaveable { mutableStateOf(true) }
    var grokFlowId by rememberSaveable { mutableStateOf<String?>(null) }
    var grokPolling by remember { mutableStateOf(false) }
    var settingsTimezone by remember { mutableStateOf(foyerRepository.currentTimezone()) }
    val onboardingPreferences = remember(context) {
        context.getSharedPreferences("foyer_onboarding", android.content.Context.MODE_PRIVATE)
    }
    var onboardingComplete by remember {
        mutableStateOf(onboardingPreferences.getBoolean("completed", false))
    }

    LaunchedEffect(accountCoordinator) {
        if (accountCoordinator.hasPreviousAccess()) {
            signedIn = accountCoordinator.restoreSession()
            if (!signedIn) foyerRepository.clearSessionData()
        }
        sessionChecked = true
    }

    LaunchedEffect(foyerRepository, signedIn) {
        if (!signedIn) return@LaunchedEffect
        foyerRepository.startPersonalData()
        runCatching { foyerRepository.refreshHostedPersonalData() }
        runCatching { foyerRepository.refreshActivities() }
        runCatching { foyerRepository.refreshHomeBriefing() }
        runCatching { foyerRepository.syncPendingTimezone() }
        runCatching { foyerRepository.refreshTimezone() }
            .onSuccess { settingsTimezone = it }
    }

    LaunchedEffect(foyerRepository, homeRequestVersion) {
        if (homeRequestVersion > 0) {
            runCatching { foyerRepository.refreshHomeBriefing() }
        }
    }

    LaunchedEffect(signedIn) {
        if (!signedIn) return@LaunchedEffect
        runCatching { grokCoordinator.status() }
            .onSuccess {
                grokConnectionEnabled = it.manageable
                grokConnectionStatus = when {
                    it.configured -> "Connected through pi-grok-cli"
                    !it.manageable -> "Only the configured model-auth administrator can connect"
                    else -> null
                }
            }
    }
    val installedApps = rememberInstalledApps(context, repository)
    val notificationContextManager = remember(context) { NotificationContextManager(context) }
    var notificationContextEnabled by remember {
        mutableStateOf(notificationContextManager.enabled())
    }
    var notificationAccessGranted by remember {
        mutableStateOf(notificationContextManager.accessGranted())
    }
    var notificationWhitelist by remember {
        mutableStateOf(notificationContextManager.whitelist())
    }
    val lifecycleOwner = LocalLifecycleOwner.current
    DisposableEffect(lifecycleOwner, notificationContextManager) {
        val observer = LifecycleEventObserver { _, event ->
            if (event == Lifecycle.Event.ON_RESUME) {
                notificationAccessGranted = notificationContextManager.accessGranted()
            }
        }
        lifecycleOwner.lifecycle.addObserver(observer)
        onDispose { lifecycleOwner.lifecycle.removeObserver(observer) }
    }
    LaunchedEffect(notificationContextManager, installedApps.apps) {
        notificationWhitelist = notificationContextManager.initialize(
            installedApps.apps.mapTo(mutableSetOf(), LauncherApp::packageName),
        )
        runCatching { foyerRepository.syncPendingTimezone() }
    }
    val state = FoyerUiState(
        dailyMessage = cachedHomeBriefing?.dailyMessage.orEmpty(),
        moment = cachedHomeBriefing?.let { briefing ->
            val target = when (briefing.targetType) {
                "activity" -> com.amazity.foyer.model.MomentTarget.Activity
                "calendar" -> com.amazity.foyer.model.MomentTarget.Calendar
                "task" -> com.amazity.foyer.model.MomentTarget.Task
                else -> null
            }
            if (
                target != null && briefing.insightMessage != null &&
                briefing.targetId != null && briefing.targetLabel != null
            ) {
                com.amazity.foyer.model.MomentInsight(
                    message = briefing.insightMessage,
                    linkedText = briefing.targetLabel,
                    target = target,
                    targetId = briefing.targetId,
                )
            } else {
                null
            }
        },
        apps = installedApps.apps,
        agendaItems = emptyList(),
        todoItems = emptyList(),
        tasks = cachedActivities.map(::agentTask),
    )

    if (!sessionChecked || !signedIn) {
        val enrollment = remember { accountCoordinator.enrollment() }
        SignInScreen(
            loading = authLoading || !sessionChecked,
            errorMessage = authError,
            enrollment = enrollment,
            developmentAuthAvailable = accountCoordinator.developmentAuthAvailable(),
            onRetryEnrollment = {
                coroutineScope.launch {
                    authLoading = true
                    authError = null
                    runCatching { accountCoordinator.retryEnrollment() }
                        .onSuccess { accepted ->
                            if (accepted) {
                                signedIn = true
                                com.amazity.foyer.sync.SyncScheduler.requestNow(context)
                            } else {
                                authError = "This device is not enrolled yet. Ask the operator to add the public key, then try again."
                            }
                        }
                        .onFailure { authError = com.amazity.foyer.auth.deviceAuthErrorMessage(it) }
                    authLoading = false
                }
            },
            onCopyEnrollment = {
                accountCoordinator.copyEnrollment()
                Toast.makeText(context, "Copied public enrollment", Toast.LENGTH_SHORT).show()
            },
            onShareEnrollment = {
                context.startActivity(
                    Intent.createChooser(
                        accountCoordinator.shareEnrollmentIntent(),
                        "Share public enrollment",
                    ),
                )
            },
            onUseDevelopmentSession = {
                coroutineScope.launch {
                    authLoading = true
                    authError = null
                    runCatching { accountCoordinator.useDevelopmentSession() }
                        .onSuccess { signedIn = true }
                        .onFailure { authError = com.amazity.foyer.auth.deviceAuthErrorMessage(it) }
                    authLoading = false
                }
            },
        )
        return
    }

    FoyerAppContent(
        state = state,
        notes = cachedNotes,
        tasks = hostedTasks,
        calendar = hostedCalendar,
        contacts = hostedContacts,
        bookmarks = hostedBookmarks,
        appsLoading = installedApps.loading,
        appsErrorMessage = installedApps.errorMessage,
        homeRequestVersion = homeRequestVersion,
        deepLinkRequestVersion = deepLinkRequestVersion,
        deepLinkTargetType = deepLinkTargetType,
        deepLinkTargetId = deepLinkTargetId,
        syncStatus = syncStatus,
        onSyncNow = { com.amazity.foyer.sync.SyncScheduler.requestNow(context) },
        showOnboarding = !onboardingComplete,
        onCompleteOnboarding = {
            onboardingComplete = true
            onboardingPreferences.edit().putBoolean("completed", true).apply()
        },
        timezone = settingsTimezone,
        onTimezoneChange = { timezone ->
            settingsTimezone = timezone
            foyerRepository.queueTimezone(timezone)
            coroutineScope.launch { runCatching { foyerRepository.syncPendingTimezone() } }
        },
        onLaunchApp = { app ->
            repository.launch(app).onFailure {
                Toast.makeText(
                    context,
                    "Couldn't open ${app.name}",
                    Toast.LENGTH_SHORT,
                ).show()
            }
        },
        onAskAgent = { message ->
            coroutineScope.launch { foyerRepository.askAgent(message) }
        },
        onCreateReminder = { text ->
            coroutineScope.launch { foyerRepository.createTask(text) }
        },
        activityMessages = foyerRepository::activityMessages,
        onRefreshActivity = foyerRepository::refreshActivity,
        onSendActivityMessage = foyerRepository::sendActivityMessage,
        onScheduleActivity = foyerRepository::scheduleActivity,
        onCancelActivitySchedule = foyerRepository::cancelActivitySchedule,
        onRenameActivity = foyerRepository::renameActivity,
        onDeleteActivity = foyerRepository::deleteActivity,
        onRunActivityNow = foyerRepository::runActivityNow,
        onRetryActivityRun = foyerRepository::retryActivityRun,
        onCreateNote = { title, body, folderId, tags ->
            foyerRepository.createNote(title, body, folderId, tags)
        },
        onUpdateNote = { note, title, body, folderId ->
            foyerRepository.updateNote(note, title, body, folderId)
        },
        onDeleteNote = foyerRepository::deleteNote,
        onCreateFolder = foyerRepository::createFolder,
        onRenameFolder = foyerRepository::renameFolder,
        onMoveFolder = foyerRepository::moveFolder,
        onDeleteFolder = foyerRepository::deleteFolder,
        onRefreshNotes = foyerRepository::refreshNotes,
        onCreateTaskList = foyerRepository::createTaskList,
        onRenameTaskList = foyerRepository::renameTaskList,
        onDeleteTaskList = foyerRepository::deleteTaskList,
        onCreateTask = { title, description, listId, due, priority ->
            foyerRepository.createHostedTask(title, description, listId, due, priority)
        },
        onUpdateTask = foyerRepository::updateHostedTask,
        onCompleteTask = foyerRepository::completeHostedTask,
        onReopenTask = foyerRepository::reopenHostedTask,
        onDeleteTask = foyerRepository::deleteHostedTask,
        onSelectCalendar = foyerRepository::selectCalendar,
        onCreateCalendar = foyerRepository::createCalendar,
        onCreateEvent = foyerRepository::createEvent,
        onUpdateEvent = foyerRepository::updateEvent,
        onDeleteEvent = foyerRepository::deleteEvent,
        onCreateAddressBook = foyerRepository::createAddressBook,
        onCreateContact = foyerRepository::createContact,
        onUpdateContact = foyerRepository::updateContact,
        onDeleteContact = foyerRepository::deleteContact,
        onCreateBookmarkFolder = foyerRepository::createBookmarkFolder,
        onRenameBookmarkFolder = foyerRepository::renameBookmarkFolder,
        onMoveBookmarkFolder = foyerRepository::moveBookmarkFolder,
        onDeleteBookmarkFolder = foyerRepository::deleteBookmarkFolder,
        onCreateBookmark = foyerRepository::createBookmark,
        onUpdateBookmark = foyerRepository::updateBookmark,
        onSetBookmarkFavorite = foyerRepository::setBookmarkFavorite,
        onSetBookmarkArchived = foyerRepository::setBookmarkArchived,
        onDeleteBookmark = foyerRepository::deleteBookmark,
        onLoadProfile = foyerRepository::profile,
        onLoadMemories = foyerRepository::memories,
        onDeleteMemory = foyerRepository::deleteMemory,
        notificationContextEnabled = notificationContextEnabled,
        notificationAccessGranted = notificationAccessGranted,
        notificationWhitelist = notificationWhitelist,
        installedApps = installedApps.apps,
        onNotificationContextToggle = { enabled ->
            notificationContextManager.setEnabled(enabled)
            notificationContextEnabled = enabled
            if (enabled && !notificationContextManager.accessGranted()) {
                notificationContextManager.openAccessSettings()
            }
            notificationAccessGranted = notificationContextManager.accessGranted()
        },
        onNotificationWhitelistChange = { packages ->
            notificationWhitelist = packages
            foyerRepository.queueNotificationWhitelist(packages)
            coroutineScope.launch { runCatching { foyerRepository.syncPendingTimezone() } }
        },
        signedIn = signedIn,
        grokConnectionStatus = grokConnectionStatus,
        grokConnectionEnabled = grokConnectionEnabled,
        assistantController = context.foyerApplication.assistantController,
        appCommands = context.foyerApplication.appCommands,
        onConnectGrok = {
            if (!grokPolling) coroutineScope.launch {
                grokPolling = true
                runCatching {
                    var flowId = grokFlowId
                    var waitSeconds = 1L
                    if (flowId == null) {
                        val login = grokCoordinator.startDeviceLogin()
                        flowId = login.flowId
                        grokFlowId = login.flowId
                        waitSeconds = login.intervalSeconds
                        grokConnectionStatus = "Enter ${login.userCode} in the browser; authorization is pending"
                        context.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(login.verificationUri)))
                    }

                    while (true) {
                        delay(waitSeconds * 1_000)
                        when (val result = grokCoordinator.pollDeviceLogin(requireNotNull(flowId))) {
                            GrokDevicePoll.Connected -> {
                                grokFlowId = null
                                grokConnectionStatus = "Connected through pi-grok-cli"
                                break
                            }
                            is GrokDevicePoll.Pending -> {
                                waitSeconds = result.retryAfterSeconds
                                grokConnectionStatus = "Waiting for SuperGrok authorization"
                            }
                        }
                    }
                }.onFailure {
                    grokFlowId = null
                    grokConnectionStatus = it.message ?: "SuperGrok connection failed"
                }
                grokPolling = false
            }
        },
        onSignOut = {
            coroutineScope.launch {
                runCatching { accountCoordinator.signOut() }
                foyerRepository.clearSessionData()
                signedIn = false
            }
        },
    )
}

@Composable
private fun FoyerAppContent(
    state: FoyerUiState,
    notes: NotesCatalog,
    tasks: TasksCatalog,
    calendar: CalendarCatalog,
    contacts: ContactsCatalog,
    bookmarks: BookmarksCatalog,
    appsLoading: Boolean,
    appsErrorMessage: String?,
    homeRequestVersion: Int,
    deepLinkRequestVersion: Int,
    deepLinkTargetType: String?,
    deepLinkTargetId: String?,
    syncStatus: SyncStatusSnapshot,
    onSyncNow: () -> Unit,
    showOnboarding: Boolean,
    onCompleteOnboarding: () -> Unit,
    timezone: String,
    onTimezoneChange: (String) -> Unit,
    onLaunchApp: (LauncherApp) -> Unit,
    onAskAgent: (String) -> Unit,
    onCreateReminder: (String) -> Unit,
    activityMessages: (String) -> kotlinx.coroutines.flow.Flow<List<com.amazity.foyer.data.CachedActivityMessage>>,
    onRefreshActivity: suspend (String) -> com.amazity.foyer.data.CachedActivity,
    onSendActivityMessage: suspend (String, String) -> Unit,
    onScheduleActivity: suspend (String, String, String, Int, String) -> Unit,
    onCancelActivitySchedule: suspend (String) -> Unit,
    onRenameActivity: suspend (String, String) -> Unit,
    onDeleteActivity: suspend (String) -> Unit,
    onRunActivityNow: suspend (String) -> Unit,
    onRetryActivityRun: suspend (String, String) -> Unit,
    onCreateNote: suspend (String, String, String, List<String>) -> VaultNote,
    onUpdateNote: suspend (VaultNote, String, String, String) -> VaultNote,
    onDeleteNote: suspend (VaultNote) -> Unit,
    onCreateFolder: suspend (String, String?) -> com.amazity.foyer.model.VaultFolder = { _, _ -> error("Folders are unavailable") },
    onRenameFolder: suspend (com.amazity.foyer.model.VaultFolder, String) -> com.amazity.foyer.model.VaultFolder = { _, _ -> error("Folders are unavailable") },
    onMoveFolder: suspend (com.amazity.foyer.model.VaultFolder, String?) -> com.amazity.foyer.model.VaultFolder = { _, _ -> error("Folders are unavailable") },
    onDeleteFolder: suspend (com.amazity.foyer.model.VaultFolder) -> Unit = {},
    onRefreshNotes: suspend () -> Unit = {},
    onCreateTaskList: suspend (String) -> VaultTaskList = { error("Tasks are unavailable") },
    onRenameTaskList: suspend (VaultTaskList, String) -> VaultTaskList = { _, _ -> error("Tasks are unavailable") },
    onDeleteTaskList: suspend (VaultTaskList) -> Unit = {},
    onCreateTask: suspend (String, String, String, TaskDue?, Int) -> VaultTask = { _, _, _, _, _ -> error("Tasks are unavailable") },
    onUpdateTask: suspend (VaultTask, String, String, TaskDue?, Int, String) -> VaultTask = { _, _, _, _, _, _ -> error("Tasks are unavailable") },
    onCompleteTask: suspend (VaultTask) -> VaultTask = { error("Tasks are unavailable") },
    onReopenTask: suspend (VaultTask) -> VaultTask = { error("Tasks are unavailable") },
    onDeleteTask: suspend (VaultTask) -> Unit = {},
    onSelectCalendar: (String?) -> Unit = {},
    onCreateCalendar: suspend (String, String) -> FoyerCalendar = { _, _ -> error("Calendar is unavailable") },
    onCreateEvent: suspend (EventDraft) -> FoyerEvent = { error("Calendar is unavailable") },
    onUpdateEvent: suspend (FoyerEvent, EventDraft) -> FoyerEvent = { _, _ -> error("Calendar is unavailable") },
    onDeleteEvent: suspend (FoyerEvent) -> Unit = {},
    onCreateAddressBook: suspend (String) -> AddressBook = { error("Contacts are unavailable") },
    onCreateContact: suspend (ContactDraft) -> Contact = { error("Contacts are unavailable") },
    onUpdateContact: suspend (Contact, ContactDraft) -> Contact = { _, _ -> error("Contacts are unavailable") },
    onDeleteContact: suspend (Contact) -> Unit = {},
    onCreateBookmarkFolder: suspend (String, String?) -> BookmarkFolder = { _, _ -> error("Bookmarks are unavailable") },
    onRenameBookmarkFolder: suspend (BookmarkFolder, String) -> BookmarkFolder = { _, _ -> error("Bookmarks are unavailable") },
    onMoveBookmarkFolder: suspend (BookmarkFolder, String?) -> BookmarkFolder = { _, _ -> error("Bookmarks are unavailable") },
    onDeleteBookmarkFolder: suspend (BookmarkFolder) -> Unit = {},
    onCreateBookmark: suspend (String, String, String, String, List<String>, Boolean) -> BookmarkItem = { _, _, _, _, _, _ -> error("Bookmarks are unavailable") },
    onUpdateBookmark: suspend (BookmarkItem, String, String, String, List<String>, String) -> BookmarkItem = { _, _, _, _, _, _ -> error("Bookmarks are unavailable") },
    onSetBookmarkFavorite: suspend (BookmarkItem, Boolean) -> BookmarkItem = { _, _ -> error("Bookmarks are unavailable") },
    onSetBookmarkArchived: suspend (BookmarkItem, Boolean) -> BookmarkItem = { _, _ -> error("Bookmarks are unavailable") },
    onDeleteBookmark: suspend (BookmarkItem) -> Unit = {},
    onLoadProfile: suspend () -> com.amazity.foyer.model.ConsolidatedProfile?,
    onLoadMemories: suspend (String?) -> com.amazity.foyer.model.MemoryPage,
    onDeleteMemory: suspend (com.amazity.foyer.model.MemoryRecord) -> Unit,
    notificationContextEnabled: Boolean,
    notificationAccessGranted: Boolean,
    notificationWhitelist: Set<String>,
    installedApps: List<LauncherApp>,
    onNotificationContextToggle: (Boolean) -> Unit,
    onNotificationWhitelistChange: (Set<String>) -> Unit,
    signedIn: Boolean,
    grokConnectionStatus: String?,
    grokConnectionEnabled: Boolean,
    onConnectGrok: () -> Unit,
    onSignOut: () -> Unit,
    assistantController: AssistantSessionController? = null,
    appCommands: AppCommandBus? = null,
) {
    val context = LocalContext.current
    val assistantState = assistantController?.state?.collectAsState()?.value ?: AssistantUiState()
    val roleManager = remember(context) { context.getSystemService(RoleManager::class.java) }
    var assistantRoleHeld by remember {
        mutableStateOf(roleManager?.isRoleHeld(RoleManager.ROLE_ASSISTANT) == true)
    }
    val assistantRoleLauncher = rememberLauncherForActivityResult(
        ActivityResultContracts.StartActivityForResult(),
    ) {
        assistantRoleHeld = roleManager?.isRoleHeld(RoleManager.ROLE_ASSISTANT) == true
    }
    val microphonePermissionLauncher = rememberLauncherForActivityResult(
        ActivityResultContracts.RequestPermission(),
    ) { granted ->
        if (granted) assistantController?.startListening()
        else assistantController?.microphonePermissionDenied()
    }
    val openAssistant: () -> Unit = {
        assistantController?.show()
        if (
            ContextCompat.checkSelfPermission(context, Manifest.permission.RECORD_AUDIO) ==
            PackageManager.PERMISSION_GRANTED
        ) {
            assistantController?.startListening()
        } else {
            microphonePermissionLauncher.launch(Manifest.permission.RECORD_AUDIO)
        }
        Unit
    }
    val noteVoiceInput = remember(context) { MoonshineVoiceInput(context.applicationContext) }
    val noteReadAloud = remember(context) { MoonshineKokoroReadAloud(context.applicationContext) }
    val messageReadAloudState by noteReadAloud.state.collectAsState()
    var activeReadAloudMessageId by remember { mutableStateOf<String?>(null) }
    DisposableEffect(noteVoiceInput) {
        onDispose { noteVoiceInput.close() }
    }
    DisposableEffect(noteReadAloud) {
        onDispose { noteReadAloud.close() }
    }
    LaunchedEffect(messageReadAloudState) {
        if (messageReadAloudState is ReadAloudState.Idle) activeReadAloudMessageId = null
    }
    LaunchedEffect(assistantState.visible) {
        if (!assistantState.visible && activeReadAloudMessageId?.startsWith("assistant:") == true) {
            noteReadAloud.stop()
            activeReadAloudMessageId = null
        }
    }
    val toggleMessageReadAloud: (String, String) -> Unit = { id, text ->
        val active = messageReadAloudState is ReadAloudState.Preparing ||
            messageReadAloudState is ReadAloudState.Speaking
        if (active && activeReadAloudMessageId == id) {
            noteReadAloud.stop()
            activeReadAloudMessageId = null
        } else {
            noteReadAloud.stop()
            activeReadAloudMessageId = id
            noteReadAloud.read(text)
        }
    }
    var destination by rememberSaveable {
        mutableStateOf(if (showOnboarding) FoyerDestination.Onboarding else FoyerDestination.Home)
    }
    var selectedPanel by rememberSaveable { mutableStateOf(HomePanel.Apps) }
    var selectedFolderId by rememberSaveable { mutableStateOf<String?>(null) }
    var selectedNoteId by rememberSaveable { mutableStateOf<String?>(null) }
    var selectedActivityId by rememberSaveable { mutableStateOf<String?>(null) }
    var selectedTaskListId by rememberSaveable { mutableStateOf<String?>(null) }
    var selectedTaskId by rememberSaveable { mutableStateOf<String?>(null) }
    var selectedEventId by rememberSaveable { mutableStateOf<String?>(null) }
    var selectedContactId by rememberSaveable { mutableStateOf<String?>(null) }
    var selectedAddressBookId by rememberSaveable { mutableStateOf<String?>(null) }
    var contactsSearchQuery by rememberSaveable { mutableStateOf("") }
    var selectedBookmarkFolderId by rememberSaveable { mutableStateOf<String?>(null) }
    var selectedBookmarkId by rememberSaveable { mutableStateOf<String?>(null) }
    var searchQuery by rememberSaveable { mutableStateOf("") }
    var editingExistingNote by rememberSaveable { mutableStateOf(false) }
    var editingExistingTask by rememberSaveable { mutableStateOf(false) }
    var editingExistingEvent by rememberSaveable { mutableStateOf(false) }
    var editingExistingContact by rememberSaveable { mutableStateOf(false) }
    var editingExistingBookmark by rememberSaveable { mutableStateOf(false) }
    var onboardingReturnsToSettings by rememberSaveable { mutableStateOf(false) }
    var detailReturnsToSearch by rememberSaveable { mutableStateOf(false) }
    val noteCatalog = notes
    val visibleState = state
    val conversationScope = rememberCoroutineScope()
    val predictiveBackProgress = remember { Animatable(0f) }
    var predictiveBackEdge by remember { mutableIntStateOf(BackEventCompat.EDGE_LEFT) }
    var suppressDestinationAnimation by remember { mutableStateOf(false) }
    var noteWriteInFlight by remember { mutableStateOf(false) }
    var noteWriteError by remember { mutableStateOf<String?>(null) }
    val currentCatalog by rememberUpdatedState(noteCatalog)
    val currentCreateNote by rememberUpdatedState(onCreateNote)

    LaunchedEffect(appCommands) {
        appCommands?.commands?.collect { command ->
            when (command) {
                is AppCommand.CreateNote -> {
                    val folderId = currentCatalog.folders
                        .firstOrNull { it.name.equals("Inbox", ignoreCase = true) }
                        ?.id
                        ?: currentCatalog.folders.firstOrNull()?.id
                    if (folderId == null) {
                        Toast.makeText(context, "Notes are still loading from the server", Toast.LENGTH_SHORT).show()
                    } else {
                        runCatching {
                            currentCreateNote(command.title, command.body, folderId, listOf("assistant"))
                        }.onSuccess { note ->
                            selectedNoteId = note.id
                            selectedFolderId = null
                            selectedPanel = HomePanel.Notes
                            destination = FoyerDestination.NoteDetail
                        }.onFailure {
                            Toast.makeText(context, it.message ?: "Couldn't save note", Toast.LENGTH_LONG).show()
                        }
                    }
                }
            }
        }
    }

    LaunchedEffect(homeRequestVersion) {
        if (homeRequestVersion > 0) {
            destination = FoyerDestination.Home
            selectedPanel = HomePanel.Apps
            selectedFolderId = null
            selectedNoteId = null
            selectedActivityId = null
            selectedTaskListId = null
            selectedTaskId = null
            selectedEventId = null
            selectedContactId = null
            selectedBookmarkFolderId = null
            selectedBookmarkId = null
        }
    }

    LaunchedEffect(deepLinkRequestVersion, deepLinkTargetType, deepLinkTargetId) {
        val targetId = deepLinkTargetId?.takeIf(String::isNotBlank) ?: return@LaunchedEffect
        if (deepLinkRequestVersion <= 0) return@LaunchedEffect
        detailReturnsToSearch = false
        when (deepLinkTargetType) {
            "activity" -> {
                selectedActivityId = targetId
                destination = FoyerDestination.ActivityChat
            }
            "calendar" -> {
                selectedEventId = targetId
                selectedPanel = HomePanel.Calendar
                destination = if (calendar.event(targetId) != null) {
                    FoyerDestination.EventDetail
                } else {
                    FoyerDestination.Home
                }
            }
            "task" -> {
                selectedTaskId = targetId
                selectedPanel = HomePanel.Tasks
                destination = if (tasks.task(targetId) != null) {
                    FoyerDestination.TaskDetail
                } else {
                    FoyerDestination.Home
                }
            }
        }
    }

    val navigateBack: () -> Unit = {
        if (destination == FoyerDestination.ActivityChat) {
            destination = if (detailReturnsToSearch) FoyerDestination.SearchResults else FoyerDestination.Home
            if (!detailReturnsToSearch) selectedPanel = HomePanel.Activity
            selectedActivityId = null
        } else if (destination == FoyerDestination.NoteEditor && editingExistingNote) {
            destination = FoyerDestination.NoteDetail
        } else if (destination == FoyerDestination.TaskEditor && editingExistingTask) {
            destination = FoyerDestination.TaskDetail
        } else if (destination == FoyerDestination.EventEditor && editingExistingEvent) {
            destination = FoyerDestination.EventDetail
        } else if (destination == FoyerDestination.ContactEditor && editingExistingContact) {
            destination = FoyerDestination.ContactDetail
        } else if (destination == FoyerDestination.BookmarkEditor && editingExistingBookmark) {
            destination = FoyerDestination.BookmarkDetail
        } else if (destination == FoyerDestination.TaskDetail && selectedTaskListId != null) {
            selectedTaskId = null
            destination = FoyerDestination.TaskList
        } else if (destination == FoyerDestination.BookmarkDetail && selectedBookmarkFolderId != null) {
            selectedBookmarkId = null
            destination = FoyerDestination.BookmarkFolder
        } else if (destination == FoyerDestination.BookmarkFolder) {
            val parentId = selectedBookmarkFolderId?.let(bookmarks::folder)?.parentId
            if (parentId != null && bookmarks.folder(parentId) != null) {
                selectedBookmarkFolderId = parentId
            } else {
                destination = FoyerDestination.Home
                selectedBookmarkFolderId = null
                selectedPanel = HomePanel.Bookmarks
            }
        } else if (destination == FoyerDestination.SyncStatus) {
            destination = FoyerDestination.Settings
        } else if (destination == FoyerDestination.MemoryProfile) {
            destination = FoyerDestination.Settings
        } else if (destination == FoyerDestination.NotificationContext) {
            destination = FoyerDestination.Settings
        } else if (destination == FoyerDestination.NoteDetail && detailReturnsToSearch) {
            destination = FoyerDestination.SearchResults
            detailReturnsToSearch = false
        } else if (
            destination == FoyerDestination.NoteDetail &&
            selectedFolderId != null &&
            selectedNoteId != null
        ) {
            selectedNoteId = null
        } else if (destination == FoyerDestination.NoteDetail && selectedFolderId != null) {
            val parentId = selectedFolderId?.let(noteCatalog::folder)?.parentId
            if (parentId != null && noteCatalog.folder(parentId) != null) {
                selectedFolderId = parentId
            } else {
                destination = FoyerDestination.Home
                selectedFolderId = null
                selectedPanel = HomePanel.Notes
            }
        } else if (destination != FoyerDestination.Home) {
            destination = FoyerDestination.Home
            selectedFolderId = null
            selectedNoteId = null
        } else {
            selectedPanel = HomePanel.Apps
        }
    }

    BackHandler(
        enabled = destination == FoyerDestination.Home && selectedPanel != HomePanel.Apps,
    ) {
        selectedPanel = HomePanel.Apps
    }

    PredictiveBackHandler(enabled = destination != FoyerDestination.Home) { events ->
        var receivedGestureProgress = false
        try {
            events.collect { event ->
                receivedGestureProgress = true
                predictiveBackEdge = event.swipeEdge
                predictiveBackProgress.snapTo(event.progress)
            }
            suppressDestinationAnimation = receivedGestureProgress
            navigateBack()
            predictiveBackProgress.snapTo(0f)
        } catch (cancellation: CancellationException) {
            predictiveBackProgress.animateTo(
                targetValue = 0f,
                animationSpec = tween(durationMillis = 180, easing = FastOutSlowInEasing),
            )
            throw cancellation
        }
    }

    LaunchedEffect(destination) {
        suppressDestinationAnimation = false
    }

    BackHandler(enabled = assistantState.visible) {
        assistantController?.dismiss()
    }

    Box(modifier = Modifier.fillMaxSize()) {
        val previewNote = if (
            destination == FoyerDestination.NoteEditor &&
            editingExistingNote &&
            predictiveBackProgress.value > 0f
        ) {
            selectedNoteId?.let(noteCatalog::note)
        } else {
            null
        }
        previewNote?.let { note ->
            NoteDetailScreen(
                note = note,
                folder = noteCatalog.folder(note.folderId),
                status = noteCatalog.status,
                readAloud = noteReadAloud,
                onBack = {},
                onEdit = {},
                onDelete = {},
            )
        }

    AnimatedContent(
        targetState = destination,
        transitionSpec = {
            if (suppressDestinationAnimation) {
                return@AnimatedContent (EnterTransition.None togetherWith ExitTransition.None)
            }
            val movingForward = destinationDepth(targetState) > destinationDepth(initialState)
            if (movingForward) {
                (slideInHorizontally(
                    animationSpec = tween(280, easing = FastOutSlowInEasing),
                    initialOffsetX = { width -> width / 7 },
                ) + fadeIn(tween(210))) togetherWith fadeOut(tween(150))
            } else {
                fadeIn(tween(210)) togetherWith (
                    slideOutHorizontally(
                        animationSpec = tween(240, easing = FastOutSlowInEasing),
                        targetOffsetX = { width -> width / 6 },
                    ) + fadeOut(tween(190))
                )
            }.using(SizeTransform(clip = false))
        },
        label = "Foyer destination",
    ) { renderedDestination ->
        val progress = predictiveBackProgress.value
        val direction = if (predictiveBackEdge == BackEventCompat.EDGE_RIGHT) -1f else 1f
        Box(
            modifier = Modifier
                .fillMaxSize()
                .graphicsLayer {
                    if (renderedDestination == destination) {
                        translationX = size.width * 0.22f * progress * direction
                        scaleX = 1f - (0.025f * progress)
                        scaleY = 1f - (0.025f * progress)
                        alpha = 1f - (0.10f * progress)
                    }
                },
        ) {
    when (renderedDestination) {
        FoyerDestination.Onboarding -> OnboardingScreen(
            initialTimezone = timezone,
            onSaveTimezone = onTimezoneChange,
            onFinish = {
                onCompleteOnboarding()
                destination = if (onboardingReturnsToSettings) FoyerDestination.Settings else FoyerDestination.Home
                onboardingReturnsToSettings = false
            },
            onBack = if (onboardingReturnsToSettings) {
                {
                    onboardingReturnsToSettings = false
                    destination = FoyerDestination.Settings
                }
            } else {
                null
            },
        )

        FoyerDestination.Home -> {
            HomeScreen(
                state = visibleState,
                notes = noteCatalog,
                tasks = tasks,
                calendar = calendar,
                contacts = contacts,
                bookmarks = bookmarks,
                homeRequestVersion = homeRequestVersion,
                selectedPanel = selectedPanel,
                onPanelSelected = { selectedPanel = it },
                onOpenActivity = { task ->
                    detailReturnsToSearch = false
                    selectedActivityId = task.id
                    destination = FoyerDestination.ActivityChat
                },
                onOpenFolder = { folderId ->
                    selectedFolderId = folderId
                    selectedNoteId = null
                    destination = FoyerDestination.NoteDetail
                },
                onOpenNote = { noteId ->
                    detailReturnsToSearch = false
                    selectedFolderId = null
                    selectedNoteId = noteId
                    destination = FoyerDestination.NoteDetail
                },
                onCreateNote = {
                    detailReturnsToSearch = false
                    selectedNoteId = null
                    editingExistingNote = false
                    noteWriteError = null
                    destination = FoyerDestination.NoteEditor
                },
                onCreateFolder = { name ->
                    conversationScope.launch {
                        runCatching { onCreateFolder(name, selectedFolderId) }
                            .onFailure {
                                Toast.makeText(context, it.message ?: "Couldn't create folder", Toast.LENGTH_LONG).show()
                            }
                    }
                },
                onRetryNotes = {
                    conversationScope.launch { runCatching { onRefreshNotes() } }
                },
                onOpenTaskList = { listId ->
                    selectedTaskListId = listId
                    selectedTaskId = null
                    destination = FoyerDestination.TaskList
                },
                onOpenTask = { taskId ->
                    detailReturnsToSearch = false
                    selectedTaskId = taskId
                    destination = FoyerDestination.TaskDetail
                },
                onCreateTask = {
                    selectedTaskId = null
                    editingExistingTask = false
                    noteWriteError = null
                    destination = FoyerDestination.TaskEditor
                },
                onCreateTaskList = { name ->
                    conversationScope.launch {
                        runCatching { onCreateTaskList(name) }
                            .onFailure {
                                Toast.makeText(context, it.message ?: "Couldn't create list", Toast.LENGTH_LONG).show()
                            }
                    }
                },
                onRetryTasks = {
                    conversationScope.launch { runCatching { onRefreshNotes() } }
                },
                onSelectCalendar = onSelectCalendar,
                onOpenEvent = { eventId ->
                    detailReturnsToSearch = false
                    selectedEventId = eventId
                    destination = FoyerDestination.EventDetail
                },
                onCreateEvent = {
                    selectedEventId = null
                    editingExistingEvent = false
                    noteWriteError = null
                    conversationScope.launch {
                        if (calendar.calendars.isEmpty()) {
                            runCatching { onCreateCalendar("Calendar", "") }
                                .onFailure {
                                    Toast.makeText(context, it.message ?: "Couldn't create calendar", Toast.LENGTH_LONG).show()
                                }
                        }
                    }
                    destination = FoyerDestination.EventEditor
                },
                onRetryCalendar = {
                    conversationScope.launch { runCatching { onRefreshNotes() } }
                },
                onSelectAddressBook = { selectedAddressBookId = it },
                contactsSearchQuery = contactsSearchQuery,
                onContactsSearchQueryChange = { contactsSearchQuery = it },
                selectedAddressBookId = selectedAddressBookId,
                onOpenContact = { contactId ->
                    detailReturnsToSearch = false
                    selectedContactId = contactId
                    destination = FoyerDestination.ContactDetail
                },
                onCreateContact = {
                    selectedContactId = null
                    editingExistingContact = false
                    noteWriteError = null
                    destination = FoyerDestination.ContactEditor
                },
                onCreateAddressBook = { name ->
                    conversationScope.launch {
                        runCatching { onCreateAddressBook(name) }
                            .onFailure {
                                Toast.makeText(context, it.message ?: "Couldn't create address book", Toast.LENGTH_LONG).show()
                            }
                    }
                },
                onRetryContacts = {
                    conversationScope.launch { runCatching { onRefreshNotes() } }
                },
                onOpenBookmarkFolder = { folderId ->
                    selectedBookmarkFolderId = folderId
                    selectedBookmarkId = null
                    destination = FoyerDestination.BookmarkFolder
                },
                onOpenBookmark = { bookmarkId ->
                    detailReturnsToSearch = false
                    selectedBookmarkId = bookmarkId
                    destination = FoyerDestination.BookmarkDetail
                },
                onCreateBookmark = {
                    selectedBookmarkId = null
                    editingExistingBookmark = false
                    noteWriteError = null
                    destination = FoyerDestination.BookmarkEditor
                },
                onCreateBookmarkFolder = { name ->
                    conversationScope.launch {
                        runCatching { onCreateBookmarkFolder(name, selectedBookmarkFolderId) }
                            .onFailure {
                                Toast.makeText(context, it.message ?: "Couldn't create folder", Toast.LENGTH_LONG).show()
                            }
                    }
                },
                onRetryBookmarks = {
                    conversationScope.launch { runCatching { onRefreshNotes() } }
                },
                onOpenSearch = { query ->
                    searchQuery = query
                    destination = FoyerDestination.SearchResults
                },
                onOpenVoice = openAssistant,
                onOpenSettings = { destination = FoyerDestination.Settings },
                onAskAgent = {
                    onAskAgent(it)
                    selectedPanel = HomePanel.Activity
                },
                onCreateReminder = {
                    onCreateReminder(it)
                    selectedPanel = HomePanel.Tasks
                },
                appsLoading = appsLoading,
                appsErrorMessage = appsErrorMessage,
                onLaunchApp = onLaunchApp,
            )
        }

        FoyerDestination.ActivityChat -> {
            val task = state.tasks.firstOrNull { it.id == selectedActivityId }
            if (task != null) {
                val messageFlow = remember(task.id) { activityMessages(task.id) }
                val storedMessages by messageFlow.collectAsState(initial = emptyList())
                LaunchedEffect(task.id) {
                    runCatching { onRefreshActivity(task.id) }
                }
                ActivityChatScreen(
                    task = task,
                    messages = storedMessages.map(::chatMessage),
                    onBack = {
                        destination = if (detailReturnsToSearch) FoyerDestination.SearchResults else FoyerDestination.Home
                        if (!detailReturnsToSearch) selectedPanel = HomePanel.Activity
                        selectedActivityId = null
                    },
                    onSendMessage = { content ->
                        conversationScope.launch {
                            runCatching { onSendActivityMessage(task.id, content) }
                                .onFailure {
                                    Toast.makeText(
                                        context,
                                        it.message ?: "Couldn't message this activity",
                                        Toast.LENGTH_LONG,
                                    ).show()
                                }
                        }
                    },
                    onSchedule = { runAt, frequency, interval, timezone ->
                        conversationScope.launch {
                            runCatching {
                                onScheduleActivity(task.id, runAt, frequency, interval, timezone)
                            }.onFailure {
                                Toast.makeText(
                                    context,
                                    it.message ?: "Couldn't schedule this activity",
                                    Toast.LENGTH_LONG,
                                ).show()
                            }
                        }
                    },
                    onCancelSchedule = {
                        conversationScope.launch {
                            runCatching { onCancelActivitySchedule(task.id) }
                        }
                    },
                    onRename = { title ->
                        conversationScope.launch {
                            runCatching { onRenameActivity(task.id, title) }
                                .onFailure {
                                    Toast.makeText(context, it.message ?: "Couldn't rename activity", Toast.LENGTH_LONG).show()
                                }
                        }
                    },
                    onDelete = {
                        conversationScope.launch {
                            runCatching { onDeleteActivity(task.id) }
                                .onSuccess {
                                    selectedActivityId = null
                                    destination = FoyerDestination.Home
                                    selectedPanel = HomePanel.Activity
                                }
                                .onFailure {
                                    Toast.makeText(context, it.message ?: "Couldn't delete activity", Toast.LENGTH_LONG).show()
                                }
                        }
                    },
                    onRunNow = {
                        conversationScope.launch {
                            runCatching { onRunActivityNow(task.id) }
                                .onFailure {
                                    Toast.makeText(context, it.message ?: "Couldn't run job", Toast.LENGTH_LONG).show()
                                }
                        }
                    },
                    onRetry = { runId ->
                        conversationScope.launch {
                            runCatching { onRetryActivityRun(task.id, runId) }
                                .onFailure {
                                    Toast.makeText(context, it.message ?: "Couldn't retry run", Toast.LENGTH_LONG).show()
                                }
                        }
                    },
                    readAloud = noteReadAloud,
                    readAloudState = messageReadAloudState,
                    activeReadAloudMessageId = activeReadAloudMessageId,
                    onToggleReadAloud = toggleMessageReadAloud,
                )
            }
        }

        FoyerDestination.NoteEditor -> {
            val existingNote = selectedNoteId?.let(noteCatalog::note).takeIf { editingExistingNote }
            NoteEditorScreen(
                note = existingNote,
                folders = noteCatalog.folders,
                initialFolderId = selectedFolderId,
                status = noteCatalog.status,
                voiceInput = noteVoiceInput,
                saving = noteWriteInFlight,
                saveError = noteWriteError,
                onCancel = {
                    noteWriteError = null
                    destination = if (existingNote == null) FoyerDestination.Home else FoyerDestination.NoteDetail
                    if (existingNote == null) selectedPanel = HomePanel.Notes
                },
                onSave = { title, body, folderId ->
                    if (!noteWriteInFlight) conversationScope.launch {
                        noteWriteInFlight = true
                        noteWriteError = null
                        runCatching {
                            if (existingNote == null) {
                                onCreateNote(title, body, folderId, emptyList())
                            } else {
                                onUpdateNote(existingNote, title, body, folderId)
                            }
                        }.onSuccess { saved ->
                            noteWriteError = null
                            selectedNoteId = saved.id
                            selectedFolderId = if (existingNote == null) null else selectedFolderId
                            editingExistingNote = false
                            destination = FoyerDestination.NoteDetail
                        }.onFailure {
                            noteWriteError = it.message ?: "Couldn't save to the server"
                        }
                        noteWriteInFlight = false
                    }
                },
            )
        }

        FoyerDestination.NoteDetail -> {
            val noteId = selectedNoteId
            val folderId = selectedFolderId
            when {
                noteId != null -> noteCatalog.note(noteId)?.let { note ->
                    NoteDetailScreen(
                        note = note,
                        folder = noteCatalog.folder(note.folderId),
                        status = noteCatalog.status,
                        readAloud = noteReadAloud,
                        onBack = {
                            when {
                                detailReturnsToSearch -> {
                                    detailReturnsToSearch = false
                                    destination = FoyerDestination.SearchResults
                                }
                                selectedFolderId != null -> selectedNoteId = null
                                else -> {
                                    destination = FoyerDestination.Home
                                    selectedPanel = HomePanel.Notes
                                }
                            }
                        },
                        onEdit = {
                            editingExistingNote = true
                            noteWriteError = null
                            destination = FoyerDestination.NoteEditor
                        },
                        onDelete = {
                            if (!noteWriteInFlight) conversationScope.launch {
                                noteWriteInFlight = true
                                runCatching { onDeleteNote(note) }
                                    .onSuccess {
                                        selectedNoteId = null
                                        when {
                                            detailReturnsToSearch -> {
                                                detailReturnsToSearch = false
                                                destination = FoyerDestination.SearchResults
                                            }
                                            selectedFolderId != null -> Unit
                                            else -> {
                                                destination = FoyerDestination.Home
                                                selectedPanel = HomePanel.Notes
                                            }
                                        }
                                    }
                                    .onFailure {
                                        Toast.makeText(
                                            context,
                                            it.message ?: "Couldn't delete note from the server",
                                            Toast.LENGTH_LONG,
                                        ).show()
                                    }
                                noteWriteInFlight = false
                            }
                        },
                    )
                }
                folderId != null -> FolderNotesScreen(
                    catalog = noteCatalog,
                    folderId = folderId,
                    onOpenNote = { selectedNoteId = it },
                    onOpenFolder = { selectedFolderId = it },
                    onCreateNote = {
                        editingExistingNote = false
                        noteWriteError = null
                        destination = FoyerDestination.NoteEditor
                    },
                    onCreateFolder = { name ->
                        conversationScope.launch {
                            runCatching { onCreateFolder(name, folderId) }
                                .onFailure {
                                    Toast.makeText(context, it.message ?: "Couldn't create folder", Toast.LENGTH_LONG).show()
                                }
                        }
                    },
                    onRenameFolder = { name ->
                        noteCatalog.folder(folderId)?.let { folder ->
                            conversationScope.launch {
                                runCatching { onRenameFolder(folder, name) }
                                    .onFailure {
                                        Toast.makeText(context, it.message ?: "Couldn't rename folder", Toast.LENGTH_LONG).show()
                                    }
                            }
                        }
                    },
                    onMoveFolder = { parentId ->
                        noteCatalog.folder(folderId)?.let { folder ->
                            conversationScope.launch {
                                runCatching { onMoveFolder(folder, parentId) }
                                    .onSuccess { moved ->
                                        selectedFolderId = moved.id
                                    }
                                    .onFailure {
                                        Toast.makeText(context, it.message ?: "Couldn't move folder", Toast.LENGTH_LONG).show()
                                    }
                            }
                        }
                    },
                    onDeleteFolder = {
                        noteCatalog.folder(folderId)?.let { folder ->
                            conversationScope.launch {
                                runCatching { onDeleteFolder(folder) }
                                    .onSuccess {
                                        selectedFolderId = folder.parentId
                                        if (selectedFolderId == null) {
                                            destination = FoyerDestination.Home
                                            selectedPanel = HomePanel.Notes
                                        }
                                    }
                                    .onFailure {
                                        Toast.makeText(context, it.message ?: "Couldn't delete folder", Toast.LENGTH_LONG).show()
                                    }
                            }
                        }
                    },
                    onBack = {
                        val parentId = noteCatalog.folder(folderId)?.parentId
                        if (parentId != null && noteCatalog.folder(parentId) != null) {
                            selectedFolderId = parentId
                        } else {
                            selectedFolderId = null
                            destination = FoyerDestination.Home
                            selectedPanel = HomePanel.Notes
                        }
                    },
                )
                else -> Unit
            }
        }

        FoyerDestination.TaskList -> selectedTaskListId?.let { listId ->
            TaskListScreen(
                catalog = tasks,
                listId = listId,
                onOpenTask = { taskId ->
                    selectedTaskId = taskId
                    destination = FoyerDestination.TaskDetail
                },
                onCreateTask = {
                    selectedTaskId = null
                    editingExistingTask = false
                    noteWriteError = null
                    destination = FoyerDestination.TaskEditor
                },
                onRenameList = { name ->
                    tasks.list(listId)?.let { list ->
                        conversationScope.launch {
                            runCatching { onRenameTaskList(list, name) }
                                .onFailure {
                                    Toast.makeText(context, it.message ?: "Couldn't rename list", Toast.LENGTH_LONG).show()
                                }
                        }
                    }
                },
                onDeleteList = {
                    tasks.list(listId)?.let { list ->
                        conversationScope.launch {
                            runCatching { onDeleteTaskList(list) }
                                .onSuccess {
                                    selectedTaskListId = null
                                    destination = FoyerDestination.Home
                                    selectedPanel = HomePanel.Tasks
                                }
                                .onFailure {
                                    Toast.makeText(context, it.message ?: "Couldn't delete list", Toast.LENGTH_LONG).show()
                                }
                        }
                    }
                },
                onBack = {
                    selectedTaskListId = null
                    destination = FoyerDestination.Home
                    selectedPanel = HomePanel.Tasks
                },
            )
        }

        FoyerDestination.TaskDetail -> selectedTaskId?.let { taskId ->
            TaskDetailScreen(
                catalog = tasks,
                taskId = taskId,
                onBack = {
                    if (detailReturnsToSearch) {
                        detailReturnsToSearch = false
                        destination = FoyerDestination.SearchResults
                    } else if (selectedTaskListId != null) {
                        destination = FoyerDestination.TaskList
                    } else {
                        destination = FoyerDestination.Home
                        selectedPanel = HomePanel.Tasks
                    }
                },
                onEdit = {
                    editingExistingTask = true
                    noteWriteError = null
                    destination = FoyerDestination.TaskEditor
                },
                onComplete = {
                    tasks.task(taskId)?.let { task ->
                        conversationScope.launch {
                            runCatching { onCompleteTask(task) }
                                .onFailure {
                                    Toast.makeText(context, it.message ?: "Couldn't complete task", Toast.LENGTH_LONG).show()
                                }
                        }
                    }
                },
                onReopen = {
                    tasks.task(taskId)?.let { task ->
                        conversationScope.launch {
                            runCatching { onReopenTask(task) }
                                .onFailure {
                                    Toast.makeText(context, it.message ?: "Couldn't reopen task", Toast.LENGTH_LONG).show()
                                }
                        }
                    }
                },
                onDelete = {
                    tasks.task(taskId)?.let { task ->
                        conversationScope.launch {
                            runCatching { onDeleteTask(task) }
                                .onSuccess {
                                    selectedTaskId = null
                                    destination = if (selectedTaskListId != null) {
                                        FoyerDestination.TaskList
                                    } else {
                                        FoyerDestination.Home
                                    }
                                    if (destination == FoyerDestination.Home) selectedPanel = HomePanel.Tasks
                                }
                                .onFailure {
                                    Toast.makeText(context, it.message ?: "Couldn't delete task", Toast.LENGTH_LONG).show()
                                }
                        }
                    }
                },
            )
        }

        FoyerDestination.TaskEditor -> TaskEditorScreen(
            task = selectedTaskId?.let(tasks::task).takeIf { editingExistingTask },
            lists = tasks.lists,
            initialListId = selectedTaskListId,
            status = tasks.status,
            saving = noteWriteInFlight,
            saveError = noteWriteError,
            onCancel = {
                noteWriteError = null
                destination = if (editingExistingTask) FoyerDestination.TaskDetail else FoyerDestination.Home
                if (!editingExistingTask) selectedPanel = HomePanel.Tasks
            },
            onSave = { title, description, listId, due, priority ->
                if (!noteWriteInFlight) conversationScope.launch {
                    noteWriteInFlight = true
                    noteWriteError = null
                    val existing = selectedTaskId?.let(tasks::task).takeIf { editingExistingTask }
                    runCatching {
                        if (existing == null) {
                            onCreateTask(title, description, listId, due, priority)
                        } else {
                            onUpdateTask(existing, title, description, due, priority, listId)
                        }
                    }.onSuccess { saved ->
                        selectedTaskId = saved.id
                        editingExistingTask = false
                        destination = FoyerDestination.TaskDetail
                    }.onFailure {
                        noteWriteError = it.message ?: "Couldn't save task"
                    }
                    noteWriteInFlight = false
                }
            },
        )

        FoyerDestination.EventDetail -> selectedEventId?.let { eventId ->
            calendar.event(eventId)?.let { event ->
                EventDetailScreen(
                    event = event,
                    calendar = calendar.calendar(event.calendarId),
                    onBack = {
                        if (detailReturnsToSearch) {
                            detailReturnsToSearch = false
                            destination = FoyerDestination.SearchResults
                        } else {
                            destination = FoyerDestination.Home
                            selectedPanel = HomePanel.Calendar
                        }
                    },
                    onEdit = {
                        editingExistingEvent = true
                        noteWriteError = null
                        destination = FoyerDestination.EventEditor
                    },
                    onDelete = {
                        conversationScope.launch {
                            runCatching { onDeleteEvent(event) }
                                .onSuccess {
                                    selectedEventId = null
                                    destination = FoyerDestination.Home
                                    selectedPanel = HomePanel.Calendar
                                }
                                .onFailure {
                                    Toast.makeText(context, it.message ?: "Couldn't delete event", Toast.LENGTH_LONG).show()
                                }
                        }
                    },
                )
            }
        }

        FoyerDestination.EventEditor -> EventEditorScreen(
            event = selectedEventId?.let(calendar::event).takeIf { editingExistingEvent },
            calendars = calendar.calendars,
            initialCalendarId = calendar.selectedCalendar()?.id,
            status = calendar.status,
            saving = noteWriteInFlight,
            saveError = noteWriteError,
            onCancel = {
                noteWriteError = null
                destination = if (editingExistingEvent) FoyerDestination.EventDetail else FoyerDestination.Home
                if (!editingExistingEvent) selectedPanel = HomePanel.Calendar
            },
            onSave = { draft ->
                if (!noteWriteInFlight) conversationScope.launch {
                    noteWriteInFlight = true
                    noteWriteError = null
                    val existing = selectedEventId?.let(calendar::event).takeIf { editingExistingEvent }
                    runCatching {
                        if (existing == null) onCreateEvent(draft) else onUpdateEvent(existing, draft)
                    }.onSuccess { saved ->
                        selectedEventId = saved.id
                        editingExistingEvent = false
                        destination = FoyerDestination.EventDetail
                    }.onFailure {
                        noteWriteError = it.message ?: "Couldn't save event"
                    }
                    noteWriteInFlight = false
                }
            },
        )

        FoyerDestination.ContactDetail -> selectedContactId?.let { contactId ->
            ContactDetailScreen(
                catalog = contacts,
                contactId = contactId,
                onEdit = {
                    editingExistingContact = true
                    noteWriteError = null
                    destination = FoyerDestination.ContactEditor
                },
                onDelete = {
                    contacts.contact(contactId)?.let { contact ->
                        conversationScope.launch {
                            runCatching { onDeleteContact(contact) }
                                .onSuccess {
                                    selectedContactId = null
                                    destination = FoyerDestination.Home
                                    selectedPanel = HomePanel.Contacts
                                }
                                .onFailure {
                                    Toast.makeText(context, it.message ?: "Couldn't delete contact", Toast.LENGTH_LONG).show()
                                }
                        }
                    }
                },
                onBack = {
                    if (detailReturnsToSearch) {
                        detailReturnsToSearch = false
                        destination = FoyerDestination.SearchResults
                    } else {
                        destination = FoyerDestination.Home
                        selectedPanel = HomePanel.Contacts
                    }
                },
            )
        }

        FoyerDestination.ContactEditor -> ContactEditorScreen(
            catalog = contacts,
            contact = selectedContactId?.let(contacts::contact).takeIf { editingExistingContact },
            initialBookId = selectedAddressBookId,
            saving = noteWriteInFlight,
            saveError = noteWriteError,
            onCancel = {
                noteWriteError = null
                destination = if (editingExistingContact) FoyerDestination.ContactDetail else FoyerDestination.Home
                if (!editingExistingContact) selectedPanel = HomePanel.Contacts
            },
            onSave = { draft ->
                if (!noteWriteInFlight) conversationScope.launch {
                    noteWriteInFlight = true
                    noteWriteError = null
                    val existing = selectedContactId?.let(contacts::contact).takeIf { editingExistingContact }
                    runCatching {
                        if (existing == null) onCreateContact(draft) else onUpdateContact(existing, draft)
                    }.onSuccess { saved ->
                        selectedContactId = saved.id
                        editingExistingContact = false
                        destination = FoyerDestination.ContactDetail
                    }.onFailure {
                        noteWriteError = it.message ?: "Couldn't save contact"
                    }
                    noteWriteInFlight = false
                }
            },
        )

        FoyerDestination.BookmarkFolder -> selectedBookmarkFolderId?.let { folderId ->
            BookmarkFolderScreen(
                catalog = bookmarks,
                folderId = folderId,
                onOpenBookmark = { bookmarkId ->
                    selectedBookmarkId = bookmarkId
                    destination = FoyerDestination.BookmarkDetail
                },
                onOpenFolder = { selectedBookmarkFolderId = it },
                onCreateBookmark = {
                    selectedBookmarkId = null
                    editingExistingBookmark = false
                    noteWriteError = null
                    destination = FoyerDestination.BookmarkEditor
                },
                onCreateFolder = { name ->
                    conversationScope.launch {
                        runCatching { onCreateBookmarkFolder(name, folderId) }
                            .onFailure {
                                Toast.makeText(context, it.message ?: "Couldn't create folder", Toast.LENGTH_LONG).show()
                            }
                    }
                },
                onRenameFolder = { name ->
                    bookmarks.folder(folderId)?.let { folder ->
                        conversationScope.launch {
                            runCatching { onRenameBookmarkFolder(folder, name) }
                                .onFailure {
                                    Toast.makeText(context, it.message ?: "Couldn't rename folder", Toast.LENGTH_LONG).show()
                                }
                        }
                    }
                },
                onMoveFolder = { parentId ->
                    bookmarks.folder(folderId)?.let { folder ->
                        conversationScope.launch {
                            runCatching { onMoveBookmarkFolder(folder, parentId) }
                                .onSuccess { selectedBookmarkFolderId = it.id }
                                .onFailure {
                                    Toast.makeText(context, it.message ?: "Couldn't move folder", Toast.LENGTH_LONG).show()
                                }
                        }
                    }
                },
                onDeleteFolder = {
                    bookmarks.folder(folderId)?.let { folder ->
                        conversationScope.launch {
                            runCatching { onDeleteBookmarkFolder(folder) }
                                .onSuccess {
                                    selectedBookmarkFolderId = folder.parentId
                                    if (selectedBookmarkFolderId == null) {
                                        destination = FoyerDestination.Home
                                        selectedPanel = HomePanel.Bookmarks
                                    }
                                }
                                .onFailure {
                                    Toast.makeText(context, it.message ?: "Couldn't delete folder", Toast.LENGTH_LONG).show()
                                }
                        }
                    }
                },
                onBack = {
                    val parentId = bookmarks.folder(folderId)?.parentId
                    if (parentId != null && bookmarks.folder(parentId) != null) {
                        selectedBookmarkFolderId = parentId
                    } else {
                        selectedBookmarkFolderId = null
                        destination = FoyerDestination.Home
                        selectedPanel = HomePanel.Bookmarks
                    }
                },
            )
        }

        FoyerDestination.BookmarkDetail -> selectedBookmarkId?.let { bookmarkId ->
            bookmarks.bookmark(bookmarkId)?.let { bookmark ->
                BookmarkDetailScreen(
                    bookmark = bookmark,
                    folder = bookmarks.folder(bookmark.folderId),
                    status = bookmarks.status,
                    onBack = {
                        when {
                            detailReturnsToSearch -> {
                                detailReturnsToSearch = false
                                destination = FoyerDestination.SearchResults
                            }
                            selectedBookmarkFolderId != null -> destination = FoyerDestination.BookmarkFolder
                            else -> {
                                destination = FoyerDestination.Home
                                selectedPanel = HomePanel.Bookmarks
                            }
                        }
                    },
                    onEdit = {
                        editingExistingBookmark = true
                        noteWriteError = null
                        destination = FoyerDestination.BookmarkEditor
                    },
                    onToggleFavorite = {
                        conversationScope.launch {
                            runCatching { onSetBookmarkFavorite(bookmark, !bookmark.favorite) }
                                .onFailure {
                                    Toast.makeText(context, it.message ?: "Couldn't update bookmark", Toast.LENGTH_LONG).show()
                                }
                        }
                    },
                    onToggleArchived = {
                        conversationScope.launch {
                            runCatching { onSetBookmarkArchived(bookmark, !bookmark.archived) }
                                .onFailure {
                                    Toast.makeText(context, it.message ?: "Couldn't update bookmark", Toast.LENGTH_LONG).show()
                                }
                        }
                    },
                    onDelete = {
                        conversationScope.launch {
                            runCatching { onDeleteBookmark(bookmark) }
                                .onSuccess {
                                    selectedBookmarkId = null
                                    destination = if (selectedBookmarkFolderId != null) {
                                        FoyerDestination.BookmarkFolder
                                    } else {
                                        FoyerDestination.Home
                                    }
                                    if (destination == FoyerDestination.Home) selectedPanel = HomePanel.Bookmarks
                                }
                                .onFailure {
                                    Toast.makeText(context, it.message ?: "Couldn't delete bookmark", Toast.LENGTH_LONG).show()
                                }
                        }
                    },
                )
            }
        }

        FoyerDestination.BookmarkEditor -> BookmarkEditorScreen(
            bookmark = selectedBookmarkId?.let(bookmarks::bookmark).takeIf { editingExistingBookmark },
            folders = bookmarks.folders,
            initialFolderId = selectedBookmarkFolderId,
            status = bookmarks.status,
            saving = noteWriteInFlight,
            saveError = noteWriteError,
            onCancel = {
                noteWriteError = null
                destination = if (editingExistingBookmark) FoyerDestination.BookmarkDetail else FoyerDestination.Home
                if (!editingExistingBookmark) selectedPanel = HomePanel.Bookmarks
            },
            onSave = { url, title, description, tags, folderId ->
                if (!noteWriteInFlight) conversationScope.launch {
                    noteWriteInFlight = true
                    noteWriteError = null
                    val existing = selectedBookmarkId?.let(bookmarks::bookmark).takeIf { editingExistingBookmark }
                    runCatching {
                        if (existing == null) {
                            onCreateBookmark(folderId, url, title, description, tags, false)
                        } else {
                            onUpdateBookmark(existing, url, title, description, tags, folderId)
                        }
                    }.onSuccess { saved ->
                        selectedBookmarkId = saved.id
                        editingExistingBookmark = false
                        destination = FoyerDestination.BookmarkDetail
                    }.onFailure {
                        noteWriteError = it.message ?: "Couldn't save bookmark"
                    }
                    noteWriteInFlight = false
                }
            },
        )

        FoyerDestination.SearchResults -> SearchResultsScreen(
            query = searchQuery,
            state = visibleState,
            notes = noteCatalog,
            tasks = tasks,
            calendar = calendar,
            contacts = contacts,
            bookmarks = bookmarks,
            onBack = { destination = FoyerDestination.Home },
            onOpenApp = onLaunchApp,
            onOpenNote = { id ->
                selectedNoteId = id
                selectedFolderId = null
                detailReturnsToSearch = true
                destination = FoyerDestination.NoteDetail
            },
            onOpenActivity = { id ->
                selectedActivityId = id
                detailReturnsToSearch = true
                destination = FoyerDestination.ActivityChat
            },
            onOpenTask = { id ->
                selectedTaskId = id
                detailReturnsToSearch = true
                destination = FoyerDestination.TaskDetail
            },
            onOpenEvent = { id ->
                selectedEventId = id
                detailReturnsToSearch = true
                destination = FoyerDestination.EventDetail
            },
            onOpenContact = { id ->
                selectedContactId = id
                detailReturnsToSearch = true
                destination = FoyerDestination.ContactDetail
            },
            onOpenBookmark = { id ->
                selectedBookmarkId = id
                detailReturnsToSearch = true
                destination = FoyerDestination.BookmarkDetail
            },
        )

        FoyerDestination.SyncStatus -> SyncStatusScreen(
            status = syncStatus,
            onSyncNow = onSyncNow,
            onBack = { destination = FoyerDestination.Settings },
        )

        FoyerDestination.MemoryProfile -> MemoryProfileScreen(
            onBack = { destination = FoyerDestination.Settings },
            loadProfile = onLoadProfile,
            loadMemories = onLoadMemories,
            deleteMemory = onDeleteMemory,
        )

        FoyerDestination.NotificationContext -> NotificationContextScreen(
            apps = installedApps,
            whitelist = notificationWhitelist,
            onWhitelistChange = onNotificationWhitelistChange,
            onBack = { destination = FoyerDestination.Settings },
        )

        FoyerDestination.Settings -> SettingsScreen(
            onBack = { destination = FoyerDestination.Home },
            serverUrl = BuildConfig.FOYER_API_BASE_URL,
            signedIn = signedIn,
            grokConnectionStatus = grokConnectionStatus,
            grokConnectionEnabled = grokConnectionEnabled,
            assistantConfigured = assistantRoleHeld,
            timezone = timezone,
            onTimezoneChange = onTimezoneChange,
            onConnectGrok = onConnectGrok,
            onSignOut = onSignOut,
            onOpenSyncStatus = { destination = FoyerDestination.SyncStatus },
            onOpenMemoryProfile = { destination = FoyerDestination.MemoryProfile },
            notificationContextEnabled = notificationContextEnabled,
            notificationAccessGranted = notificationAccessGranted,
            notificationWhitelistCount = notificationWhitelist.size,
            onNotificationContextToggle = onNotificationContextToggle,
            onOpenNotificationWhitelist = {
                destination = FoyerDestination.NotificationContext
            },
            onConfigureAssistant = {
                val manager = roleManager
                if (manager != null && manager.isRoleAvailable(RoleManager.ROLE_ASSISTANT)) {
                    assistantRoleLauncher.launch(manager.createRequestRoleIntent(RoleManager.ROLE_ASSISTANT))
                } else {
                    context.startActivity(Intent(android.provider.Settings.ACTION_VOICE_INPUT_SETTINGS))
                }
            },
            onOpenOnboarding = {
                onboardingReturnsToSettings = true
                destination = FoyerDestination.Onboarding
            },
        )
    }
}
        AssistantOverlayHost(
            state = assistantState,
            onInputChange = assistantController?.let { controller -> controller::editInput } ?: {},
            onToggleListening = assistantController?.let { controller -> controller::toggleListening } ?: {},
            onSubmit = assistantController?.let { controller -> controller::submit } ?: {},
            onConfirm = assistantController?.let { controller -> controller::confirmPendingAction } ?: {},
            onCancelAction = assistantController?.let { controller -> controller::cancelPendingAction } ?: {},
            onDismiss = {
                noteReadAloud.stop()
                activeReadAloudMessageId = null
                assistantController?.dismiss()
            },
            readAloudState = messageReadAloudState,
            activeReadAloudMessageId = activeReadAloudMessageId,
            onToggleReadAloud = toggleMessageReadAloud,
        )
    }
}
}

@Preview(
    name = "Foyer home",
    showBackground = true,
    backgroundColor = 0xFF000000,
    widthDp = 390,
    heightDp = 844,
)
@Composable
private fun FoyerAppPreview() {
    FoyerTheme {
        FoyerAppContent(
            state = FoyerUiState(
                dailyMessage = "",
                moment = null,
                agendaItems = emptyList(),
                todoItems = emptyList(),
                tasks = emptyList(),
                apps = emptyList(),
            ),
            notes = NotesCatalog(emptyList(), emptyList(), emptyList()),
            tasks = TasksCatalog(emptyList(), emptyList()),
            calendar = CalendarCatalog(emptyList(), emptyList()),
            contacts = ContactsCatalog(emptyList(), emptyList()),
            bookmarks = BookmarksCatalog(emptyList(), emptyList(), emptyList()),
            appsLoading = false,
            appsErrorMessage = null,
            homeRequestVersion = 0,
            deepLinkRequestVersion = 0,
            deepLinkTargetType = null,
            deepLinkTargetId = null,
            syncStatus = SyncStatusSnapshot(),
            onSyncNow = {},
            showOnboarding = false,
            onCompleteOnboarding = {},
            timezone = ZoneId.systemDefault().id,
            onTimezoneChange = {},
            onLaunchApp = {},
            onAskAgent = {},
            onCreateReminder = {},
            activityMessages = { kotlinx.coroutines.flow.flowOf(emptyList()) },
            onRefreshActivity = { error("Preview does not refresh activities") },
            onSendActivityMessage = { _, _ -> },
            onScheduleActivity = { _, _, _, _, _ -> },
            onCancelActivitySchedule = {},
            onRenameActivity = { _, _ -> },
            onDeleteActivity = {},
            onRunActivityNow = {},
            onRetryActivityRun = { _, _ -> },
            onCreateNote = { _, _, _, _ -> error("Preview does not write notes") },
            onUpdateNote = { _, _, _, _ -> error("Preview does not write notes") },
            onDeleteNote = {},
            onLoadProfile = { null },
            onLoadMemories = { com.amazity.foyer.model.MemoryPage(emptyList(), null) },
            onDeleteMemory = {},
            notificationContextEnabled = false,
            notificationAccessGranted = false,
            notificationWhitelist = emptySet(),
            installedApps = emptyList(),
            onNotificationContextToggle = {},
            onNotificationWhitelistChange = {},
            signedIn = false,
            grokConnectionStatus = null,
            grokConnectionEnabled = true,
            onConnectGrok = {},
            onSignOut = {},
        )
    }
}

private fun destinationDepth(destination: FoyerDestination): Int = when (destination) {
    FoyerDestination.Home -> 0
    FoyerDestination.ActivityChat,
    FoyerDestination.NoteDetail,
    FoyerDestination.TaskList,
    FoyerDestination.TaskDetail,
    FoyerDestination.EventDetail,
    FoyerDestination.ContactDetail,
    FoyerDestination.BookmarkFolder,
    FoyerDestination.BookmarkDetail,
    FoyerDestination.SearchResults,
    FoyerDestination.Settings,
    -> 1
    FoyerDestination.Onboarding,
    FoyerDestination.NoteEditor,
    FoyerDestination.TaskEditor,
    FoyerDestination.EventEditor,
    FoyerDestination.ContactEditor,
    FoyerDestination.BookmarkEditor,
    FoyerDestination.SyncStatus,
    FoyerDestination.MemoryProfile,
    FoyerDestination.NotificationContext,
    -> 2
}
