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

/// The version folder the prepared resources belong to, named by the single
/// source the engine pins them to.
pub const INSTANCE: &str = horizon_core::VANILLA_VERSION;

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
    let resources = resources_dir(app);
    for dir in [
        &root,
        &home,
        &instance,
        &instance.join("saves"),
        &resources,
    ] {
        if let Err(error) = std::fs::create_dir_all(dir) {
            log::warn!("Horizon android: cannot create {}: {error}", dir.display());
        }
    }
    unsafe {
        // Read once by the engine's own path helpers; set before any of them runs.
        std::env::set_var("HOME", &root);
        std::env::set_var("XDG_DATA_HOME", &home);
        std::env::set_var("HORIZON_HOME", &home);
        // An activity gets no command line, so this is how the game is told where
        // its pack is. `Options::read` reads it before the working-directory
        // default, which would otherwise point inside private storage.
        std::env::set_var("HORIZON_RESOURCES", &resources);
        // The touch layout, the same way: the engine has no command line and no way to
        // be handed a path, and it needs one to know where a player's buttons live.
        std::env::set_var("HORIZON_CONTROLS", controls_path(app));
    }
    match std::env::set_current_dir(&instance) {
        Ok(()) => log::info!(
            "Horizon android: working directory {} (data {}, resources {})",
            instance.display(),
            root.display(),
            resources.display()
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

/// The prepared pack the game is told to read: the same `horizon-client/<version>`
/// layout `horizon-prepare` writes under `.local`, in storage a phone's owner can
/// actually reach. Nothing copies itself here — the folder is created so a file
/// manager shows where the pack belongs.
pub fn resources_dir(app: &AndroidApp) -> PathBuf {
    import_dir(app).join("horizon-client").join(INSTANCE)
}

/// Where the touch layout is kept, in the same folder a file manager can reach: a
/// player whose thumb does not land on the jump button should be able to move it
/// with the panel, and then see what they did without pulling anything off the
/// phone.
pub fn controls_path(app: &AndroidApp) -> PathBuf {
    import_dir(app).join("controls.json")
}
