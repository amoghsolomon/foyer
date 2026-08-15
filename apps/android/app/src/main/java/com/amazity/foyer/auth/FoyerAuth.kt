package com.amazity.foyer.auth

import android.content.Context
import com.amazity.foyer.FoyerApplication
import com.amazity.foyer.network.FoyerApiClient

fun foyerAuthSession(context: Context): AuthSessionCoordinator {
    val app = context.applicationContext
    return if (app is FoyerApplication) app.authSession else AuthSessionCoordinator.create(app)
}

fun foyerApiClient(context: Context): FoyerApiClient = FoyerApiClient(foyerAuthSession(context))
