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
mod clipboard;
#[cfg(target_os = "android")]
mod engine;
#[cfg(target_os = "android")]
mod frame_dump;
#[cfg(target_os = "android")]
mod ime;
#[cfg(target_os = "android")]
mod installer;
#[cfg(target_os = "android")]
mod logcat;
#[cfg(target_os = "android")]
mod paths;

/// The activity, for the seams that need it: the event loop builder, the storage
/// paths, and the clipboard, whose `ClipboardManager` hangs off this one `Context`.
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
    // The clipboard's JNI goes through `APP`, so it can only be registered after it.
    clipboard::install();
    // The soft keyboard's JNI too: the editable view lives on the activity.
    ime::install();
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
///
/// The pack itself is installed here too, and also before the loop: the launcher
/// downloads the pinned client JAR from Mojang and unpacks it with the seed CI
/// prepared, while the activity shows a Java dialog (the only surface that exists
/// before winit). Then the same launch continues into the engine, with no restart.
fn play(app: &winit::platform::android::activity::AndroidApp) {
    let resources = paths::resources_dir(app);
    if !installer::installed(&resources) {
        if !installer::seed_available() {
            log::warn!(
                "Horizon for Android: no prepared pack at {}, showing the bring-up card. This \
                 build has no embedded Minecraft seed; CI builds one, and `horizon-prepare` can \
                 make the folder by hand, but it drives a JVM, so it cannot run here.",
                resources.display(),
            );
            match engine::event_loop(app) {
                Ok(event_loop) => {
                    if let Err(error) =
                        bring_up::run(app, event_loop, "no resource pack: put one in import/")
                    {
                        log::error!("Horizon for Android: the bring-up card stopped: {error}");
                    }
                }
                Err(error) => log::error!("Horizon for Android: no event loop: {error}"),
            }
            return;
        }
        log::info!(
            "Horizon for Android: no pack at {}, installing Minecraft Java from Mojang",
            resources.display()
        );
        if let Err(error) = installer::install(app) {
            log::error!("Horizon for Android: the Minecraft install failed: {error}");
            installer::fail_dialog(&error);
            match engine::event_loop(app) {
                Ok(event_loop) => {
                    let notice = format!("Minecraft install failed: {error}");
                    if let Err(error) = bring_up::run(app, event_loop, &notice) {
                        log::error!("Horizon for Android: the bring-up card stopped: {error}");
                    }
                }
                Err(error) => log::error!("Horizon for Android: no event loop: {error}"),
            }
            return;
        }
    }
    let event_loop = match engine::event_loop(app) {
        Ok(event_loop) => event_loop,
        Err(error) => {
            log::error!("Horizon for Android: no event loop: {error}");
            return;
        }
    };
    if let Err(error) = engine::run(event_loop) {
        // Nothing can be drawn from here: the engine spent the only loop, and a
        // second one is refused. What the player gets is the logcat line above.
        log::error!("Horizon for Android: the engine stopped: {error}");
    }
}
