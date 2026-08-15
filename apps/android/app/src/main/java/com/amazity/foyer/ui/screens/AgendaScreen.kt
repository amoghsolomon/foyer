package com.amazity.foyer.ui.screens

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.unit.dp
import com.amazity.foyer.model.AgendaDay
import com.amazity.foyer.model.AgendaItem
import com.amazity.foyer.model.FoyerUiState
import com.amazity.foyer.model.TodoItem
import com.amazity.foyer.ui.components.HairlineDivider
import com.amazity.foyer.ui.components.ContentStatePanel
import com.amazity.foyer.ui.components.SectionLabel
import com.amazity.foyer.ui.theme.FoyerText
import com.amazity.foyer.ui.theme.FoyerTextDim
import com.amazity.foyer.ui.theme.FoyerTextMuted
import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter

@Composable
fun AgendaPage(
    state: FoyerUiState,
    onOpenAgendaItem: (AgendaItem) -> Unit,
    onOpenTodoItem: (TodoItem) -> Unit,
    isLoading: Boolean = false,
    errorMessage: String? = null,
    onRetry: () -> Unit = {},
    modifier: Modifier = Modifier,
) {
    val todayItems = state.agendaItems.filter { it.day == AgendaDay.Today }
    val tomorrowItems = state.agendaItems.filter { it.day == AgendaDay.Tomorrow }
    val upcomingItems = state.agendaItems.filter { it.day == AgendaDay.Upcoming }

    Column(
        modifier = modifier
            .fillMaxSize()
            .padding(top = 10.dp, bottom = 76.dp),
    ) {
        if (isLoading) {
            com.amazity.foyer.ui.components.LoadingStatePanel("Loading agenda and tasks")
            return@Column
        }
        if (errorMessage != null) {
            com.amazity.foyer.ui.components.ErrorStatePanel(errorMessage, onRetry)
            return@Column
        }
        AgendaPane(
            todayItems = todayItems,
            tomorrowItems = tomorrowItems,
            upcomingItems = upcomingItems,
            onOpenItem = onOpenAgendaItem,
            modifier = Modifier.weight(1f),
        )
        Spacer(Modifier.height(10.dp))
        HairlineDivider()
        Spacer(Modifier.height(10.dp))
        TodoPane(
            items = state.todoItems,
            onOpenItem = onOpenTodoItem,
            modifier = Modifier.weight(1f),
        )
    }
}

@Composable
private fun AgendaPane(
    todayItems: List<AgendaItem>,
    tomorrowItems: List<AgendaItem>,
    upcomingItems: List<AgendaItem>,
    onOpenItem: (AgendaItem) -> Unit,
    modifier: Modifier = Modifier,
) {
    Column(modifier = modifier.fillMaxWidth()) {
        PaneTitle("Agenda")
        Spacer(Modifier.height(8.dp))
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .weight(1f)
                .verticalScroll(rememberScrollState()),
        ) {
            if (todayItems.isEmpty() && tomorrowItems.isEmpty() && upcomingItems.isEmpty()) {
                ContentStatePanel("Nothing scheduled", "Calendar events and reminders will appear here.")
            }
            AgendaSection(
                label = AgendaDay.Today.label,
                items = todayItems,
                onOpenItem = onOpenItem,
            )
            if (tomorrowItems.isNotEmpty()) {
                Spacer(Modifier.height(16.dp))
                AgendaSection(
                    label = AgendaDay.Tomorrow.label,
                    items = tomorrowItems,
                    onOpenItem = onOpenItem,
                )
            }
            if (upcomingItems.isNotEmpty()) {
                Spacer(Modifier.height(16.dp))
                AgendaSection(
                    label = AgendaDay.Upcoming.label,
                    items = upcomingItems,
                    onOpenItem = onOpenItem,
                )
            }
        }
    }
}

@Composable
private fun AgendaSection(
    label: String,
    items: List<AgendaItem>,
    onOpenItem: (AgendaItem) -> Unit,
) {
    SectionLabel(label)
    Spacer(Modifier.height(3.dp))
    items.forEachIndexed { index, item ->
        AgendaRow(item = item, onClick = { onOpenItem(item) })
        if (index != items.lastIndex) {
            HairlineDivider(modifier = Modifier.padding(start = 82.dp))
        }
    }
}

@Composable
private fun AgendaRow(item: AgendaItem, onClick: () -> Unit) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .clickable(onClick = onClick)
            .padding(vertical = 13.dp),
        verticalAlignment = Alignment.Top,
    ) {
        Text(
            text = if (item.day == AgendaDay.Upcoming) itemDate(item) else item.time,
            style = MaterialTheme.typography.bodySmall,
            color = FoyerTextMuted,
            modifier = Modifier.width(58.dp),
        )
        Box(
            modifier = Modifier
                .width(24.dp)
                .padding(top = 5.dp),
            contentAlignment = Alignment.TopStart,
        ) {
            Box(
                modifier = Modifier
                    .size(8.dp)
                    .background(FoyerTextMuted, RoundedCornerShape(2.dp)),
            )
        }
        Column(modifier = Modifier.weight(1f)) {
            Text(
                text = item.title,
                style = MaterialTheme.typography.bodyLarge,
                color = FoyerText,
            )
            item.detail?.takeIf { it.isNotBlank() }?.let { detail ->
                Text(
                    text = detail,
                    style = MaterialTheme.typography.bodySmall,
                    color = FoyerTextDim,
                    modifier = Modifier.padding(top = 2.dp),
                )
            }
        }
    }
}

@Composable
private fun TodoPane(
    items: List<TodoItem>,
    onOpenItem: (TodoItem) -> Unit,
    modifier: Modifier = Modifier,
) {
    Column(modifier = modifier.fillMaxWidth()) {
        PaneTitle("To do")
        Spacer(Modifier.height(8.dp))
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .weight(1f)
                .verticalScroll(rememberScrollState()),
        ) {
            if (items.isEmpty()) {
                ContentStatePanel("No open tasks", "Quick reminders and Foyer tasks will appear here.")
            }
            items.forEachIndexed { index, item ->
                TodoRow(item = item, onClick = { onOpenItem(item) })
                if (index != items.lastIndex) {
                    HairlineDivider(modifier = Modifier.padding(start = 24.dp))
                }
            }
        }
    }
}

@Composable
private fun PaneTitle(title: String) {
    Text(
        text = title,
        style = MaterialTheme.typography.bodyMedium,
        color = FoyerTextMuted,
        fontWeight = FontWeight.Medium,
    )
}

@Composable
private fun TodoRow(item: TodoItem, onClick: () -> Unit) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .clickable(onClick = onClick)
            .padding(vertical = 14.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(
            modifier = Modifier.width(24.dp),
            contentAlignment = Alignment.CenterStart,
        ) {
            Box(
                modifier = Modifier
                    .size(9.dp)
                    .then(
                        if (item.completed) {
                            Modifier.background(FoyerTextMuted, RoundedCornerShape(2.dp))
                        } else {
                            Modifier.border(1.dp, FoyerTextMuted, RoundedCornerShape(2.dp))
                        },
                    ),
            )
        }
        Column(modifier = Modifier.weight(1f)) {
            Text(
                text = item.title,
                style = MaterialTheme.typography.bodyLarge,
                color = if (item.completed) FoyerTextDim else FoyerText,
                textDecoration = if (item.completed) TextDecoration.LineThrough else null,
            )
            item.description?.takeIf { it.isNotBlank() }?.let { description ->
                Text(
                    text = description,
                    style = MaterialTheme.typography.bodySmall,
                    color = FoyerTextDim,
                    modifier = Modifier.padding(top = 2.dp),
                    maxLines = 2,
                )
            }
        }
    }
}

private fun itemDate(item: AgendaItem): String = item.startsAtEpochMillis?.let { millis ->
    val date = Instant.ofEpochMilli(millis)
        .atZone(ZoneId.systemDefault())
        .format(DateTimeFormatter.ofPattern("d MMM"))
    "$date\n${item.time}"
} ?: item.time
