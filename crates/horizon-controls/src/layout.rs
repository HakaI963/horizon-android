//! The layout file a control pad is written in.
//!
//! This is the format launchers have used for years, and it is read here as it is
//! rather than converted into a private shape: a pad the player already has, or one
//! they download, is a file this launcher opens without a translation step and
//! without a loss.
//!
//! A layout is a list of buttons. Each has a name, a list of key codes, a width and a
//! height in density-independent pixels, a position written as a formula over the
//! screen's own numbers, and the colours and corner it is drawn with. Nothing is
//! stored in pixels of a particular phone, which is what makes the same file work on
//! all of them.
//!
//! The key codes are Android's own, because the file is an Android launcher's: a
//! negative code is not a key at all but one of the pad's own buttons (a mouse
//! button, the virtual mouse, the keyboard), and a positive one is a key the game
//! already knows.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::expr::{self, Values};
use crate::keys;

/// A button that is not a key: one of the pad's own.
pub const SPECIAL_KEYBOARD: i32 = -1;
pub const SPECIAL_TOGGLE_CONTROLS: i32 = -2;
pub const SPECIAL_MOUSE_PRIMARY: i32 = -3;
pub const SPECIAL_MOUSE_SECONDARY: i32 = -4;
pub const SPECIAL_VIRTUAL_MOUSE: i32 = -5;
pub const SPECIAL_MOUSE_MIDDLE: i32 = -6;
pub const SPECIAL_SCROLL_UP: i32 = -7;
pub const SPECIAL_SCROLL_DOWN: i32 = -8;
pub const SPECIAL_MENU: i32 = -9;

/// One button, as the file stores it.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ControlData {
    pub name: String,
    /// Up to four keys sent together, Android key codes. Negative values are the
    /// pad's own buttons; `0` (Android's `KEYCODE_UNKNOWN`) is unused padding.
    #[serde(default)]
    pub keycodes: Vec<i32>,
    /// Where the button is, as a formula over the screen.
    #[serde(rename = "dynamicX", default)]
    pub dynamic_x: String,
    #[serde(rename = "dynamicY", default)]
    pub dynamic_y: String,
    /// Width and height in density-independent pixels.
    #[serde(default = "default_side")]
    pub width: f32,
    #[serde(default = "default_side")]
    pub height: f32,
    /// A button that flips between pressed and not, instead of being held.
    #[serde(rename = "isToggle", default)]
    pub is_toggle: bool,
    #[serde(default = "default_opacity")]
    pub opacity: f32,
    /// ARGB, as Android stores a colour: signed, so -1 is opaque white and a file
    /// that says -1 is not a file that failed to load.
    #[serde(rename = "bgColor", default = "default_bg")]
    pub bg_color: i32,
    #[serde(rename = "strokeColor", default = "default_stroke")]
    pub stroke_color: i32,
    /// In density-independent pixels.
    #[serde(rename = "strokeWidth", default)]
    pub stroke_width: f32,
    /// A percentage of half the button's shorter side: 100 is a circle.
    #[serde(rename = "cornerRadius", default)]
    pub corner_radius: f32,
    /// Whether it is drawn over the world, and over a menu.
    #[serde(rename = "displayInGame", default = "default_true")]
    pub display_in_game: bool,
    #[serde(rename = "displayInMenu", default = "default_true")]
    pub display_in_menu: bool,
    /// A swipe across the button is a drag of the camera rather than a press.
    #[serde(rename = "isSwipeable", default)]
    pub is_swipeable: bool,
    /// The touch is also passed to the game as a mouse drag, for a button that
    /// doubles as a look area.
    #[serde(rename = "passThruEnabled", default)]
    pub pass_thru: bool,
}

fn default_side() -> f32 {
    50.0
}
fn default_opacity() -> f32 {
    1.0
}
fn default_bg() -> i32 {
    0x4d00_0000u32 as i32
}
fn default_stroke() -> i32 {
    -1
}
fn default_true() -> bool {
    true
}

impl ControlData {
    /// Whether any of its keys is one of the pad's own buttons.
    pub fn has_special(&self, code: i32) -> bool {
        self.keycodes.contains(&code)
    }

    /// The action this button stands for: the first key it carries, which is the
    /// format's own rule for a button with more than one (the rest are modifiers).
    pub fn action(&self) -> Action {
        self.keycodes
            .iter()
            .copied()
            .find(|code| *code != 0)
            .map(action_for_key)
            .unwrap_or(Action::Raw(0))
    }

    /// The keys the game should be told about: the positive ones.
    pub fn game_keys(&self) -> impl Iterator<Item = i32> + '_ {
        self.keycodes.iter().copied().filter(|code| *code > 0)
    }

    /// Where this button is on a `width`x`height` screen, in pixels.
    ///
    /// A formula that cannot be evaluated puts the button in the top-left corner
    /// rather than nowhere: a pad with one broken button is still a pad, and a
    /// button that has silently vanished is a bug report about a missing feature.
    pub fn position(&self, width: f32, height: f32, density: f32) -> [f32; 2] {
        let (w, h) = (self.px_width(density), self.px_height(density));
        let values = self.values(width, height, w, h, density);
        let x = expr::eval(&self.dynamic_x, &values).unwrap_or(0.0);
        let y = expr::eval(&self.dynamic_y, &values).unwrap_or(0.0);
        // A formula is free to name the button's own size, so the position can land
        // outside the glass on a screen the file was not drawn for. Clamping keeps
        // every button reachable, which is the one thing a pad must guarantee.
        [
            x.clamp(0.0, (width - w).max(0.0)),
            y.clamp(0.0, (height - h).max(0.0)),
        ]
    }

    /// The names a formula may use, as the format defines them.
    fn values(&self, width: f32, height: f32, w: f32, h: f32, density: f32) -> Values {
        let mut values = Values::new();
        values.insert("top".into(), 0.0);
        values.insert("left".into(), 0.0);
        values.insert("right".into(), width - w);
        values.insert("bottom".into(), height - h);
        values.insert("width".into(), w);
        values.insert("height".into(), h);
        values.insert("screen_width".into(), width);
        values.insert("screen_height".into(), height);
        // Two dp of breathing room, the same margin the format's own editor uses.
        values.insert("margin".into(), 2.0 * density);
        values.insert("preferred_scale".into(), 100.0);
        values
    }

    pub fn px_width(&self, density: f32) -> f32 {
        self.width * density
    }

    pub fn px_height(&self, density: f32) -> f32 {
        self.height * density
    }

    /// The corner, in pixels: a percentage of half the shorter side.
    pub fn corner(&self, density: f32) -> f32 {
        let side = self.px_width(density).min(self.px_height(density));
        side / 2.0 * (self.corner_radius / 100.0)
    }

    /// The stroke, in pixels.
    pub fn stroke(&self, density: f32) -> f32 {
        self.stroke_width * density
    }
}

/// A whole pad.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LayoutFile {
    #[serde(default)]
    pub version: i32,
    #[serde(default = "default_scaled_at")]
    pub scaled_at: f32,
    #[serde(rename = "mControlDataList", default)]
    pub controls: Vec<ControlData>,
}

fn default_scaled_at() -> f32 {
    100.0
}

impl LayoutFile {
    /// The layout every install starts with, compiled in.
    ///
    /// It is the format's own default pad rather than something drawn here: the
    /// buttons, their positions, their sizes and their colours are the file's, and
    /// this launcher only decides what the special keys mean.
    pub fn default_pad() -> Self {
        Self::parse(DEFAULT_PAD).unwrap_or_else(|| Self {
            version: 9,
            scaled_at: 100.0,
            controls: Vec::new(),
        })
    }

    /// Read a layout file's text.
    ///
    /// Both the current format and the older one are accepted: the older one has no
    /// `version` and stores a position as a fraction of the screen instead of a
    /// formula, which is exactly the difference this turns into a formula. Its key
    /// codes are converted to Android's on the way in, so everything downstream
    /// speaks one vocabulary.
    pub fn parse(text: &str) -> Option<Self> {
        let value: serde_json::Value = serde_json::from_str(text).ok()?;
        let mut layout = if value.get("version").is_none() {
            upgrade_fractional(&value)
        } else {
            serde_json::from_value(value).ok()?
        };
        layout.normalize_keys();
        Some(layout)
    }

    /// Rewrite the key codes as Android's, whichever vocabulary the file used.
    ///
    /// The version alone is not enough to tell. The compiled-in pad is written in the
    /// old vocabulary and is saved back out with version 9 after conversion, so a file
    /// this build wrote says "Android" while its numbers may still be GLFW's if it was
    /// ever saved before the conversion ran. The numbers are the honest answer, and
    /// they are unambiguous: no Android key code above 255 is a key this pad uses, and
    /// every GLFW special key is above it.
    fn normalize_keys(&mut self) {
        let codes: Vec<i32> = self
            .controls
            .iter()
            .flat_map(|control| control.keycodes.iter().copied())
            .collect();
        let glfw = keys::is_glfw(None, codes.into_iter());
        if !glfw {
            return;
        }
        for control in &mut self.controls {
            for code in &mut control.keycodes {
                *code = keys::to_android(*code, true);
            }
        }
        self.version = 9;
    }

    /// Read a layout from disk.
    pub fn load(path: &Path) -> Option<Self> {
        let text = std::fs::read_to_string(path).ok()?;
        Self::parse(&text)
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = serde_json::to_string_pretty(self)
            .map_err(|error| std::io::Error::other(format!("controls.json: {error}")))?;
        std::fs::write(path, text)
    }

    /// Whether this is a file of the shared format rather than one an older build of
    /// this launcher wrote.
    ///
    /// The format's own marker is a button carrying one of its special key codes: a
    /// private layout has none, because its buttons were actions and not keys. A file
    /// that cannot say which button is the virtual mouse is not a file this launcher
    /// can draw.
    /// Whether this file was written by a build that draws the pad this one draws.
    ///
    /// The format is shared, but a file this launcher saved in an earlier build is not
    /// interchangeable with the current one: an early build named the D-pad's buttons
    /// with the glyphs a different font had, and those names are question marks here.
    /// The marker is the pad's own name for itself, which only this launcher writes.
    pub fn is_current(&self) -> bool {
        self.is_the_shared_format()
            && self
                .controls
                .iter()
                .all(|control| control.name.is_ascii())
    }

    pub fn is_the_shared_format(&self) -> bool {
        self.controls.iter().any(|control| {
            control.keycodes.iter().any(|code| {
                matches!(
                    *code,
                    SPECIAL_KEYBOARD
                        | SPECIAL_TOGGLE_CONTROLS
                        | SPECIAL_MOUSE_PRIMARY
                        | SPECIAL_MOUSE_SECONDARY
                        | SPECIAL_VIRTUAL_MOUSE
                        | SPECIAL_MOUSE_MIDDLE
                        | SPECIAL_SCROLL_UP
                        | SPECIAL_SCROLL_DOWN
                        | SPECIAL_MENU
                )
            })
        })
    }

    /// A button by the name the file gave it.
    pub fn named(&self, name: &str) -> Option<&ControlData> {
        self.controls.iter().find(|control| control.name == name)
    }
}

/// An older file: `x`/`y` as a fraction of the screen, no `version`.
fn upgrade_fractional(value: &serde_json::Value) -> LayoutFile {
    let mut controls = Vec::new();
    if let Some(list) = value.get("mControlDataList").and_then(|v| v.as_array()) {
        for entry in list {
            let Some(object) = entry.as_object() else {
                continue;
            };
            let name = object
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("button")
                .to_string();
            let keycodes = object
                .get("keycodes")
                .and_then(|v| v.as_array())
                .map(|codes| {
                    codes
                        .iter()
                        .filter_map(|c| c.as_i64().map(|c| c as i32))
                        .collect()
                })
                .unwrap_or_default();
            let formula = |key: &str, screen: &str| -> String {
                object
                    .get(key)
                    .and_then(|v| v.as_f64())
                    .map(|fraction| format!("{fraction} * ${{{screen}}}"))
                    .unwrap_or_else(|| "0".to_string())
            };
            controls.push(ControlData {
                name,
                keycodes,
                dynamic_x: formula("x", "screen_width"),
                dynamic_y: formula("y", "screen_height"),
                width: object
                    .get("width")
                    .and_then(|v| v.as_f64())
                    .map_or(50.0, |v| v as f32),
                height: object
                    .get("height")
                    .and_then(|v| v.as_f64())
                    .map_or(50.0, |v| v as f32),
                is_toggle: object
                    .get("isToggle")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
                opacity: object
                    .get("opacity")
                    .and_then(|v| v.as_f64())
                    .map_or(1.0, |v| v as f32),
                bg_color: object
                    .get("bgColor")
                    .and_then(|v| v.as_i64())
                    .map_or(default_bg(), |v| v as i32),
                stroke_color: object
                    .get("strokeColor")
                    .and_then(|v| v.as_i64())
                    .map_or(default_stroke(), |v| v as i32),
                stroke_width: object
                    .get("strokeWidth")
                    .and_then(|v| v.as_f64())
                    .map_or(0.0, |v| v as f32),
                corner_radius: object
                    .get("cornerRadius")
                    .and_then(|v| v.as_f64())
                    .map_or(0.0, |v| v as f32),
                display_in_game: true,
                display_in_menu: true,
                is_swipeable: false,
                pass_thru: false,
            });
        }
    }
    LayoutFile {
        version: 9,
        scaled_at: value
            .get("scaledAt")
            .and_then(|v| v.as_f64())
            .map_or(100.0, |v| v as f32),
        controls,
    }
}

/// The default pad, compiled in so a fresh install has one before any file exists.
const DEFAULT_PAD: &str = include_str!("../assets/mojo_default.json");

/// What a press of a pad button means.
///
/// The pad speaks Android key codes, because the file does. The engine speaks its
/// own actions, because a phone has no keyboard to name them with. This is the one
/// place the two vocabularies meet.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Action {
    // The pad's own buttons.
    Keyboard,
    ToggleControls,
    MousePrimary,
    MouseSecondary,
    MouseMiddle,
    MouseVirtual,
    ScrollUp,
    ScrollDown,
    Menu,
    // Keys the game knows, named for what they do.
    Forward,
    Back,
    Left,
    Right,
    Jump,
    Sneak,
    Sprint,
    Inventory,
    Chat,
    TabList,
    Debug,
    View,
    Drop,
    SwapOffhand,
    Command,
    Screenshot,
    Pause,
    Hotbar(u8),
    /// A key this crate has no name for, kept as the code the file gave so the
    /// engine can still be asked for it.
    Raw(i32),
}

impl Action {
    /// What the button is called on screen, when the layout file did not name it.
    pub fn label(self) -> &'static str {
        match self {
            Action::Keyboard => "KEY",
            Action::ToggleControls => "GUI",
            Action::MousePrimary => "PRI",
            Action::MouseSecondary => "SEC",
            Action::MouseMiddle => "MID",
            Action::MouseVirtual => "MOUSE",
            Action::ScrollUp => "UP",
            Action::ScrollDown => "DOWN",
            Action::Menu => "MENU",
            Action::Forward => "W",
            Action::Back => "S",
            Action::Left => "A",
            Action::Right => "D",
            Action::Jump => "JUMP",
            Action::Sneak => "SNEAK",
            Action::Sprint => "RUN",
            Action::Inventory => "BAG",
            Action::Chat => "CHAT",
            Action::TabList => "TAB",
            Action::Debug => "F3",
            Action::View => "F5",
            Action::Drop => "DROP",
            Action::SwapOffhand => "F",
            Action::Command => "/",
            Action::Screenshot => "F2",
            Action::Pause => "ESC",
            Action::Hotbar(_) => "",
            Action::Raw(_) => "",
        }
    }

    /// Whether the overlay answers this instead of the game.
    ///
    /// These have no key behind them: there is no Android key code for "show a
    /// cursor", and handing one to the engine would be a key it does not know.
    pub fn is_overlay(self) -> bool {
        matches!(
            self,
            Action::ToggleControls | Action::MouseVirtual | Action::Menu
        )
    }

    /// Whether the press is a tap rather than a hold.
    pub fn momentary(self) -> bool {
        matches!(
            self,
            Action::Keyboard
                | Action::Menu
                | Action::MouseVirtual
                | Action::ScrollUp
                | Action::ScrollDown
                | Action::Debug
                | Action::View
                | Action::Screenshot
                | Action::Pause
                | Action::Hotbar(_)
        )
    }
}

/// The action an Android key code stands for, or `None` for a key the game has no
/// use for.
pub fn action_for_key(code: i32) -> Action {
    match code {
        SPECIAL_KEYBOARD => Action::Keyboard,
        SPECIAL_TOGGLE_CONTROLS => Action::ToggleControls,
        SPECIAL_MOUSE_PRIMARY => Action::MousePrimary,
        SPECIAL_MOUSE_SECONDARY => Action::MouseSecondary,
        SPECIAL_MOUSE_MIDDLE => Action::MouseMiddle,
        SPECIAL_VIRTUAL_MOUSE => Action::MouseVirtual,
        SPECIAL_SCROLL_UP => Action::ScrollUp,
        SPECIAL_SCROLL_DOWN => Action::ScrollDown,
        SPECIAL_MENU => Action::Menu,
        // Android's own key codes, named for what the game does with them.
        19 => Action::Forward,      // KEYCODE_DPAD_UP
        20 => Action::Back,         // KEYCODE_DPAD_DOWN
        21 => Action::Left,         // KEYCODE_DPAD_LEFT
        22 => Action::Right,        // KEYCODE_DPAD_RIGHT
        51 => Action::Forward,      // KEYCODE_W
        47 => Action::Back,         // KEYCODE_S
        29 => Action::Left,         // KEYCODE_A
        32 => Action::Right,        // KEYCODE_D
        62 => Action::Jump,         // KEYCODE_SPACE
        59 => Action::Sneak,        // KEYCODE_SHIFT_LEFT
        113 => Action::Sprint,      // KEYCODE_CTRL_LEFT
        33 => Action::Inventory,    // KEYCODE_E
        48 => Action::Chat,         // KEYCODE_T
        61 => Action::TabList,      // KEYCODE_TAB
        133 => Action::Debug,       // KEYCODE_F3
        135 => Action::View,        // KEYCODE_F5
        45 => Action::Drop,         // KEYCODE_Q
        42 => Action::SwapOffhand,  // KEYCODE_F
        76 => Action::Command,      // KEYCODE_SLASH
        132 => Action::Screenshot,  // KEYCODE_F2
        111 => Action::Pause,       // KEYCODE_ESCAPE
        8..=16 => Action::Hotbar((code - 7) as u8), // KEYCODE_1..KEYCODE_9
        other => Action::Raw(other),
    }
}

/// The Android key code an action stands for, when it has one.
pub fn key_for_action(action: Action) -> Option<i32> {
    Some(match action {
        Action::Forward => 51,
        Action::Back => 47,
        Action::Left => 29,
        Action::Right => 32,
        Action::Jump => 62,
        Action::Sneak => 59,
        Action::Sprint => 113,
        Action::Inventory => 33,
        Action::Chat => 48,
        Action::TabList => 61,
        Action::Debug => 133,
        Action::View => 135,
        Action::Drop => 45,
        Action::SwapOffhand => 42,
        Action::Command => 76,
        Action::Screenshot => 132,
        Action::Pause => 111,
        Action::Hotbar(slot) => 7 + i32::from(slot),
        Action::Raw(code) => code,
        Action::Keyboard
        | Action::ToggleControls
        | Action::MousePrimary
        | Action::MouseSecondary
        | Action::MouseMiddle
        | Action::MouseVirtual
        | Action::ScrollUp
        | Action::ScrollDown
        | Action::Menu => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_pad_is_the_formats_own() {
        let pad = LayoutFile::default_pad();
        assert!(pad.controls.len() >= 15, "{} buttons", pad.controls.len());
        assert!(pad.named("PRI").is_some());
        assert!(pad.named("SEC").is_some());
        assert!(pad.named("GUI").is_some());
        assert!(pad.named("Mouse").is_some());
        assert!(pad.named("Keyboard").is_some());
    }

    #[test]
    fn every_default_button_lands_on_the_glass() {
        let pad = LayoutFile::default_pad();
        for (w, h, density) in [
            (1280.0, 720.0, 1.0),
            (1920.0, 953.0, 1.3236),
            (2400.0, 1080.0, 1.5),
        ] {
            for control in &pad.controls {
                let [x, y] = control.position(w, h, density);
                assert!(
                    x >= -0.5 && y >= -0.5,
                    "{} at {x},{y} on {w}x{h}",
                    control.name
                );
                assert!(
                    x + control.px_width(density) <= w + 0.5
                        && y + control.px_height(density) <= h + 0.5,
                    "{} hangs off {w}x{h}",
                    control.name
                );
            }
        }
    }

    #[test]
    fn no_two_default_buttons_share_a_pixel() {
        // Two buttons in the same band pass every other check and still steal each
        // other's taps, and a phone has no right-click to find out which one a player
        // pressed. This is the one mistake a pad cannot recover from.
        let pad = LayoutFile::default_pad();
        let (w, h, density) = (1920.0, 953.0, 1.3236);
        let placed: Vec<(&str, [f32; 4])> = pad
            .controls
            .iter()
            .map(|control| {
                let [x, y] = control.position(w, h, density);
                (
                    control.name.as_str(),
                    [
                        x,
                        y,
                        control.px_width(density),
                        control.px_height(density),
                    ],
                )
            })
            .collect();
        for (i, (name, a)) in placed.iter().enumerate() {
            for (other, b) in placed.iter().skip(i + 1) {
                let apart = a[0] + a[2] <= b[0]
                    || b[0] + b[2] <= a[0]
                    || a[1] + a[3] <= b[1]
                    || b[1] + b[3] <= a[1];
                assert!(apart, "{name} overlaps {other}");
            }
        }
    }

    #[test]
    fn the_pad_moves_with_the_screen() {
        let pad = LayoutFile::default_pad();
        let pri = pad.named("PRI").unwrap();
        let small = pri.position(1280.0, 720.0, 1.0);
        let large = pri.position(2400.0, 1080.0, 1.5);
        // Anchored to the left edge, so it stays near it on both.
        assert!(small[0] < 40.0, "{small:?}");
        assert!(large[0] < 60.0, "{large:?}");
        // And to the bottom, so it follows the height down.
        assert!(small[1] > 720.0 / 2.0);
        assert!(large[1] > 1080.0 / 2.0);
    }

    #[test]
    fn a_broken_formula_still_puts_the_button_somewhere() {
        let control = ControlData {
            name: "x".into(),
            keycodes: vec![],
            dynamic_x: "${nope}".into(),
            dynamic_y: "1 / 0".into(),
            width: 50.0,
            height: 50.0,
            is_toggle: false,
            opacity: 1.0,
            bg_color: 0,
            stroke_color: 0,
            stroke_width: 0.0,
            corner_radius: 0.0,
            display_in_game: true,
            display_in_menu: true,
            is_swipeable: false,
            pass_thru: false,
        };
        assert_eq!(control.position(100.0, 100.0, 1.0), [0.0, 0.0]);
    }

    #[test]
    fn android_keys_become_the_actions_the_game_knows() {
        assert_eq!(action_for_key(51), Action::Forward);
        assert_eq!(action_for_key(62), Action::Jump);
        assert_eq!(action_for_key(8), Action::Hotbar(1));
        assert_eq!(action_for_key(16), Action::Hotbar(9));
        assert_eq!(action_for_key(SPECIAL_VIRTUAL_MOUSE), Action::MouseVirtual);
        assert_eq!(key_for_action(Action::Jump), Some(62));
        assert_eq!(key_for_action(Action::MousePrimary), None);
    }
}
