//! Stage B of the port: the engine itself, on the loop the activity handed over.
//!
//! Nothing here is Horizon's game code. It is the one thing a `NativeActivity`
//! has to do that a `main` never does: build the winit event loop out of the
//! `AndroidApp` the system passed in — `EventLoop::new()` panics without it — and
//! give it to the engine, which runs on it exactly as it does on desktop.

use winit::event_loop::EventLoop;
use winit::platform::android::EventLoopBuilderExtAndroid;
use winit::platform::android::activity::AndroidApp;

/// The one event loop this activity gets, built out of the `AndroidApp` the system
/// passed in — `EventLoop::builder().build()` panics without it.
///
/// It is handed out rather than used here because Android gives a process exactly one:
/// constructing a loop registers winit's handler on the activity, and the second call
/// comes back "EventLoop can't be recreated". Whoever holds it is whoever runs, so the
/// choice between the engine and the bring-up card has to be made before it is spent,
/// and nothing can draw after the engine has stopped.
pub fn event_loop(app: &AndroidApp) -> Result<EventLoop<()>, String> {
    let mut builder = EventLoop::builder();
    builder.with_android_app(app.clone());
    builder.build().map_err(|error| error.to_string())
}

/// Run the game on the loop the activity handed over, until it stops.
///
/// The error is a string because the caller is an activity: there is no terminal
/// to print to and no exit status anybody reads, only logcat and whatever the
/// entry point decides to draw.
pub fn run(event_loop: EventLoop<()>) -> Result<(), String> {
    log::info!("Horizon: the activity handed over a loop; starting the engine");
    horizon_client::game::run_with(event_loop).map_err(|error| error.to_string())
}
