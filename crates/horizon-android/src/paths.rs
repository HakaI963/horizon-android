//! Where the game keeps its files on Android.
//!
//! The engine resolves saves, `options.txt`, its cache and its prepared
//! resources against the *working directory* — that is how a launcher runs
//! several instances side by side. A `NativeActivity` starts in `/`, which an
//! app cannot write to, so the entry point moves the process into the app's own
//! storage before the engine looks at anything. Nothing else in the engine needs
//! to know it is on a phone.

use std::path::PathBuf;
use winit::platform::android::activity::AndroidApp;

/// The version folder the prepared resources belong to, matching Horizon's
/// `.local/horizon-client/<version>` layout. Stage B of the port replaces this
/// with `horizon_core::VANILLA_VERSION`, the single source the engine pins.
pub const INSTANCE: &str = "26.3";

/// The game's own storage: `<internal>/horizon` for logs and crash reports, and
/// `<internal>/horizon/<version>` as the working directory, exactly the folder a
/// desktop launcher would run one instance from.
pub fn enter_app_storage(app: &AndroidApp) {
    let root = app
        .internal_data_path()
        .or_else(|| app.external_data_path())
        .unwrap_or_else(|| PathBuf::from("/data/local/tmp"));
    let home = root.join("horizon");
    let instance = home.join(INSTANCE);
    for dir in [&root, &home, &instance, &instance.join("saves")] {
        if let Err(error) = std::fs::create_dir_all(dir) {
            log::warn!("Horizon android: cannot create {}: {error}", dir.display());
        }
    }
    unsafe {
        // Read once by the engine's own path helpers; set before any of them runs.
        std::env::set_var("HOME", &root);
        std::env::set_var("XDG_DATA_HOME", &home);
        std::env::set_var("HORIZON_HOME", &home);
    }
    match std::env::set_current_dir(&instance) {
        Ok(()) => log::info!(
            "Horizon android: working directory {} (data {})",
            instance.display(),
            root.display()
        ),
        Err(error) => log::error!("Horizon android: cannot enter {}: {error}", instance.display()),
    }
}

/// The external storage folder of the app, which `adb push` reaches without
/// root: where a user drops the resource set prepared on their own computer.
pub fn import_dir(app: &AndroidApp) -> PathBuf {
    app.external_data_path()
        .or_else(|| app.internal_data_path())
        .unwrap_or_else(|| PathBuf::from("/data/local/tmp"))
        .join("import")
}
