import java.net.URI

plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.compose)
    alias(libs.plugins.ksp)
}

val foyerApiBaseUrl = providers.gradleProperty("FOYER_API_BASE_URL")
    .orElse(providers.environmentVariable("FOYER_API_BASE_URL"))
    .map(String::trim)
val foyerDevToken = providers.gradleProperty("FOYER_DEV_TOKEN")
    .orElse(providers.environmentVariable("FOYER_DEV_TOKEN"))
    .map(String::trim)
val foyerPowerSyncUrl = providers.gradleProperty("FOYER_POWERSYNC_URL")
    .orElse(providers.environmentVariable("FOYER_POWERSYNC_URL"))
    .map(String::trim)

fun quotedBuildConfigValue(value: String): String =
    "\"${value.replace("\\", "\\\\").replace("\"", "\\\"")}\""

android {
    namespace = "com.amazity.foyer"
    compileSdk {
        version = release(36) {
            minorApiLevel = 1
        }
    }

    defaultConfig {
        applicationId = "com.amazity.foyer"
        minSdk = 36
        targetSdk = 36
        versionCode = 1
        versionName = "1.0"

        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
    }

    buildTypes {
        debug {
            buildConfigField(
                "String",
                "FOYER_API_BASE_URL",
                quotedBuildConfigValue(foyerApiBaseUrl.getOrElse("http://10.0.2.2:3583")),
            )
            buildConfigField(
                "String",
                "FOYER_DEV_TOKEN",
                quotedBuildConfigValue(
                    foyerDevToken.getOrElse("foyer-dev-token-do-not-use-outside-development"),
                ),
            )
            buildConfigField(
                "String",
                "FOYER_POWERSYNC_URL",
                quotedBuildConfigValue(foyerPowerSyncUrl.getOrElse("http://10.0.2.2:8080")),
            )
            buildConfigField("boolean", "FOYER_DEVELOPMENT_AUTH", "true")
        }
        release {
            buildConfigField(
                "String",
                "FOYER_API_BASE_URL",
                quotedBuildConfigValue(foyerApiBaseUrl.getOrElse("")),
            )
            buildConfigField(
                "String",
                "FOYER_DEV_TOKEN",
                quotedBuildConfigValue(""),
            )
            buildConfigField(
                "String",
                "FOYER_POWERSYNC_URL",
                quotedBuildConfigValue(foyerPowerSyncUrl.getOrElse("")),
            )
            buildConfigField("boolean", "FOYER_DEVELOPMENT_AUTH", "false")
            optimization {
                enable = false
            }
        }
    }
    compileOptions {
        isCoreLibraryDesugaringEnabled = true
        sourceCompatibility = JavaVersion.VERSION_11
        targetCompatibility = JavaVersion.VERSION_11
    }
    buildFeatures {
        buildConfig = true
        compose = true
    }
    packaging {
        jniLibs.excludes += "**/x86/**"
    }
}

val validateFoyerReleaseApiBaseUrl by tasks.registering {
    group = "verification"
    description = "Fails release builds unless FOYER_API_BASE_URL is an absolute HTTPS URL."
    inputs.property("foyerApiBaseUrl", foyerApiBaseUrl.getOrElse(""))
    doLast {
        val value = inputs.properties["foyerApiBaseUrl"] as String
        val uri = value.takeIf(String::isNotEmpty)?.let { runCatching { URI(it) }.getOrNull() }
        if (value.isEmpty() || uri?.scheme != "https" || uri.host.isNullOrEmpty()) {
            throw GradleException(
                "Release builds require FOYER_API_BASE_URL to be an absolute https:// URL " +
                    "(set -PFOYER_API_BASE_URL=... or the FOYER_API_BASE_URL environment variable).",
            )
        }
    }
}

tasks.configureEach {
    if (name == "preReleaseBuild") dependsOn("validateFoyerReleaseApiBaseUrl")
}

ksp {
    arg("room.schemaLocation", "$projectDir/schemas")
}

dependencies {
    coreLibraryDesugaring(libs.desugar.jdk.libs)
    implementation(platform(libs.androidx.compose.bom))
    implementation(libs.androidx.activity.compose)
    implementation(libs.androidx.compose.material3)
    implementation(libs.androidx.compose.ui)
    implementation(libs.androidx.compose.ui.graphics)
    implementation(libs.androidx.compose.ui.tooling.preview)
    implementation(libs.androidx.core.ktx)
    implementation(libs.androidx.lifecycle.runtime.ktx)
    implementation(libs.androidx.room.ktx)
    implementation(libs.androidx.room.runtime)
    implementation(libs.androidx.work.runtime.ktx)
    implementation(libs.androidx.fragment.ktx)
    implementation(libs.moonshine.voice)
    implementation(libs.powersync.core)
    ksp(libs.androidx.room.compiler)
    testImplementation(libs.junit)
    testImplementation(libs.org.json)
    androidTestImplementation(platform(libs.androidx.compose.bom))
    androidTestImplementation(libs.androidx.compose.ui.test.junit4)
    androidTestImplementation(libs.androidx.espresso.core)
    androidTestImplementation(libs.androidx.junit)
    debugImplementation(libs.androidx.compose.ui.test.manifest)
    debugImplementation(libs.androidx.compose.ui.tooling)
}
