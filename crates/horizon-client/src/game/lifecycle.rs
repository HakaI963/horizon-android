//! The phone takes the window away, and the game gives the surface up with it.
//!
//! Android stops an activity as soon as it is covered, and the screen locking counts:
//! the `ANativeWindow` behind the Vulkan surface is destroyed, and a new one arrives
//! when the activity comes back. winit says so through `suspended` and then `resumed`,
//! and `resumed` builds a window and a renderer only when it has neither, so the whole
//! of the handling is to give both up the moment the first notice comes. Keeping them
//! is the classic Android failure: the app is alive, drawing into a surface nobody
//! shows, and the player gets a black screen where their world was.
//!
//! `bring_up.rs` in the adapter does exactly this for the diagnostic card, which is
//! where the shape of it comes from.
use super::{Game, Loading};
use winit::event_loop::{ActiveEventLoop, ControlFlow};

/// The window the game draws to is gone: everything made from it goes, and the loop
/// waits instead of polling one that has nothing left to say.
///
/// The renderer is dropped before the window, because it is the renderer that holds the
/// surface and the surface that holds the window. The world is saved first, without
/// waiting: Android can destroy a stopped activity's process without a close or an exit,
/// so `save_for_exit` is never reached, and a kill takes everything since the last
/// autosave with it. A stopped integrated server would not come back, so it keeps running
/// and does the writing on its own thread while the activity is away.
pub(super) fn suspend(game: &mut Game<'_>, event_loop: &ActiveEventLoop) {
    save_for_background(game);
    // The controls are drawn with the same device and the same swapchain, so they go
    // with it; the frame that follows a resume attaches them to the new one.
    horizon_overlay::with(|overlay| overlay.detach());
    game.renderer = None;
    game.window = None;
    event_loop.set_control_flow(ControlFlow::Wait);
    log::info!("lifecycle: the surface is given up until the activity is resumed");
}

/// Start the save the phone's absence asks for, if there is a world to save and
/// nobody is already saving it. Started, never awaited: the event loop has to stay
/// free while the activity is away, and the disk is slow.
fn save_for_background(game: &mut Game<'_>) {
    if game.title.is_some()
        || game.saved_for_exit
        || game.pending_save.is_some()
        || game.remote.is_some()
    {
        return;
    }
    if let Some(server) = &game.integrated {
        server.save_later();
        return;
    }
    game.begin_background_save();
}

/// The same for the loading screen, which has a window and a renderer of its own and
/// builds them the same way: only when it has neither.
pub(super) fn suspend_loading(loading: &mut Loading, event_loop: &ActiveEventLoop) {
    horizon_overlay::with(|overlay| overlay.detach());
    loading.renderer = None;
    loading.window = None;
    event_loop.set_control_flow(ControlFlow::Wait);
}
