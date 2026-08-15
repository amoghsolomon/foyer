package com.amazity.foyer.ui.components

import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import com.amazity.foyer.R
import com.amazity.foyer.data.UserSettingsStore
import com.amazity.foyer.ui.theme.FoyerLine
import com.amazity.foyer.ui.theme.FoyerText
import com.amazity.foyer.ui.theme.FoyerTextDim
import java.time.ZoneId

@Composable
fun TimezoneInput(
    value: String,
    onValueChange: (String) -> Unit,
    modifier: Modifier = Modifier,
) {
    val suggestions = remember(value) {
        if (value.isBlank() || UserSettingsStore.isValidTimezone(value)) {
            emptyList()
        } else {
            ZoneId.getAvailableZoneIds().asSequence()
                .filter { it.contains(value, ignoreCase = true) }
                .sorted()
                .take(5)
                .toList()
        }
    }
    Column(modifier = modifier) {
        Text(
            text = stringResource(R.string.timezone_iana_label),
            style = MaterialTheme.typography.labelSmall,
            color = FoyerTextDim,
        )
        BasicTextField(
            value = value,
            onValueChange = onValueChange,
            singleLine = true,
            textStyle = MaterialTheme.typography.bodyMedium.copy(color = FoyerText),
            cursorBrush = SolidColor(FoyerText),
            modifier = Modifier
                .fillMaxWidth()
                .padding(top = 8.dp)
                .border(1.dp, FoyerLine, RoundedCornerShape(12.dp))
                .padding(horizontal = 12.dp, vertical = 13.dp),
        )
        if (value.isNotBlank() && !UserSettingsStore.isValidTimezone(value)) {
            Text(
                text = stringResource(R.string.timezone_invalid),
                style = MaterialTheme.typography.bodySmall,
                color = FoyerTextDim,
                modifier = Modifier.padding(top = 6.dp),
            )
        }
        suggestions.forEach { zone ->
            Text(
                text = zone,
                style = MaterialTheme.typography.bodySmall,
                color = FoyerText,
                modifier = Modifier
                    .fillMaxWidth()
                    .clickable { onValueChange(zone) }
                    .padding(vertical = 8.dp),
            )
        }
    }
}
