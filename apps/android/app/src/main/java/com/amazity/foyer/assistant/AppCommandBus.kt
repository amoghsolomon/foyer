package com.amazity.foyer.assistant

import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.receiveAsFlow

sealed interface AppCommand {
    data class CreateNote(val title: String, val body: String) : AppCommand
}

class AppCommandBus {
    private val channel = Channel<AppCommand>(capacity = Channel.BUFFERED)
    val commands = channel.receiveAsFlow()

    fun send(command: AppCommand): Boolean = channel.trySend(command).isSuccess
}
