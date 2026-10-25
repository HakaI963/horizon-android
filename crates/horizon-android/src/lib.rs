//! Horizon on Android: the native entry point the system loader starts.
//!
//! Everything here is the seam between Android's `NativeActivity` lifecycle and
//! the Rust engine, which is compiled unchanged for `aarch64-linux-android` and
//! talks to the GPU through Vulkan and to the screen through winit. No emulator,
//! no JVM: the same engine code, running as arm64 machine code.
//!
//! `android_main` is the symbol `android-activity` (through winit) calls once the
//! process exists for the activity, so this crate is a `cdylib`: the `.so` the APK
//! installs and the activity loads by name.
#![allow(unsafe_code)]

#[cfg(target_os = "android")]
mod bring_up;
#[cfg(target_os = "android")]
mod frame_dump;
#[cfg(target_os = "android")]
mod logcat;
#[cfg(target_os = "android")]
mod paths;

/// The activity, for the seams that need it: the event loop builder, the storage
/// paths and later the JNI environment.
#[cfg(target_os = "android")]
pub(crate) static APP: std::sync::OnceLock<winit::platform::android::activity::AndroidApp> =
    std::sync::OnceLock::new();

/// Called by `ANativeActivity_onCreate`, on the thread Android made for us.
///
/// `android-activity` declares this side as `extern "Rust"`, so it is written the
/// way its docs show rather than with a C ABI, and the activity calls it again
/// when it is destroyed and recreated: everything it needs is idempotent.
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub unsafe extern "Rust" fn android_main(app: winit::platform::android::activity::AndroidApp) {
    logcat::init();
    let _ = APP.set(app.clone());
    paths::enter_app_storage(&app);
    log::info!(
        "Horizon for Android starting: sdk {}, {} {}, import dir {}",
        app.config().sdk_version(),
        std::env::consts::ARCH,
        if cfg!(target_endian = "little") { "little" } else { "big" },
        paths::import_dir(&app).display()
    );
    // A panic here would otherwise unwind into the C entry point and abort the
    // process before the hook's log record reached logcat.
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| bring_up::run(&app))) {
        Ok(Ok(())) => log::info!("Horizon for Android stopped"),
        Ok(Err(error)) => log::error!("Horizon for Android stopped: {error}"),
        Err(panic) => {
            let message = panic
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| panic.downcast_ref::<&str>().copied())
                .unwrap_or("no message");
            log::error!("Horizon for Android panicked: {message}");
        }
    }
}
