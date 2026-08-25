import org.jetbrains.kotlin.gradle.dsl.JvmTarget

val releaseSigning = mapOf(
    "storeFile" to providers.environmentVariable("FLOWTYPE_ANDROID_KEYSTORE").orNull,
    "storePassword" to providers.environmentVariable("FLOWTYPE_ANDROID_STORE_PASSWORD").orNull,
    "keyAlias" to providers.environmentVariable("FLOWTYPE_ANDROID_KEY_ALIAS").orNull,
    "keyPassword" to providers.environmentVariable("FLOWTYPE_ANDROID_KEY_PASSWORD").orNull,
)
val hasReleaseSigning = releaseSigning.values.any { it != null }
check(!hasReleaseSigning || releaseSigning.values.all { !it.isNullOrBlank() }) {
    "All FLOWTYPE_ANDROID signing environment variables must be set together"
}

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "app.flowtype"
    compileSdk = 36

    defaultConfig {
        applicationId = "app.flowtype"
        minSdk = 29
        targetSdk = 36
        versionCode = 15
        versionName = "0.1.14"
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
    }

    signingConfigs {
        if (hasReleaseSigning) {
            create("release") {
                storeFile = file(releaseSigning.getValue("storeFile")!!)
                storePassword = releaseSigning.getValue("storePassword")
                keyAlias = releaseSigning.getValue("keyAlias")
                keyPassword = releaseSigning.getValue("keyPassword")
            }
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = true
            signingConfig = signingConfigs.findByName("release")
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro",
            )
        }
    }

    sourceSets.getByName("test").resources.srcDir("../../protocol/v1")

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}

tasks.register("packageFlowTypeRelease") {
    dependsOn("assembleRelease")
    doLast {
        val releaseDir = layout.buildDirectory.dir("outputs/apk/release").get().asFile
        val source = sequenceOf(
            releaseDir.resolve("app-release.apk"),
            releaseDir.resolve("app-release-unsigned.apk"),
        ).firstOrNull { it.isFile } ?: error("Release APK was not generated")
        val version = android.defaultConfig.versionName ?: "dev"
        source.copyTo(
            releaseDir.resolve("FlowType-${version}-android-release.apk"),
            overwrite = true,
        )
    }
}

kotlin {
    compilerOptions {
        jvmTarget.set(JvmTarget.JVM_17)
    }
}

dependencies {
    implementation("androidx.activity:activity-ktx:1.10.1")
    implementation("com.journeyapps:zxing-android-embedded:4.3.0")
    implementation("com.squareup.okhttp3:okhttp:4.12.0")
    testImplementation("junit:junit:4.13.2")
    testImplementation("org.json:json:20250517")
    androidTestImplementation("androidx.test:runner:1.7.0")
    androidTestImplementation("androidx.test.ext:junit:1.2.1")
}
