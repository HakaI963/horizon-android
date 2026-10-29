//! What the screen asks the game to do: the button layout, and which fingers are
//! pressing what.
//!
//! A phone has no keyboard and one pointer per finger, and the game is written
//! against a keyboard and a mouse that is captured in the middle of the screen.
//! Everything between those two shapes is here: where a button is, what pressing
//! it means, how a drag on the empty glass becomes a camera turn, and nothing of
//! the engine's. Keeping the model free of imgui is what lets the same layout be
//! drawn as live widgets while it is being edited and as solid quads while it is
//! being used, and let a test compare a tap against a button without a renderer.

use std::collections::BTreeSet;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// A request the game understands, named the way the engine names it. The wiring
/// from this to the engine's own key and mouse state is a match over this list,
/// which is the whole reason the list exists: a phone cannot send `KeyCode::Space`,
/// but it can send `Jump`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Serialize, Deserialize)]
pub enum Action {
    Forward,
    Back,
    Left,
    Right,
    Jump,
    Sneak,
    Sprint,
    /// Creative flight, and the two buttons flight replaces: up and down.
    Fly,
    Up,
    Down,
    /// The two mouse buttons, held: mining and placing.
    Mine,
    Use,
    /// Middle click: the creative inventory's picker.
    Pick,
    Inventory,
    Drop,
    /// The F key: the item in the main hand and the off hand trade places.
    SwapOffhand,
    Chat,
    Command,
    TabList,
    Debug,
    Pause,
    /// The view bobbing and third-person toggle the F5 key carries.
    View,
    /// The F2 key: a screenshot, written to the instance's `screenshots` folder.
    Screenshot,
    /// Not a key at all: the button that opens the overlay's own settings panel.
    /// It is in the layout because it has to be draggable like everything else, and
    /// the overlay answers it instead of the game.
    Menu,
    Hotbar(u8),
}

impl Action {
    /// The legend on the button. ASCII only: the font imgui ships is, and a
    /// missing glyph is a box on screen.
    pub fn label(self) -> &'static str {
        match self {
            Action::Forward => "W",
            Action::Back => "S",
            Action::Left => "A",
            Action::Right => "D",
            Action::Jump => "JUMP",
            Action::Sneak => "SNEAK",
            Action::Sprint => "RUN",
            Action::Fly => "FLY",
            Action::Up => "UP",
            Action::Down => "DOWN",
            Action::Mine => "MINE",
            Action::Use => "USE",
            Action::Pick => "PICK",
            Action::Inventory => "BAG",
            Action::Drop => "DROP",
            Action::SwapOffhand => "F",
            Action::Chat => "CHAT",
            Action::Command => "CMD",
            Action::TabList => "TAB",
            Action::Debug => "F3",
            Action::Pause => "ESC",
            Action::View => "F5",
            Action::Screenshot => "F2",
            Action::Menu => "MENU",
            Action::Hotbar(_) => "",
        }
    }

    /// Whether the button is the overlay's own business rather than the game's.
    /// Handing these to the engine would be wrong twice over: there is no key for a
    /// settings panel, and a player who cannot close it has no way back out.
    pub fn is_overlay(self) -> bool {
        matches!(self, Action::Menu)
    }

    /// The number on a hotbar button, or the legend of anything else.
    pub fn text(self) -> String {
        match self {
            Action::Hotbar(slot) => slot.to_string(),
            other => other.label().to_owned(),
        }
    }

    /// A tap is a keystroke: it is held until the finger lifts. Anything the game
    /// treats as a toggle is a tap of its own, and the button lets go at once, so
    /// that a player can leave flight on and still see the button release.
    pub fn momentary(self) -> bool {
        !matches!(
            self,
            Action::Forward
                | Action::Back
                | Action::Left
                | Action::Right
                | Action::Jump
                | Action::Sneak
                | Action::Sprint
                | Action::Mine
                | Action::Use
                | Action::Up
                | Action::Down
        )
    }
}

/// A place on the glass, in the same top-left pixel coordinates winit reports
/// touch in.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub fn contains(self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.x + self.width && y >= self.y && y < self.y + self.height
    }

    /// Where the button is, and how big it is on this screen.
    pub fn center(self) -> [f32; 2] {
        [self.x + self.width / 2.0, self.y + self.height / 2.0]
    }

    /// The rounded corner, and the line weight, both off the button's own size: a
    /// layout scaled down to a small phone still reads.
    pub fn rounding(self) -> f32 {
        self.width.min(self.height) * 0.18
    }
}

/// One button: what it sends, where it sits, and how large it is relative to the
/// screen it was placed on.
///
/// A button is a rectangle, not a square. Cubiq's layouts — the ones a player
/// recognises from other launchers — are built out of wide tabs, tall D-pad arms and
/// circles, and a square-only model cannot express any of them: the shape has to come
/// from the layout, not from the button's own idea of what a button looks like.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Control {
    pub action: Action,
    /// The button's rectangle as fractions of the screen: left, top, width, height.
    ///
    /// This is the shape a Cubiq layout gives, and it is the one that is used when it
    /// is present. `center`/`size` below are what an older file carries, and are read
    /// only when this is absent, so a layout written by a previous build still loads.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bounds: Option<[f32; 4]>,
    /// The middle of the button, as a fraction of the width and the height. A
    /// phone rotates and a virtual display is not the size of the next one, so
    /// nothing in a layout is stored in pixels.
    #[serde(default)]
    pub center: [f32; 2],
    /// The side of the button as a fraction of the screen's shorter edge, so that
    /// the same layout keeps its proportions in both orientations.
    #[serde(default = "default_size")]
    pub size: f32,
    /// This button's own opacity, or `None` to follow the layout's. A player who
    /// wants a barely-there movement pad and a solid mine button can have both.
    /// Absent from a file written before it existed, so the layout's opacity wins.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opacity: Option<f32>,
    /// The glyph drawn instead of the legend, named the way Cubiq names its icons:
    /// `attack`, `jump`, `sneak`, `sprint`, `inventory`, `swap`, `chat`, `pause`,
    /// `keyboard`, `perspective`, `forward`, `back`, `left`, `right`. A button with
    /// none draws its text, which is what the Java-style preset wants.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// A circle rather than a rounded rectangle: Bedrock's own buttons are round.
    #[serde(default)]
    pub round: bool,
}

fn default_size() -> f32 {
    0.1
}

impl Control {
    /// Where this button is on a `width`x`height` screen.
    pub fn rect(&self, width: f32, height: f32, scale: f32) -> Rect {
        if let Some([left, top, w, h]) = self.bounds {
            // A rectangle scales about its own middle, so a button keeps the place it
            // was put at while it grows: scaling from the top-left corner would walk
            // the whole pad across the screen as the player turned the size up.
            let (cx, cy) = (left + w / 2.0, top + h / 2.0);
            let (bw, bh) = (w * width * scale, h * height * scale);
            return Rect {
                x: cx * width - bw / 2.0,
                y: cy * height - bh / 2.0,
                width: bw,
                height: bh,
            };
        }
        let side = width.min(height) * self.size * scale;
        Rect {
            x: self.center[0] * width - side / 2.0,
            y: self.center[1] * height - side / 2.0,
            width: side,
            height: side,
        }
    }
}

/// The whole touch surface: buttons, the drag-anywhere area that turns the camera,
/// and the two numbers the player can set without a keyboard.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Layout {
    /// Every button's size, multiplied by this.
    pub scale: f32,
    /// How solid the buttons are drawn. A HUD that hides the world is worse than
    /// no HUD.
    pub opacity: f32,
    /// Pixels of glass per degree of camera turn. High for a thumb, which cannot
    /// sweep far, and it is the one number a player notices within a minute.
    pub look_sensitivity: f32,
    /// Whether a drag on empty glass turns the camera at all. Off, a button is the
    /// only way to look, which is what a Bluetooth mouse user wants.
    pub drag_to_look: bool,
    /// Which built-in arrangement this is: `"java"` or `"bedrock"`. Kept in the file
    /// so the launcher's own page can light the preset in force, and so a save from
    /// the arrange panel does not forget which one the player picked.
    #[serde(default = "default_preset")]
    pub preset: String,
    pub controls: Vec<Control>,
}

fn default_preset() -> String {
    "java".to_string()
}

/// A button at a fraction of the screen and a fraction of its short edge.
fn control(action: Action, x: f32, y: f32, size: f32) -> Control {
    Control {
        action,
        bounds: None,
        center: [x, y],
        size,
        opacity: None,
        icon: None,
        round: false,
    }
}

/// A button from a Cubiq-shaped layout: a rectangle in fractions of the screen, an
/// icon and a shape, with no centre or size of its own.
fn shaped(
    action: Action,
    bounds: [f32; 4],
    icon: Option<&str>,
    round: bool,
) -> Control {
    Control {
        action,
        bounds: Some(bounds),
        center: [bounds[0] + bounds[2] / 2.0, bounds[1] + bounds[3] / 2.0],
        size: bounds[2].max(bounds[3]),
        opacity: None,
        icon: icon.map(str::to_owned),
        round,
    }
}

impl Default for Layout {
    /// The arrangement every launcher of this kind converged on, because a thumb
    /// resting on the bottom-left corner reaches a cross of four buttons without
    /// moving, and the one on the right reaches a cluster of them: movement left,
    /// the two mouse buttons and jump right, the hotbar and the key row along the
    /// top, and the two buttons nobody presses while playing in the corners.
    ///
    /// The numbers are the landscape a phone is held in, and they are chosen so that
    /// no button's middle falls inside another's: a row of hotbar squares in the same
    /// band as the inventory row swallows the taps meant for the row behind it, which
    /// is the one mistake a HUD layout cannot be recovered from by a player who
    /// cannot see why the bag will not open.
    fn default() -> Self {
        let mut controls = vec![
            control(Action::Forward, 0.10, 0.70, 0.105),
            control(Action::Left, 0.045, 0.815, 0.105),
            control(Action::Right, 0.155, 0.815, 0.105),
            control(Action::Back, 0.10, 0.93, 0.105),
            control(Action::Sneak, 0.235, 0.93, 0.095),
            control(Action::Sprint, 0.235, 0.80, 0.09),
            control(Action::Mine, 0.855, 0.885, 0.155),
            control(Action::Use, 0.70, 0.885, 0.125),
            control(Action::Jump, 0.925, 0.72, 0.12),
            control(Action::Up, 0.80, 0.66, 0.09),
            control(Action::Down, 0.945, 0.60, 0.09),
            control(Action::Fly, 0.675, 0.72, 0.09),
            control(Action::Pick, 0.96, 0.47, 0.08),
            control(Action::Menu, 0.045, 0.05, 0.075),
            // The two keys that fit in the gap between the menu button and the
            // hotbar: the off hand and a screenshot, which vanilla reaches with F
            // and F2 and a finger would otherwise have no way to press.
            control(Action::SwapOffhand, 0.18, 0.05, 0.05),
            control(Action::Screenshot, 0.24, 0.05, 0.05),
            control(Action::Pause, 0.045, 0.155, 0.075),
            control(Action::Inventory, 0.55, 0.155, 0.08),
            control(Action::Drop, 0.63, 0.155, 0.08),
            control(Action::Chat, 0.71, 0.155, 0.08),
            control(Action::Command, 0.79, 0.155, 0.08),
            control(Action::TabList, 0.87, 0.155, 0.08),
            control(Action::Debug, 0.95, 0.155, 0.08),
            control(Action::View, 0.95, 0.05, 0.075),
        ];
        for slot in 1..=9u8 {
            controls.push(control(
                Action::Hotbar(slot),
                0.30 + f32::from(slot - 1) * 0.05,
                0.05,
                0.05,
            ));
        }
        Self {
            scale: 1.0,
            opacity: 0.42,
            look_sensitivity: 6.0,
            drag_to_look: true,
            preset: default_preset(),
            controls,
        }
    }
}

impl Layout {
    /// The layout on disk, if there is one and it still has buttons in it. A file
    /// from an older build, or one a player dragged every button out of, is not a
    /// layout.
    pub fn load(path: &Path) -> Option<Self> {
        let text = std::fs::read_to_string(path).ok()?;
        let layout: Self = serde_json::from_str(&text).ok()?;
        if layout.controls.is_empty() {
            log::warn!("controls: {} has no buttons, using the default layout", path.display());
            return None;
        }
        let mut layout = layout;
        layout.clamp();
        log::info!(
            "controls: read {} buttons from {}",
            layout.controls.len(),
            path.display()
        );
        Some(layout)
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = serde_json::to_string_pretty(self)
            .map_err(|error| std::io::Error::other(format!("controls.json: {error}")))?;
        std::fs::write(path, text)
    }

    /// Nothing that would make the HUD unusable is allowed to survive a read or an
    /// edit: a zero-sized button cannot be pressed, and a button off the screen
    /// cannot be dragged back by a finger that cannot see it.
    fn clamp(&mut self) {
        self.scale = self.scale.clamp(0.5, 2.0);
        self.opacity = self.opacity.clamp(0.05, 1.0);
        self.look_sensitivity = self.look_sensitivity.clamp(0.5, 40.0);
        for control in &mut self.controls {
            control.center = [
                control.center[0].clamp(0.02, 0.98),
                control.center[1].clamp(0.02, 0.98),
            ];
            control.size = control.size.clamp(0.03, 0.4);
            control.opacity = control.opacity.map(|opacity| opacity.clamp(0.05, 1.0));
            if let Some(bounds) = &mut control.bounds {
                // A button has to keep its middle on the glass: a rectangle that has
                // been pushed past the edge can still be dragged back, but one whose
                // centre is gone cannot be seen to drag.
                let (w, h) = (bounds[2].clamp(0.02, 1.0), bounds[3].clamp(0.02, 1.0));
                let cx = (bounds[0] + bounds[2] / 2.0).clamp(0.01, 0.99);
                let cy = (bounds[1] + bounds[3] / 2.0).clamp(0.01, 0.99);
                *bounds = [cx - w / 2.0, cy - h / 2.0, w, h];
                control.center = [cx, cy];
            }
        }
    }

    /// Where every button is on this screen. Buttons that have been dragged off the
    /// glass are left in the list — they come back when the screen grows — but they
    /// cannot be pressed and are not drawn.
    pub fn buttons(&self, width: f32, height: f32) -> Vec<(Action, Rect)> {
        self.controls
            .iter()
            .map(|control| (control.action, control.rect(width, height, self.scale)))
            .filter(|(_, rect)| {
                rect.x > -rect.width
                    && rect.y > -rect.height
                    && rect.x < width
                    && rect.y < height
            })
            .collect()
    }

    /// The same rectangles front to back: the smallest first, so that the first one a
    /// finger is matched against is the one it can see most of, and the one it gets.
    /// Painting walks the list backwards for the same reason — the button that wins
    /// the tap has to be the one drawn on top.
    pub fn front_to_back(&self, width: f32, height: f32) -> Vec<(Action, Rect)> {
        let mut buttons = self.buttons(width, height);
        buttons.sort_by(|a, b| {
            a.1.width
                .partial_cmp(&b.1.width)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        buttons
    }

    /// A button's icon and shape, when the layout gave it one.
    pub fn control_look(&self, action: Action) -> Option<(Option<String>, bool)> {
        self.controls
            .iter()
            .find(|control| control.action == action)
            .map(|control| (control.icon.clone(), control.round))
    }

    /// The opacity a button is drawn at: its own, if it has one, else the layout's.
    pub fn opacity_of(&self, action: Action) -> f32 {
        self.controls
            .iter()
            .find(|control| control.action == action)
            .and_then(|control| control.opacity)
            .unwrap_or(self.opacity)
    }
}

/// One finger on the glass, and what it was doing when it landed.
struct Finger {
    /// winit's touch id, which is the hardware slot number on Android.
    id: u64,
    x: f32,
    y: f32,
    /// The button it pressed, if it landed on one. It keeps pressing that button
    /// even after sliding off it, which is what a player expects from a D-pad and
    /// what Minecraft's own touch controls do.
    control: Option<Action>,
    /// Whether it is turning the camera, and where the drag started.
    looking: bool,
}

/// The touch state of one frame: which buttons are held, and how far the camera
/// has been asked to turn since the game last looked.
pub struct Hud {
    pub layout: Layout,
    /// The layout is on screen as live widgets, so a finger can drag it. While this
    /// is set nothing is sent to the game: editing a HUD should not mine a block.
    pub edit: bool,
    /// The button the editor is pointed at. Tapping a button while editing selects
    /// it, and the panel then changes that one button's own size and opacity.
    selected: Option<Action>,
    fingers: Vec<Finger>,
    held: BTreeSet<Action>,
    /// Edges to hand to the engine: `true` is a press, `false` a release. They
    /// outlive the frame that made them, because the engine reads them when it next
    /// ticks and a tap can begin and end inside one frame at 120 fps.
    edges: Vec<(Action, bool)>,
    look: [f32; 2],
}

impl Default for Hud {
    fn default() -> Self {
        Self::new(Layout::default())
    }
}

impl Hud {
    pub fn new(layout: Layout) -> Self {
        Self {
            layout,
            edit: false,
            selected: None,
            fingers: Vec::new(),
            held: BTreeSet::new(),
            edges: Vec::new(),
            look: [0.0, 0.0],
        }
    }

    /// A layout dropped onto a phone that is not the one it was drawn on: nothing
    /// to do but read it back.
    pub fn load(path: &Path) -> Self {
        Self::new(Layout::load(path).unwrap_or_default())
    }

    pub fn save(&self, path: &Path) {
        if let Err(error) = self.layout.save(path) {
            log::error!("controls: cannot write {}: {error}", path.display());
        } else {
            log::info!("controls: wrote {}", path.display());
        }
    }

    /// A finger went down at `x`x`y` on a `width`x`height` screen, and says whether a
    /// button of the layout caught it. Which button, if any, it landed on is decided
    /// here and never revisited: a finger that slides off keeps the button it pressed,
    /// and one that lands on nothing is free to turn the camera. The glass it landed on
    /// nothing with is otherwise the game's to have, so whoever is told about the
    /// finger needs the answer to that question, not just the button.
    pub fn down(&mut self, id: u64, x: f32, y: f32, width: f32, height: f32) -> bool {
        let hit = self
            .buttons(width, height)
            .into_iter()
            .find(|(_, rect)| rect.contains(x, y))
            .map(|(action, _)| action);
        // While the layout is being arranged a finger picks its button up instead of
        // pressing it, and turns nothing: editing a HUD must not mine a block. The
        // panel's own button is the exception: it is the only way back to the switch
        // that ends the arranging, and a MENU that can only be dragged puts the panel
        // behind a door with no handle.
        let editing = self.edit;
        let looking = !editing && hit.is_none() && self.layout.drag_to_look;
        if editing {
            // The button a finger lands on becomes the one the panel edits; landing
            // on bare glass selects nothing.
            self.selected = hit;
        }
        if !editing || hit.is_some_and(Action::is_overlay) {
            if let Some(action) = hit {
                self.press(action);
            }
        }
        self.fingers.push(Finger {
            id,
            x,
            y,
            control: hit,
            looking,
        });
        log::info!(
            "controls: finger {id} down at {x:.0}x{y:.0} on {}",
            match hit {
                Some(action) if editing => format!("{} being moved", action.text()),
                Some(action) => format!("{}", action.text()),
                None if looking => "the look area".to_owned(),
                None => "nothing".to_owned(),
            }
        );
        hit.is_some()
    }

    /// Slide a finger that is already down. While the layout is being edited this
    /// moves the button the finger picked up; otherwise where it lands matters only
    /// if it is still on a button, and how far it travelled is what turns the camera.
    pub fn drag(&mut self, id: u64, x: f32, y: f32, width: f32, height: f32) {
        if self.edit {
            if let Some(action) = self
                .fingers
                .iter()
                .find(|finger| finger.id == id)
                .and_then(|finger| finger.control)
            {
                self.move_button(action, [x, y], width, height);
            }
            return;
        }
        let Some(finger) = self.fingers.iter_mut().find(|finger| finger.id == id) else {
            return;
        };
        let dx = x - finger.x;
        let dy = y - finger.y;
        finger.x = x;
        finger.y = y;
        if finger.looking {
            let sensitivity = self.layout.look_sensitivity.max(0.01);
            self.look[0] += dx / sensitivity;
            self.look[1] += dy / sensitivity;
        }
    }

    pub fn up(&mut self, id: u64) {
        let Some(index) = self.fingers.iter().position(|finger| finger.id == id) else {
            return;
        };
        let finger = self.fingers.remove(index);
        if let Some(action) = finger.control {
            self.release(action);
        }
    }

    /// The activity lost the window under its fingers: every button has to be let
    /// go, or the player comes back to a character running into a wall forever.
    pub fn release_all(&mut self) {
        for action in std::mem::take(&mut self.held) {
            self.edges.push((action, false));
        }
        self.fingers.clear();
    }

    fn press(&mut self, action: Action) {
        if self.held.insert(action) {
            self.edges.push((action, true));
        }
        if action.momentary() {
            self.edges.push((action, false));
            self.held.remove(&action);
        }
    }

    fn release(&mut self, action: Action) {
        if self.held.remove(&action) {
            self.edges.push((action, false));
        }
    }

    /// The buttons held right now, for drawing.
    pub fn held(&self) -> impl Iterator<Item = Action> + '_ {
        self.held.iter().copied()
    }

    pub fn is_held(&self, action: Action) -> bool {
        self.held.contains(&action)
    }

    /// The presses and releases since the last call, oldest first. The engine
    /// applies these to its own key and mouse state.
    pub fn take_edges(&mut self) -> Vec<(Action, bool)> {
        std::mem::take(&mut self.edges)
    }

    /// Degrees since the last call: x is a turn to the right, y a pitch down.
    pub fn take_look(&mut self) -> [f32; 2] {
        std::mem::replace(&mut self.look, [0.0, 0.0])
    }

    pub fn buttons(&self, width: f32, height: f32) -> Vec<(Action, Rect)> {
        self.layout.front_to_back(width, height)
    }

    /// The button the layout is being dragged by, if a finger has hold of one. The
    /// overlay draws that one brighter, because a picked-up button that looks like
    /// every other one reads as a control that is not answering.
    pub fn grabbed(&self) -> Option<Action> {
        self.fingers.iter().find_map(|finger| finger.control)
    }

    /// Put a button's centre at `center`, in the pixels of this screen. Fractions come
    /// back out of it, and nonsense is clamped: a button dragged off the glass can be
    /// dragged back, but one that is nowhere cannot.
    pub fn move_button(&mut self, action: Action, center: [f32; 2], width: f32, height: f32) {
        if let Some(control) = self
            .layout
            .controls
            .iter_mut()
            .find(|control| control.action == action)
        {
            let at = [center[0] / width.max(1.0), center[1] / height.max(1.0)];
            control.center = at;
            // A shaped button moves as a rectangle: its own width and height are kept
            // and only the middle is placed, or dragging one would also resize it.
            if let Some([_, _, w, h]) = control.bounds {
                control.bounds = Some([at[0] - w / 2.0, at[1] - h / 2.0, w, h]);
            }
        }
        self.layout.clamp();
    }

    /// The button the editor is pointed at, if any.
    pub fn selected(&self) -> Option<Action> {
        self.selected
    }

    /// Point the editor at a button, or at none.
    pub fn set_selected(&mut self, action: Option<Action>) {
        self.selected = action;
    }

    /// A button's own size, or `None` when there is no such button.
    pub fn control_size(&self, action: Action) -> Option<f32> {
        self.layout
            .controls
            .iter()
            .find(|control| control.action == action)
            .map(|control| match control.bounds {
                // A shaped button reports its longest side, which is the number the
                // slider sets and the one a player sees it grow by.
                Some([_, _, w, h]) => w.max(h),
                None => control.size,
            })
    }

    /// A button's own opacity, or the layout's when it has none of its own.
    pub fn control_opacity(&self, action: Action) -> Option<f32> {
        self.layout
            .controls
            .iter()
            .find(|control| control.action == action)
            .map(|control| control.opacity.unwrap_or(self.layout.opacity))
    }

    /// Set a button's own size, clamped like every other edit.
    ///
    /// A shaped button is resized about its own middle and keeps its proportions: the
    /// Cubiq layouts draw a wide tab and a tall D-pad arm on purpose, and a size slider
    /// that squared them off would throw that away.
    pub fn set_control_size(&mut self, action: Action, size: f32) {
        if let Some(control) = self
            .layout
            .controls
            .iter_mut()
            .find(|control| control.action == action)
        {
            control.size = size;
            if let Some([left, top, w, h]) = control.bounds {
                let (cx, cy) = (left + w / 2.0, top + h / 2.0);
                let longest = w.max(h).max(0.0001);
                let factor = (size / longest).clamp(0.05, 20.0);
                let (nw, nh) = (w * factor, h * factor);
                control.bounds = Some([cx - nw / 2.0, cy - nh / 2.0, nw, nh]);
            }
        }
        self.layout.clamp();
    }

    /// Give a button an opacity of its own, away from the layout's.
    pub fn set_control_opacity(&mut self, action: Action, opacity: f32) {
        if let Some(control) = self
            .layout
            .controls
            .iter_mut()
            .find(|control| control.action == action)
        {
            control.opacity = Some(opacity);
        }
        self.layout.clamp();
    }

    /// The buttons held right now, in press order.
    pub fn pressed_actions(&self) -> Vec<Action> {
        self.held.iter().copied().collect()
    }

    /// How many fingers are down, which is what the diagnostic line reports.
    pub fn fingers(&self) -> usize {
        self.fingers.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The screen of the phone this was written for, in the landscape the game
    /// asks the activity for.
    const WIDTH: f32 = 1280.0;
    const HEIGHT: f32 = 720.0;

    fn hud() -> Hud {
        Hud::default()
    }

    #[test]
    fn a_finger_on_a_button_presses_it_and_lets_go() {
        let mut hud = hud();
        let (jump, rect) = hud
            .buttons(WIDTH, HEIGHT)
            .into_iter()
            .find(|(action, _)| *action == Action::Jump)
            .expect("the default layout has a jump button");
        assert_eq!(jump, Action::Jump);
        let [x, y] = rect.center();
        assert!(
            hud.down(1, x, y, WIDTH, HEIGHT),
            "a finger on the jump button is the layout's, not the screen behind it"
        );
        assert!(hud.is_held(Action::Jump));
        assert_eq!(hud.take_edges(), vec![(Action::Jump, true)]);
        hud.up(1);
        assert!(!hud.is_held(Action::Jump));
        assert_eq!(hud.take_edges(), vec![(Action::Jump, false)]);
    }

    #[test]
    fn a_tap_on_a_toggle_fires_once_and_leaves_the_button() {
        let mut hud = hud();
        let rect = hud
            .buttons(WIDTH, HEIGHT)
            .into_iter()
            .find(|(action, _)| *action == Action::Inventory)
            .map(|(_, rect)| rect)
            .expect("the default layout has an inventory button");
        let [x, y] = rect.center();
        hud.down(1, x, y, WIDTH, HEIGHT);
        hud.up(1);
        assert_eq!(hud.take_edges(), vec![(Action::Inventory, true), (Action::Inventory, false)]);
        assert!(!hud.is_held(Action::Inventory));
    }

    #[test]
    fn two_fingers_press_two_buttons_at_once() {
        let mut hud = hud();
        let buttons = hud.buttons(WIDTH, HEIGHT);
        let forward = buttons.iter().find(|(a, _)| *a == Action::Forward).unwrap().1;
        let mine = buttons.iter().find(|(a, _)| *a == Action::Mine).unwrap().1;
        hud.down(1, forward.x + 1.0, forward.y + 1.0, WIDTH, HEIGHT);
        hud.down(2, mine.x + 1.0, mine.y + 1.0, WIDTH, HEIGHT);
        assert!(hud.is_held(Action::Forward) && hud.is_held(Action::Mine));
        // An imgui widget driven by a mouse would have taken the second finger for
        // a click on whatever the first finger is already holding.
        assert_eq!(hud.fingers(), 2);
        hud.up(1);
        assert!(hud.is_held(Action::Mine) && !hud.is_held(Action::Forward));
    }

    #[test]
    fn a_drag_off_the_button_keeps_pressing_it() {
        let mut hud = hud();
        let rect = hud
            .buttons(WIDTH, HEIGHT)
            .into_iter()
            .find(|(action, _)| *action == Action::Forward)
            .map(|(_, rect)| rect)
            .unwrap();
        hud.down(1, rect.x + 1.0, rect.y + 1.0, WIDTH, HEIGHT);
        hud.drag(1, rect.x + 4000.0, rect.y + 4000.0, WIDTH, HEIGHT);
        assert!(hud.is_held(Action::Forward));
        assert_eq!(hud.take_look(), [0.0, 0.0]);
        hud.up(1);
        assert!(!hud.is_held(Action::Forward));
    }

    #[test]
    fn a_drag_on_empty_glass_turns_the_camera() {
        let mut hud = hud();
        // The middle of the screen: nothing is there in the default layout.
        assert!(
            !hud.down(1, WIDTH * 0.5, HEIGHT * 0.5, WIDTH, HEIGHT),
            "a finger on bare glass belongs to the game"
        );
        assert_eq!(hud.pressed_actions(), vec![]);
        hud.drag(1, WIDTH * 0.5 + 30.0, HEIGHT * 0.5 + 12.0, WIDTH, HEIGHT);
        hud.up(1);
        let [yaw, pitch] = hud.take_look();
        assert!(
            yaw > pitch && yaw > 0.0 && pitch > 0.0,
            "a drag right and down should turn right and pitch down, got {yaw} {pitch}"
        );
        let expected = 30.0 / hud.layout.look_sensitivity;
        assert!((yaw - expected).abs() < 0.01, "{yaw} should be {expected} degrees");
        assert_eq!(hud.take_look(), [0.0, 0.0], "the turn is taken once");
    }

    #[test]
    fn editing_the_layout_moves_buttons_and_sends_nothing() {
        let mut hud = hud();
        hud.edit = true;
        let jump = |hud: &Hud| {
            hud.buttons(WIDTH, HEIGHT)
                .into_iter()
                .find(|(action, _)| *action == Action::Jump)
                .map(|(_, rect)| rect.center())
                .unwrap()
        };
        let before = jump(&hud);
        hud.down(1, before[0], before[1], WIDTH, HEIGHT);
        assert_eq!(hud.grabbed(), Some(Action::Jump));
        assert_eq!(hud.take_edges(), vec![], "picking a button up is not pressing it");
        hud.drag(1, 100.0, 100.0, WIDTH, HEIGHT);
        hud.up(1);
        assert_eq!(hud.take_edges(), vec![], "letting go of a button is not pressing it");
        assert_eq!(hud.grabbed(), None);
        let after = jump(&hud);
        assert!(
            (after[0] - 100.0).abs() < 1.0 && (after[1] - 100.0).abs() < 1.0,
            "the button should have followed the finger from {before:?} to [100,100], got {after:?}"
        );
        assert_eq!(hud.take_look(), [0.0, 0.0], "editing never turns the camera");
    }

    #[test]
    fn the_menu_button_answers_even_while_the_layout_is_being_arranged() {
        let mut hud = hud();
        hud.edit = true;
        let menu = hud
            .buttons(WIDTH, HEIGHT)
            .into_iter()
            .find(|(action, _)| *action == Action::Menu)
            .map(|(_, rect)| rect.center())
            .unwrap();
        assert!(hud.down(1, menu[0], menu[1], WIDTH, HEIGHT));
        hud.up(1);
        assert_eq!(
            hud.take_edges(),
            vec![(Action::Menu, true), (Action::Menu, false)],
            "the panel is the only door back to the switch that ends the arranging"
        );
    }

    #[test]
    fn a_layout_survives_a_round_trip_through_disk() {
        let dir = std::env::temp_dir().join("horizon-controls-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("controls.json");
        let mut layout = Layout::default();
        layout.scale = 1.4;
        layout.opacity = 0.2;
        layout.look_sensitivity = 9.5;
        layout.controls[0].center = [0.12, 0.7];
        layout.save(&path).unwrap();
        let read = Layout::load(&path).expect("the file we just wrote reads back");
        assert_eq!(read.scale, 1.4);
        assert_eq!(read.opacity, 0.2);
        assert_eq!(read.look_sensitivity, 9.5);
        assert_eq!(read.controls.len(), layout.controls.len());
        assert_eq!(read.controls[0].center, [0.12, 0.7]);
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn nonsense_in_a_layout_file_is_clamped_not_trusted() {
        let dir = std::env::temp_dir().join("horizon-controls-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bad.json");
        std::fs::write(
            &path,
            r#"{"scale":99.0,"opacity":-3.0,"look_sensitivity":0.0,"drag_to_look":true,
               "controls":[{"action":"Jump","center":[7.0,-2.0],"size":0.0}]}"#,
        )
        .unwrap();
        let layout = Layout::load(&path).expect("one button is a layout");
        assert_eq!(layout.scale, 2.0);
        assert_eq!(layout.opacity, 0.05);
        assert_eq!(layout.look_sensitivity, 0.5);
        assert_eq!(layout.controls[0].center, [0.98, 0.02]);
        assert_eq!(layout.controls[0].size, 0.03);
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn an_empty_or_unreadable_file_is_no_layout_at_all() {
        let dir = std::env::temp_dir().join("horizon-controls-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("empty.json");
        std::fs::write(&path, r#"{"scale":1.0,"opacity":0.5,"look_sensitivity":6.0,
             "drag_to_look":true,"controls":[]}"#)
            .unwrap();
        assert!(Layout::load(&path).is_none());
        assert!(Layout::load(&dir.join("missing.json")).is_none());
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn every_button_of_the_default_layout_is_reachable_and_labeled() {
        let layout = Layout::default();
        let buttons = layout.buttons(WIDTH, HEIGHT);
        assert_eq!(buttons.len(), layout.controls.len(), "nothing off the screen");
        let mut actions: Vec<Action> = buttons.iter().map(|(action, _)| *action).collect();
        actions.sort_unstable();
        actions.dedup();
        assert_eq!(actions.len(), layout.controls.len(), "no button twice");
        for (action, rect) in &buttons {
            assert!(rect.width > 24.0, "{action:?} is too small to press: {rect:?}");
            assert!(
                rect.x >= 0.0 && rect.y >= 0.0 && rect.x + rect.width <= WIDTH + 0.01,
                "{action:?} hangs off the screen: {rect:?}"
            );
            assert!(rect.y + rect.height <= HEIGHT + 0.01, "{action:?}: {rect:?}");
            assert!(!action.text().is_empty(), "{action:?} has no legend");
        }
        // The middle of every button has to answer to that button. Two rows in the
        // same band pass every check above and still steal each other's taps, and on a
        // phone there is no right-click to find out which one a player pressed.
        let front = layout.front_to_back(WIDTH, HEIGHT);
        for (action, rect) in &buttons {
            let [x, y] = rect.center();
            let hit = front
                .iter()
                .find(|(_, other)| other.contains(x, y))
                .map(|(other, _)| *other);
            assert_eq!(hit, Some(*action), "{action:?} loses its own middle at {x:.0}x{y:.0}");
        }
        for slot in 1..=9u8 {
            assert!(
                buttons.iter().any(|(action, _)| *action == Action::Hotbar(slot)),
                "hotbar slot {slot} is missing"
            );
        }
    }

    #[test]
    fn a_shaped_button_is_the_rectangle_the_file_asked_for() {
        let layout = Layout {
            scale: 1.0,
            opacity: 0.5,
            look_sensitivity: 6.0,
            drag_to_look: true,
            preset: "bedrock".into(),
            controls: vec![
                // A square in pixels: the same fraction of the short edge in both
                // axes is what a round button has to be, and the fractions differ
                // because the screen's two edges do.
                shaped(
                    Action::Jump,
                    [0.80, 0.20, 0.10 * 9.0 / 16.0, 0.10],
                    Some("jump"),
                    true,
                ),
                // A wide tab: the shape a square cannot be.
                shaped(Action::Chat, [0.10, 0.02, 0.20, 0.05], Some("chat"), false),
            ],
        };
        let buttons = layout.buttons(WIDTH, HEIGHT);
        let jump = buttons
            .iter()
            .find(|(action, _)| *action == Action::Jump)
            .map(|(_, rect)| *rect)
            .unwrap();
        assert!((jump.width - jump.height).abs() < 0.01, "{jump:?}");
        assert!((jump.x - 0.80 * WIDTH).abs() < 0.01, "{jump:?}");
        assert!((jump.y - 0.20 * HEIGHT).abs() < 0.01, "{jump:?}");
        let chat = buttons
            .iter()
            .find(|(action, _)| *action == Action::Chat)
            .map(|(_, rect)| *rect)
            .unwrap();
        // Four times as wide as it is tall in pixels, which is the whole point of a
        // tab. The bounds are fractions of two different edges, so the ratio has to
        // be taken after the conversion, not before it.
        assert!((chat.width - 0.20 * WIDTH).abs() < 0.01, "{chat:?}");
        assert!((chat.height - 0.05 * HEIGHT).abs() < 0.01, "{chat:?}");
        assert!((chat.width / chat.height - 7.111).abs() < 0.05, "{chat:?}");
    }

    #[test]
    fn scaling_a_shaped_button_keeps_its_proportions_and_its_middle() {
        let mut hud = Hud::new(Layout {
            scale: 1.0,
            opacity: 0.5,
            look_sensitivity: 6.0,
            drag_to_look: true,
            preset: "bedrock".into(),
            controls: vec![shaped(
                Action::Chat,
                [0.10, 0.10, 0.20, 0.05],
                Some("chat"),
                false,
            )],
        });
        let before = hud.layout.buttons(WIDTH, HEIGHT)[0].1;
        hud.set_control_size(Action::Chat, 0.30);
        let after = hud.layout.buttons(WIDTH, HEIGHT)[0].1;
        // The tab keeps the proportions it was drawn with: the slider changes how big
        // it is, never what shape it is.
        assert!(
            (after.width / after.height - before.width / before.height).abs() < 0.05,
            "{before:?} -> {after:?}"
        );
        // And it grows about its own middle, so it stays where the player put it.
        assert!((after.center()[0] - before.center()[0]).abs() < 0.5, "{after:?}");
        assert!((after.center()[1] - before.center()[1]).abs() < 0.5, "{after:?}");
        assert!(after.width > before.width, "{before:?} -> {after:?}");
    }

    #[test]
    fn a_layout_written_before_shapes_still_loads() {
        // The old file has no `bounds`, `icon` or `round`: it is centre-and-size only,
        // and it must keep working rather than loading as an empty layout.
        let dir = std::env::temp_dir().join("horizon-controls-legacy");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("legacy.json");
        std::fs::write(
            &path,
            r#"{"scale":1.0,"opacity":0.4,"look_sensitivity":6.0,"drag_to_look":true,
               "controls":[{"action":"Jump","center":[0.9,0.7],"size":0.12}]}"#,
        )
        .unwrap();
        let layout = Layout::load(&path).expect("an old layout is still a layout");
        assert_eq!(layout.controls.len(), 1);
        assert!(layout.controls[0].bounds.is_none());
        assert!(layout.controls[0].icon.is_none());
        assert!(!layout.controls[0].round);
        let rect = layout.buttons(WIDTH, HEIGHT)[0].1;
        assert!(rect.width > 0.0 && rect.contains(rect.center()[0], rect.center()[1]));
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn the_layout_reads_the_same_in_both_orientations() {
        let layout = Layout::default();
        let portrait = layout.buttons(720.0, 1280.0);
        assert_eq!(portrait.len(), layout.controls.len(), "rotating loses buttons");
        let jump = layout
            .buttons(720.0, 1280.0)
            .into_iter()
            .find(|(action, _)| *action == Action::Jump)
            .map(|(_, rect)| rect)
            .unwrap();
        // A thumb on a 720-wide screen still has something 68 pixels across to press.
        assert!(jump.width > 60.0, "{jump:?}");
    }

    #[test]
    fn releasing_every_finger_lets_every_button_go() {
        let mut hud = hud();
        let held: Vec<(Action, Rect)> = hud
            .buttons(WIDTH, HEIGHT)
            .into_iter()
            .filter(|(action, _)| !action.momentary())
            .take(4)
            .collect();
        assert_eq!(held.len(), 4, "the default layout has four buttons to hold");
        for (index, (action, rect)) in held.iter().enumerate() {
            let [x, y] = rect.center();
            hud.down(index as u64, x, y, WIDTH, HEIGHT);
            assert!(hud.is_held(*action), "{action:?}");
        }
        // The presses themselves are the game's business and it has taken them; what
        // is left after the window goes away is the question.
        assert_eq!(hud.take_edges().len(), 4, "each finger pressed once");
        hud.release_all();
        assert_eq!(hud.pressed_actions(), vec![]);
        assert_eq!(hud.fingers(), 0);
        let edges = hud.take_edges();
        assert_eq!(edges.len(), 4, "every held button was let go");
        assert!(
            edges.iter().all(|(_, down)| !down),
            "everything released, nothing pressed"
        );
    }

    #[test]
    fn a_button_can_have_its_own_size_and_opacity() {
        let mut hud = hud();
        hud.set_selected(Some(Action::Jump));
        assert_eq!(hud.selected(), Some(Action::Jump));
        assert_eq!(
            hud.control_opacity(Action::Jump),
            Some(hud.layout.opacity),
            "with none of its own a button follows the layout"
        );
        hud.set_control_opacity(Action::Jump, 0.9);
        assert_eq!(hud.layout.opacity_of(Action::Jump), 0.9);
        assert_eq!(
            hud.layout.opacity_of(Action::Mine),
            hud.layout.opacity,
            "an untouched button still follows the layout"
        );
    }

    #[test]
    fn a_button_size_edit_is_clamped_like_every_other() {
        let mut hud = hud();
        hud.set_control_size(Action::Jump, 9.0);
        assert_eq!(hud.control_size(Action::Jump), Some(0.4));
        hud.set_control_size(Action::Jump, 0.0);
        assert_eq!(hud.control_size(Action::Jump), Some(0.03));
    }

    #[test]
    fn tapping_a_button_while_editing_selects_it_and_glass_deselects() {
        let mut hud = hud();
        hud.edit = true;
        let jump = hud
            .buttons(WIDTH, HEIGHT)
            .into_iter()
            .find(|(action, _)| *action == Action::Jump)
            .map(|(_, rect)| rect.center())
            .unwrap();
        assert_eq!(hud.selected(), None);
        hud.down(1, jump[0], jump[1], WIDTH, HEIGHT);
        assert_eq!(hud.selected(), Some(Action::Jump));
        hud.up(1);
        hud.down(2, WIDTH * 0.5, HEIGHT * 0.5, WIDTH, HEIGHT);
        assert_eq!(hud.selected(), None, "bare glass takes the selection away");
    }

    #[test]
    fn a_button_opacity_survives_the_round_trip_and_absent_means_the_layout() {
        let dir = std::env::temp_dir().join("horizon-controls-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("per-button.json");
        let mut layout = Layout::default();
        layout.controls[0].opacity = Some(0.8);
        layout.save(&path).unwrap();
        let read = Layout::load(&path).expect("the file we just wrote reads back");
        assert_eq!(read.controls[0].opacity, Some(0.8));
        assert_eq!(read.controls[1].opacity, None);
        std::fs::remove_file(&path).unwrap();
    }
}
