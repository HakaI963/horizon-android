//! The touch controls, said in the input the game already understands.
//!
//! A phone has neither a keyboard nor a mouse, so the imgui overlay hit-tests a layout
//! of buttons for it and hands back what it decided: which button was pressed or let go,
//! and how far a finger dragged the glass. Each of those becomes the keystroke or the
//! click the player's own bindings say it is — `pad.rs` does exactly the same for a
//! controller — so nothing downstream of here knows the input came from a thumb. That
//! is what makes the layout editable without touching the game: the buttons are only
//! another way to press `W`.
//!
//! The overlay itself lives on the thread rather than on the renderer, because the
//! engine builds a device for the loading screen and another for the world, and imgui
//! panics when a thread is given a second context.
use super::{Game, Screen};
use horizon_client::{
    Result,
    keybinds::{key_code, mouse_button},
    mouse::Rotation,
};
use horizon_controls::Action;
use winit::event::{Touch, TouchPhase};

impl Game<'_> {
    /// What the fingers did since the last frame. Called where the game reads its
    /// keyboard, before anything moves, so a press is in the simulation the frame it
    /// was made and not one after it.
    pub(super) fn touch_frame(&mut self) -> Result<()> {
        let Some((edges, look)) =
            horizon_overlay::with(|overlay| (overlay.presses(), overlay.look()))
        else {
            return Ok(());
        };
        for (action, down) in edges {
            self.touch_action(action, down)?;
        }
        self.touch_look(look);
        Ok(())
    }

    /// The line in the corner of the controls. A phone has no title bar to read a frame
    /// rate from, and a HUD that has stopped answering has to be visible as one: "0
    /// fingers" while a finger is on the glass says the touches never arrived.
    pub(super) fn overlay_line(&mut self) {
        let Some((fingers, editing)) =
            horizon_overlay::with(|overlay| (overlay.hud().fingers(), overlay.hud().edit))
        else {
            return;
        };
        horizon_overlay::with(|overlay| {
            overlay.set_status(format!(
                "{:.0} fps, {fingers} finger(s){}",
                self.fps,
                if editing { ", editing" } else { "" }
            ));
        });
    }

    /// One button of the layout, as the mapping the player bound to it.
    fn touch_action(&mut self, action: Action, down: bool) -> Result<()> {
        match action {
            // Escape, with the same care the controller's B takes: a screen has to close
            // before the game does.
            Action::Pause => return if down { self.pad_back() } else { Ok(()) },
            Action::Fly => {
                if down {
                    self.touch_fly();
                }
                return Ok(());
            }
            _ => {}
        }
        let Some(name) = mapping(action) else {
            return Ok(());
        };
        // The player's binding rather than the default: the layout follows the same
        // options.txt the keyboard does, so rebinding `E` moves the bag button too.
        let bound = self.settings.keys.get(&name).to_owned();
        if let Some(button) = mouse_button(&bound) {
            // Mining and placing are mouse buttons, and only exist in the world.
            if self.title.is_none() && self.screen == Screen::Playing {
                self.play_mouse(button, down)?;
            }
            return Ok(());
        }
        let Some(key) = key_code(&bound) else {
            return Ok(());
        };
        self.mapped_key(key, down, false)
    }

    /// Creative flight, on and off. Vanilla reaches it by double-tapping jump, which
    /// needs two ticks to tell the taps apart; a button that says `FLY` sets the ability
    /// the second tap would have, and does nothing a double tap would not.
    fn touch_fly(&mut self) {
        if let Some(player) = &mut self.player
            && player.may_fly
        {
            player.flying = !player.flying;
        }
    }

    /// A drag of the glass, in degrees, straight to the view — the way the right stick
    /// turns (`pad_look`). Not through the mouse: the layout has already divided the
    /// pixels by its own sensitivity, and the mouse would divide them by the game's.
    fn touch_look(&mut self, look: [f32; 2]) {
        let [yaw, pitch] = look;
        if yaw == 0.0 && pitch == 0.0 {
            return;
        }
        if self.title.is_some() || self.screen != Screen::Playing || self.player.is_none() {
            return;
        }
        let mut rotation = Rotation {
            x_rot: self.pitch,
            y_rot: self.yaw,
            x_rot_o: self.pitch,
            y_rot_o: self.yaw,
        };
        rotation.turn(f64::from(yaw) / 0.15, f64::from(pitch) / 0.15);
        self.yaw = rotation.y_rot;
        self.pitch = rotation.x_rot;
    }
}

/// The vanilla mapping a button stands for, `None` for the ones that are not keys at
/// all. Flight and the layout's own panel are answered above and here.
fn mapping(action: Action) -> Option<String> {
    let name = match action {
        Action::Forward => "key.forward",
        Action::Back => "key.back",
        Action::Left => "key.left",
        Action::Right => "key.right",
        Action::Jump => "key.jump",
        Action::Sneak => "key.sneak",
        Action::Sprint => "key.sprint",
        // While flying these are the ascend and descend keys flight replaces.
        Action::Up => "key.jump",
        Action::Down => "key.sneak",
        Action::Mine => "key.attack",
        Action::Use => "key.use",
        Action::Pick => "key.pickItem",
        Action::Inventory => "key.inventory",
        Action::Drop => "key.drop",
        Action::Chat => "key.chat",
        Action::Command => "key.command",
        Action::TabList => "key.playerlist",
        Action::Debug => "key.debug.overlay",
        Action::View => "key.togglePerspective",
        Action::Hotbar(slot) => return Some(format!("key.hotbar.{slot}")),
        Action::Fly | Action::Pause | Action::Menu => return None,
    };
    Some(name.to_owned())
}

/// A finger on the glass, for the overlay that owns the layout.
///
/// The game's own pointer stays out of it: on a phone the buttons *are* the input, and a
/// tap that also moved a cursor nobody can see would take the finger off the button it
/// landed on.
pub(super) fn finger(game: &Game<'_>, touch: &Touch) {
    let (width, height) = game
        .renderer
        .as_ref()
        .map_or((1, 1), |r| (r.config.width, r.config.height));
    // A finger is reported in logical pixels and the buttons are placed in the
    // framebuffer's own, which is the space the overlay draws into.
    let scale = game
        .window
        .as_ref()
        .map_or(1.0, |window| window.scale_factor()) as f32;
    let (x, y) = (
        touch.location.x as f32 * scale,
        touch.location.y as f32 * scale,
    );
    let id = touch.id;
    horizon_overlay::with(|overlay| match touch.phase {
        TouchPhase::Started => overlay.finger_down(id, x, y, width as f32, height as f32),
        TouchPhase::Moved => overlay.finger_move(id, x, y, width as f32, height as f32),
        TouchPhase::Ended | TouchPhase::Cancelled => overlay.finger_up(id),
    });
}
