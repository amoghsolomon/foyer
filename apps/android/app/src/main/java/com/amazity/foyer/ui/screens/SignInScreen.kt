package com.amazity.foyer.ui.screens

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import com.amazity.foyer.auth.DeviceEnrollmentPresentation
import com.amazity.foyer.ui.components.FoyerScreen
import com.amazity.foyer.ui.theme.FoyerBlack
import com.amazity.foyer.ui.theme.FoyerText
import com.amazity.foyer.ui.theme.FoyerTextDim

@Composable
fun SignInScreen(
    loading: Boolean,
    errorMessage: String?,
    enrollment: DeviceEnrollmentPresentation,
    developmentAuthAvailable: Boolean = false,
    onRetryEnrollment: () -> Unit,
    onCopyEnrollment: () -> Unit,
    onShareEnrollment: () -> Unit,
    onUseDevelopmentSession: () -> Unit = {},
) {
    FoyerScreen {
        Column(
            modifier = Modifier
                .fillMaxSize()
                .verticalScroll(rememberScrollState())
                .padding(horizontal = 28.dp, vertical = 48.dp),
            verticalArrangement = Arrangement.Center,
        ) {
            Text("FOYER", style = MaterialTheme.typography.labelSmall, color = FoyerTextDim)
            Spacer(Modifier.height(12.dp))
            Text("This device", style = MaterialTheme.typography.displaySmall, color = FoyerText)
            Spacer(Modifier.height(8.dp))
            Text(
                "Foyer uses a local signing key on this phone. An operator must add the public key on the server before this device can sign in. There is no password, email, or registration.",
                style = MaterialTheme.typography.bodyLarge,
                color = FoyerTextDim,
            )
            Spacer(Modifier.height(22.dp))
            Text("DEVICE FINGERPRINT", style = MaterialTheme.typography.labelSmall, color = FoyerTextDim)
            Spacer(Modifier.height(8.dp))
            Text(
                enrollment.fingerprint,
                style = MaterialTheme.typography.bodySmall,
                color = FoyerText,
                fontFamily = FontFamily.Monospace,
            )
            Spacer(Modifier.height(18.dp))
            Text("PUBLIC ENROLLMENT", style = MaterialTheme.typography.labelSmall, color = FoyerTextDim)
            Spacer(Modifier.height(8.dp))
            Text(
                enrollment.enrollmentJson,
                style = MaterialTheme.typography.bodySmall,
                color = FoyerText,
                fontFamily = FontFamily.Monospace,
            )
            errorMessage?.let {
                Spacer(Modifier.height(14.dp))
                Text(it, style = MaterialTheme.typography.bodySmall, color = FoyerTextDim)
            }
            Spacer(Modifier.height(22.dp))
            Button(
                onClick = onRetryEnrollment,
                enabled = !loading,
                shape = RoundedCornerShape(26.dp),
                colors = ButtonDefaults.buttonColors(containerColor = FoyerText, contentColor = FoyerBlack),
                modifier = Modifier.fillMaxWidth().height(52.dp),
            ) {
                Text(if (loading) "Checking enrollment…" else "Try again after enrollment")
            }
            Spacer(Modifier.height(10.dp))
            OutlinedButton(
                onClick = onCopyEnrollment,
                enabled = !loading,
                shape = RoundedCornerShape(26.dp),
                modifier = Modifier.fillMaxWidth().height(52.dp),
            ) {
                Text("Copy public enrollment")
            }
            Spacer(Modifier.height(10.dp))
            OutlinedButton(
                onClick = onShareEnrollment,
                enabled = !loading,
                shape = RoundedCornerShape(26.dp),
                modifier = Modifier.fillMaxWidth().height(52.dp),
            ) {
                Text("Share public enrollment")
            }
            if (developmentAuthAvailable) {
                Spacer(Modifier.height(18.dp))
                Text(
                    "Development authentication is enabled in this debug build. It is rejected outside FOYER_SERVER_ENV=development.",
                    style = MaterialTheme.typography.bodySmall,
                    color = FoyerTextDim,
                )
                Spacer(Modifier.height(10.dp))
                Button(
                    onClick = onUseDevelopmentSession,
                    enabled = !loading,
                    shape = RoundedCornerShape(26.dp),
                    colors = ButtonDefaults.buttonColors(containerColor = FoyerBlack, contentColor = FoyerText),
                    modifier = Modifier.fillMaxWidth().height(52.dp),
                ) {
                    Text(if (loading) "Connecting…" else "Use local development token")
                }
            }
        }
    }
}
