// Packages the Rust engine as an APK. Nothing is compiled here but the activity
// subclass that hosts the phone keyboard's field; the `.so` comes from cargo, and
// this file only says how it ships.

plugins {
    id("com.android.application")
}

val nativeLibrary = file("src/main/jniLibs/arm64-v8a/libhorizon_android.so")

android {
    namespace = "dev.horizon.android"
    compileSdk = 35
    // Named so CI installs exactly this one: AGP's own default (34.0.0) would ask
    // the SDK for a build-tools package the workflow never downloads.
    buildToolsVersion = "35.0.0"

    defaultConfig {
        applicationId = "dev.horizon.android"
        // AAudio, and wgpu's Vulkan swapchain on Android, both start at API 26.
        minSdk = 26
        targetSdk = 35
        versionCode = (project.findProperty("horizonVersionCode") as String?)?.toIntOrNull() ?: 1
        versionName = (project.findProperty("horizonVersionName") as String?) ?: "0.1.0-android"
        ndk {
            abiFilters += listOf("arm64-v8a")
        }
    }

    buildTypes {
        getByName("debug") {
            // Debuggable, so `adb` can read the game's own storage folder.
            isDebuggable = true
        }
        getByName("release") {
            isMinifyEnabled = false
            // Signed with the debug key until a release key exists: the APK this
            // produces installs on a phone either way.
            signingConfig = signingConfigs.getByName("debug")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    packaging {
        jniLibs {
            useLegacyPackaging = false
        }
    }

    lint {
        // A shell with no Java code and no icon of its own has nothing for lint
        // to judge; the engine is checked by clippy instead.
        abortOnError = false
    }
}

// A missing engine is a broken APK that only fails on the phone, twelve minutes
// after the build looked green. Fail here, in one line.
tasks.register("checkNativeLibrary") {
    description = "Fails when cargo has not produced the engine library yet"
    doLast {
        if (!nativeLibrary.exists()) {
            throw GradleException(
                "${nativeLibrary.path} is missing: build it first with " +
                    "cargo build --target aarch64-linux-android -p horizon-android"
            )
        }
    }
}

tasks.named("preBuild") {
    dependsOn("checkNativeLibrary")
}
