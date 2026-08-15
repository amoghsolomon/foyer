package com.amazity.foyer.ui.screens

import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithText
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.amazity.foyer.auth.DeviceEnrollmentPresentation
import com.amazity.foyer.auth.DevicePublicJwk
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class SignInScreenTest {
    @get:Rule
    val compose = createComposeRule()

    @Test
    fun showsPublicEnrollmentAndOmitsPasswordFields() {
        val enrollment = DeviceEnrollmentPresentation(
            deviceKeyId = "cn-I_WNMClehiVp51i_0VpOENW1upEerA8sEam5hn-s",
            publicJwk = DevicePublicJwk(
                x = "MKBCTNIcKUSDii11ySs3526iDZ8AiTo7Tu6KPAqv7D4",
                y = "4Etl6SRW2YiLUrN5vfvVHuhp7x8PxltmWWlbbM4IFyM",
            ),
        )
        compose.setContent {
            SignInScreen(
                loading = false,
                errorMessage = null,
                enrollment = enrollment,
                onRetryEnrollment = {},
                onCopyEnrollment = {},
                onShareEnrollment = {},
            )
        }
        compose.onNodeWithText("Email").assertDoesNotExist()
        compose.onNodeWithText("Password").assertDoesNotExist()
        compose
            .onAllNodesWithText("cn-I_WNMClehiVp51i_0VpOENW1upEerA8sEam5hn-s", substring = true)
            .assertCountEquals(2)
        compose.onNodeWithText("Copy public enrollment").assertIsDisplayed()
        compose.onNodeWithText("Try again after enrollment").assertIsDisplayed()
    }
}
