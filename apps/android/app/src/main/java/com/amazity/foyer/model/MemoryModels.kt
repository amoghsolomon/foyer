package com.amazity.foyer.model

data class ConsolidatedProfile(
    val text: String,
    val updatedAt: String,
)

data class MemoryRecord(
    val id: String,
    val kind: String,
    val content: String,
    val createdAt: String,
)

data class MemoryPage(
    val items: List<MemoryRecord>,
    val nextCursor: String?,
)
