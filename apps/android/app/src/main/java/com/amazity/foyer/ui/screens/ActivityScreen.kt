package com.amazity.foyer.ui.screens

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import com.amazity.foyer.model.AgentTask
import com.amazity.foyer.model.FoyerUiState
import com.amazity.foyer.model.TaskState
import com.amazity.foyer.ui.components.HairlineDivider
import com.amazity.foyer.ui.components.ContentStatePanel
import com.amazity.foyer.ui.components.SectionLabel
import com.amazity.foyer.ui.components.TaskRow
import com.amazity.foyer.ui.theme.FoyerLine
import com.amazity.foyer.ui.theme.FoyerSurfaceRaised
import com.amazity.foyer.ui.theme.FoyerTextMuted

@Composable
fun ActivityPage(
    state: FoyerUiState,
    onOpenTask: (AgentTask) -> Unit,
    isLoading: Boolean = false,
    errorMessage: String? = null,
    onRetry: () -> Unit = {},
    modifier: Modifier = Modifier,
) {
    Column(
        modifier = modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(top = 14.dp, bottom = 88.dp),
    ) {
        when {
            isLoading -> {
                com.amazity.foyer.ui.components.LoadingStatePanel("Loading agent activity")
                return@Column
            }
            errorMessage != null -> {
                com.amazity.foyer.ui.components.ErrorStatePanel(errorMessage, onRetry)
                return@Column
            }
            state.tasks.isEmpty() -> {
                ContentStatePanel("No activity yet", "Ask Foyer something from the universal input to start a conversation.")
                return@Column
            }
        }
            SectionLabel("In progress")
            Spacer(Modifier.height(5.dp))
            state.tasks
                .filter { it.state == TaskState.Running || it.state == TaskState.Queued }
                .forEachIndexed { index, task ->
                    TaskRow(
                        task = task,
                        showChevron = true,
                        onClick = { onOpenTask(task) },
                    )
                    if (index == 0) HairlineDivider(modifier = Modifier.padding(start = 18.dp))
                }

            Spacer(Modifier.height(22.dp))
            SectionLabel("Scheduled")
            Spacer(Modifier.height(5.dp))
            state.tasks
                .filter { it.state == TaskState.Scheduled }
                .forEach { task ->
                    TaskRow(
                        task = task,
                        showChevron = true,
                        onClick = { onOpenTask(task) },
                    )
                }

            Spacer(Modifier.height(22.dp))
            SectionLabel("Done")
            Spacer(Modifier.height(5.dp))
            state.tasks
                .filter { it.state == TaskState.Done || it.state == TaskState.Failed }
                .forEach { task ->
                    TaskRow(
                        task = task,
                        showChevron = true,
                        onClick = { onOpenTask(task) },
                    )
                    task.result?.let { result ->
                        Surface(
                            modifier = Modifier
                                .fillMaxWidth()
                                .padding(start = 18.dp, top = 2.dp),
                            shape = RoundedCornerShape(12.dp),
                            color = FoyerSurfaceRaised,
                            border = BorderStroke(1.dp, FoyerLine),
                        ) {
                            Text(
                                text = result,
                                style = MaterialTheme.typography.bodyMedium,
                                color = FoyerTextMuted,
                                modifier = Modifier.padding(14.dp),
                            )
                        }
                    }
                }
        Spacer(Modifier.height(28.dp))
    }
}
