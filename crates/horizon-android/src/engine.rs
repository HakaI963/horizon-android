//! Stage B of the port: the engine itself, on the loop the activity handed over.
//!
//! Nothing here is Horizon's game code. It is the one thing a `NativeActivity`
//! has to do that a `main` never does: build the winit event loop out of the
//! `AndroidApp` the system passed in — `EventLoop::new()` panics without it — and
//! give it to the engine, which runs on it exactly as it does on desktop.

use winit::event_loop::EventLoop;
use winit::platform::android::EventLoopBuilderExtAndroid;
use winit::platform::android::activity::AndroidApp;

/// Run the game until it stops.
///
/// The error is a string because the caller is an activity: there is no terminal
/// to print to and no exit status anybody reads, only logcat and whatever the
/// entry point decides to draw.
pub fn run(app: &AndroidApp) -> Result<(), String> {
    let mut builder = EventLoop::builder();
    builder.with_android_app(app.clone());
    let event_loop = builder
        .build()
        .map_err(|error| format!("no event loop: {error}"))?;
    log::info!("Horizon: the activity handed over a loop; starting the engine");
    horizon_client::game::run_with(event_loop).map_err(|error| error.to_string())
}
