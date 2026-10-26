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
//! A menu is the exception. A screen the game draws for itself is hit-tested from a
//! cursor and a button, and Android sends neither, so over one of those a finger on
//! bare glass is said as both — which is the only way a phone can reach Singleplayer.
//!
//! The keyboard is the other half of having no keys. A phone gives one call about it —
//! show the keyboard, or hide it — so this asks for one on the screens that read typed
//! text and keeps it away from the world, where it would sit on top of the buttons.
//!
//! The overlay itself lives on the thread rather than on the renderer, because the
//! engine builds a device for the loading screen and another for the world, and imgui
//! panics when a thread is given a second context.
use super::{Game, Screen};
use horizon_client::{
    Result,
    keybinds::{key_code, mouse_button},
    mouse::Rotation,
    soft_keyboard,
};
use horizon_controls::Action;
use std::sync::atomic::{AtomicBool, Ordering};
use winit::{
    dpi::PhysicalPosition,
    event::{DeviceId, ElementState, MouseButton, Touch, TouchPhase, WindowEvent},
    keyboard::{KeyCode, NativeKeyCode, PhysicalKey},
    window::Window,
};

impl Game<'_> {
    /// What the fingers did since the last frame. Called where the game reads its
    /// keyboard, before anything moves, so a press is in the simulation the frame it
    /// was made and not one after it.
    pub(super) fn touch_frame(&mut self) -> Result<()> {
        // The keyboard before anything else: the screen has just changed, and nobody
        // should wait a frame for the keys to come up when they have opened chat.
        let typing = typing(self);
        if self.window.is_some() {
            let opened = keyboard(self.window.as_deref(), typing) == Some(true);
            // A computer's keyboard is a real one: nothing is selected for it. Only a
            // phone's field, which starts empty over a game field that does not.
            #[cfg(target_os = "android")]
            if opened {
                self.ime_select_all();
            }
            #[cfg(not(target_os = "android"))]
            let _ = opened;
        }
        if typing {
            self.ime_frame()?;
        }
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

    /// The words the phone's keyboard committed since the last frame, said as the
    /// edits the focused screen already takes. Enter is the screen's own Enter: chat
    /// submits, a sign moves to its next line, a book starts a line or saves.
    fn ime_frame(&mut self) -> Result<()> {
        let Some(input) = soft_keyboard::poll() else {
            return Ok(());
        };
        if self.title.is_none() && self.screen == Screen::Chat {
            for _ in 0..input.backspaces {
                self.chat_key_code(KeyCode::Backspace, None);
            }
            if !input.text.is_empty() {
                self.chat_type(&input.text);
            }
            if input.enter {
                self.chat_key_code(KeyCode::Enter, None);
            }
        } else if self.sign_edit.is_some() {
            for _ in 0..input.backspaces {
                self.sign_input(Some(KeyCode::Backspace), None);
            }
            if !input.text.is_empty() {
                self.sign_input(None, Some(&input.text));
            }
            if input.enter {
                self.sign_input(Some(KeyCode::Enter), None);
                // Every line of a sign is a field of its own: the phone's one field
                // is emptied, or the next word would be appended to the line left.
                soft_keyboard::reset();
            }
        } else if self.book_typing() {
            for _ in 0..input.backspaces {
                self.book_input(Some(KeyCode::Backspace), None);
            }
            if !input.text.is_empty() {
                self.book_input(None, Some(&input.text));
            }
            if input.enter {
                self.book_input(Some(KeyCode::Enter), None);
            }
        } else if self.screen == Screen::Creative && self.creative_screen.tab().search {
            for _ in 0..input.backspaces {
                self.creative_search_edit(None, true);
            }
            if !input.text.is_empty() {
                self.creative_search_edit(Some(&input.text), false);
            }
        } else if self.recipe_search_focused() {
            for _ in 0..input.backspaces {
                self.recipe_text(None, true);
            }
            if !input.text.is_empty() {
                self.recipe_text(Some(&input.text), false);
            }
        } else if self.station_typing() {
            for _ in 0..input.backspaces {
                self.station_text(None, true);
            }
            if !input.text.is_empty() {
                self.station_text(Some(&input.text), false);
            }
        } else if self.screen == Screen::WorldOptions && self.world_options_typing() {
            for _ in 0..input.backspaces {
                self.world_options_text(None);
            }
            if !input.text.is_empty() {
                self.world_options_text(Some(&input.text));
            }
        } else if self.screen == Screen::CommandBlock {
            for _ in 0..input.backspaces {
                self.command_block_backspace();
            }
            if !input.text.is_empty() {
                self.command_block_type(&input.text);
            }
            if input.enter {
                self.command_block_confirm();
            }
        } else if self.screen == Screen::Options && self.options_typing() {
            for _ in 0..input.backspaces {
                self.options_key(KeyCode::Backspace, None)?;
            }
            if !input.text.is_empty() {
                self.options_text(&input.text);
            }
        } else if self.title.is_some() {
            for _ in 0..input.backspaces {
                self.title_edit(None, true);
            }
            if !input.text.is_empty() {
                self.title_edit(Some(&input.text), false);
            }
            if input.enter && let Some(title) = self.title.as_mut() {
                title.confirm_edit();
            }
        }
        Ok(())
    }

    /// Select All in the focused screen's field, on the frame the keyboard opens.
    /// The edits above are relative, so what is selected is still the game's text
    /// even though the phone's empty field knows nothing about it.
    fn ime_select_all(&mut self) {
        let all = horizon_client::clipboard::EditShortcut::SelectAll;
        if self.title.is_none() && self.screen == Screen::Chat {
            self.chat_select_all();
        } else if self.sign_edit.is_some() {
            self.sign_shortcut(all);
        } else if self.book_typing() {
            self.book_shortcut(all);
        } else if self.screen == Screen::Creative && self.creative_screen.tab().search {
            self.creative_search_shortcut(all);
        } else if self.recipe_search_focused() {
            self.recipe_shortcut(all);
        } else if self.station_typing() {
            self.station_shortcut(all);
        } else if self.screen == Screen::WorldOptions && self.world_options_typing() {
            self.world_options_shortcut(all);
        } else if self.screen == Screen::CommandBlock {
            self.command_block_select_all();
        } else if self.screen == Screen::Options && self.options_typing() {
            self.options_shortcut(all);
        } else if self.title.is_some() {
            self.title_shortcut(all);
        }
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
        Action::SwapOffhand => "key.swapOffhand",
        Action::Chat => "key.chat",
        Action::Command => "key.command",
        Action::TabList => "key.playerlist",
        Action::Debug => "key.debug.overlay",
        Action::View => "key.togglePerspective",
        Action::Screenshot => "key.screenshot",
        Action::Hotbar(slot) => return Some(format!("key.hotbar.{slot}")),
        Action::Fly | Action::Pause | Action::Menu => return None,
    };
    Some(name.to_owned())
}

/// A finger on the glass, for the overlay that owns the layout — and, over a menu, the
/// pointer that menu is drawn for.
///
/// The game's own pointer stays out of the world: on a phone the buttons *are* the
/// input, and a tap that also moved a cursor nobody can see would take the finger off
/// the button it landed on. A screen the game draws for itself is the other case. winit
/// sends no `CursorMoved` and no `MouseInput` on Android at all, and those screens
/// hit-test a cursor position and a button, so without this the title is a panorama a
/// phone can look at and never leave.
///
/// What comes back is for the caller to say to its own handler: the world and the
/// loading screen are two different windows on the same glass. `screen` is the picture
/// the buttons are placed in, `menu` whether a screen of the game's own is on it.
pub(super) fn finger(
    touch: &Touch,
    screen: [u32; 2],
    menu: bool,
) -> Vec<WindowEvent> {
    // winit reports a touch in the window's physical pixels, the same space the
    // buttons, the menus and a frame dump are in. Scaling it by the display's
    // density here would move every press to twice the distance the finger went.
    let (x, y) = (touch.location.x as f32, touch.location.y as f32);
    let (width, height) = (screen[0] as f32, screen[1] as f32);
    let id = touch.id;
    let pointer = horizon_overlay::with(|overlay| match touch.phase {
        TouchPhase::Started => {
            overlay.finger_down(id, x, y, width, height);
            overlay.finger_is_pointer(id)
        }
        TouchPhase::Moved => {
            overlay.finger_move(id, x, y, width, height);
            overlay.finger_is_pointer(id)
        }
        TouchPhase::Ended | TouchPhase::Cancelled => {
            // Asked before the lift, which is the moment the overlay lets the finger go.
            let was = overlay.finger_is_pointer(id);
            overlay.finger_up(id);
            was
        }
    });
    if !menu || !pointer.unwrap_or(false) {
        return Vec::new();
    }
    let moved = || WindowEvent::CursorMoved {
        device_id: DeviceId::dummy(),
        position: PhysicalPosition::new(f64::from(x), f64::from(y)),
    };
    let button = |state| WindowEvent::MouseInput {
        device_id: DeviceId::dummy(),
        state,
        button: MouseButton::Left,
    };
    match touch.phase {
        TouchPhase::Started => vec![moved(), button(ElementState::Pressed)],
        TouchPhase::Moved => vec![moved()],
        TouchPhase::Ended | TouchPhase::Cancelled => vec![button(ElementState::Released)],
    }
}

/// The title screen's pass over the glass, in the one place the world's own
/// (`touch_frame`) does not run. The buttons have to be read or MENU never opens the
/// panel, and a title screen that cannot open the panel is a phone with no way to move
/// a button sitting where a menu wants to be. The drag is dropped with them: there is
/// no world to walk yet, and a turn saved up would be spent as a camera jerk the frame
/// the player finally gets one. The keyboard is the loading screen's own, in
/// `Loading::draw`: it is the screen that knows which field has the focus.
pub(super) fn menu_frame() {
    let _ = horizon_overlay::with(|overlay| (overlay.presses(), overlay.look()));
}

/// Whether the phone's keyboard is up, so that only a change costs a call into the
/// activity. A new window starts with the keyboard away, and `keyboard_off` says so.
static KEYBOARD_UP: AtomicBool = AtomicBool::new(false);

/// A window has just been made, and with it a keyboard nobody has asked for yet. The
/// flag belongs to the window, not to the process: leaving it as the old window set it
/// would mean the new one never hears from the game again.
pub(super) fn keyboard_off(window: &Window) {
    window.set_ime_allowed(false);
    KEYBOARD_UP.store(false, Ordering::Relaxed);
    // The field the old window's keyboard wrote into is empty for the new one too.
    soft_keyboard::hide();
}

/// Put the phone's keyboard up or away, on the one change since the last frame.
/// `Some(true)` says it was just opened — the caller selects the field it opens over —
/// `Some(false)` that it was just put away, `None` that it was already as asked.
pub(super) fn keyboard(window: Option<&Window>, wanted: bool) -> Option<bool> {
    if KEYBOARD_UP.swap(wanted, Ordering::Relaxed) == wanted {
        return None;
    }
    // winit's Android backend sends the activity a hide request on every
    // `set_ime_allowed(false)`, so even the unchanged state is IPC at frame rate.
    if let Some(window) = window {
        window.set_ime_allowed(wanted);
    }
    if wanted {
        soft_keyboard::show();
    } else {
        soft_keyboard::hide();
    }
    Some(wanted)
}

/// Whether whatever is on the screen takes letters, which on a phone is the only
/// question about the keyboard: winit's Android backend has no caret to aim a composed
/// word at and sends no `Ime` events, so `set_ime_allowed` — show it, or hide it — is
/// the whole of the interface.
///
/// The screens are the ones whose arms in `window_event` pass `event.text` on, so a
/// field that can be written in is a keyboard by the same rule. Each one also has to
/// have a focused field: the title's main menu and an options page between searches
/// are screens no keyboard belongs over. Over the world the keyboard stays away: it
/// would cover the buttons the player moves with.
fn typing(game: &mut Game<'_>) -> bool {
    (game.title.is_none() && game.screen == Screen::Chat)
        || game.sign_edit.is_some()
        || game.book_typing()
        || (game.screen == Screen::Creative && game.creative_screen.tab().search)
        || game.recipe_search_focused()
        || game.station_typing()
        || (game.screen == Screen::WorldOptions && game.world_options_typing())
        || game.screen == Screen::CommandBlock
        || (game.screen == Screen::Options && game.options_typing())
        || game
            .title
            .as_mut()
            .is_some_and(|title| title.focused_field().is_some())
}

/// `AKEYCODE_BACK`, said as the game's own Escape.
///
/// Back is the one key on a phone that the touch layout does not draw, and winit's
/// Android table has no `KeyCode` for it, so it would come in as an unidentified key
/// that nothing listens for. Every screen already answers Escape, which is what a
/// player means when they reach for back.
pub(super) fn back_as_escape(mut event: WindowEvent) -> WindowEvent {
    const BACK: u32 = 4;
    if let WindowEvent::KeyboardInput { event, .. } = &mut event
        && let PhysicalKey::Unidentified(NativeKeyCode::Android(BACK)) = event.physical_key
    {
        event.physical_key = PhysicalKey::Code(KeyCode::Escape);
    }
    event
}
