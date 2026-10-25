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
mod engine;
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
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| play(&app))) {
        Ok(()) => log::info!("Horizon for Android stopped"),
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

/// Run the game, or what stands in for it until the game can run.
///
/// A phone gives no terminal and keeps no exit status: whatever goes wrong has to
/// leave something on screen that the tools can still look at. The trouble is that
/// Android allows one winit event loop per activity — building a loop registers
/// winit's handler on it, and a second build is refused — so the loop belongs to
/// `play`, and whichever handler takes it owns the window until the process ends.
/// The choice has to be made before the loop is spent, and a resource pack is the
/// thing that decides: without one the engine cannot load an asset, draw a title or
/// open a world, so the card takes the loop and the engine is not tried.
fn play(app: &winit::platform::android::activity::AndroidApp) {
    let event_loop = match engine::event_loop(app) {
        Ok(event_loop) => event_loop,
        Err(error) => {
            log::error!("Horizon for Android: no event loop: {error}");
            return;
        }
    };
    let pack = paths::resources_dir(app).join("horizon-resources.json");
    if !pack.is_file() {
        let import = paths::import_dir(app);
        log::warn!(
            "Horizon for Android: no prepared pack at {}, showing the bring-up card. Prepare it \
             on a computer with `horizon-prepare` — it drives a JVM, so it cannot be done here — \
             and copy the `.local/horizon-client/{}` folder to {}/",
            pack.display(),
            paths::INSTANCE,
            import.display(),
        );
        if let Err(error) = bring_up::run(app, event_loop, "no resource pack: put one in import/")
        {
            log::error!("Horizon for Android: the bring-up card stopped: {error}");
        }
        return;
    }
    if let Err(error) = engine::run(event_loop) {
        // Nothing can be drawn from here: the engine spent the only loop, and a
        // second one is refused. What the player gets is the logcat line above.
        log::error!("Horizon for Android: the engine stopped: {error}");
    }
}
