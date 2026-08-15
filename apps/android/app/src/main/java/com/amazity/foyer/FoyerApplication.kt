package com.amazity.foyer

import android.app.Application
import android.content.Context
import com.amazity.foyer.assistant.AppCommandBus
import com.amazity.foyer.assistant.AssistantSessionController
import com.amazity.foyer.auth.AuthSessionCoordinator
import com.amazity.foyer.network.FoyerApiClient
import com.amazity.foyer.notifications.FoyerNotifications
import com.amazity.foyer.notifications.NotificationContextManager
import com.amazity.foyer.sync.PersonalDataReplica

class FoyerApplication : Application() {
    val appCommands: AppCommandBus by lazy(::AppCommandBus)
    val authSession: AuthSessionCoordinator by lazy { AuthSessionCoordinator.create(this) }
    val assistantController: AssistantSessionController by lazy {
        AssistantSessionController(this, appCommands)
    }
    val personalData: PersonalDataReplica by lazy {
        PersonalDataReplica(this, FoyerApiClient(authSession))
    }

    override fun onCreate() {
        super.onCreate()
        FoyerNotifications.ensureChannels(this)
        NotificationContextManager(this).initialize()
    }
}

val Context.foyerApplication: FoyerApplication
    get() = applicationContext as FoyerApplication
