// The Android shell has no code of its own: the game is Rust, built by cargo and
// dropped into the app's jniLibs by .github/workflows/android-apk.yml.
plugins {
    id("com.android.application") version "8.7.3" apply false
}
