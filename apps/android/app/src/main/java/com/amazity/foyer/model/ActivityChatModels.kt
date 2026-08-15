package com.amazity.foyer.model

enum class ChatRole(val label: String) {
    User("You"),
    Assistant("Foyer"),
    System("System"),
}

enum class ChatMessageState {
    Sending,
    Delivered,
    Failed,
}

data class ChatMessage(
    val id: String,
    val role: ChatRole,
    val content: String,
    val timestamp: String,
    val state: ChatMessageState = ChatMessageState.Delivered,
)
