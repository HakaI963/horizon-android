//! What the screen asks the game to do: the button layout, and which fingers are
//! pressing what.
//!
//! A phone has no keyboard and one pointer per finger, and the game is written
//! against a keyboard and a mouse that is captured in the middle of the screen.
//! Everything between those two shapes is here: where a button is, what pressing it
//! means, how a drag on the empty glass becomes a camera turn, and nothing of the
//! engine's. Keeping the model free of imgui is what lets a layout be tested against
//! a tap without a renderer.
//!
//! The layout itself is not invented here. It is read from the file format control
//! pads have used for years — formulas over the screen's own numbers, sizes in dp,
//! Android key codes — so a pad a player already has opens as it is.

pub mod expr;
pub mod keys;
pub mod layout;

use std::collections::BTreeSet;
use std::path::Path;

pub use layout::{
    action_for_key, key_for_action, Action, ControlData, LayoutFile, SPECIAL_KEYBOARD,
    SPECIAL_MENU, SPECIAL_MOUSE_MIDDLE, SPECIAL_MOUSE_PRIMARY, SPECIAL_MOUSE_SECONDARY,
    SPECIAL_SCROLL_DOWN, SPECIAL_SCROLL_UP, SPECIAL_TOGGLE_CONTROLS, SPECIAL_VIRTUAL_MOUSE,
};

/// A rectangle on the glass, in pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
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

    pub fn center(self) -> [f32; 2] {
        [self.x + self.width / 2.0, self.y + self.height / 2.0]
    }
}

/// A button of the layout, placed on a particular screen.
#[derive(Clone, Debug)]
pub struct Placed {
    pub action: Action,
    pub rect: Rect,
    /// Which entry of the layout file it came from, for its colours and its corner.
    pub index: usize,
}

/// The pad: the file, and the few numbers the player can change without editing it.
#[derive(Clone, Debug)]
pub struct Layout {
    pub file: LayoutFile,
    /// Every button's size, multiplied by this.
    pub scale: f32,
    /// How solid the buttons are drawn. A HUD that hides the world is worse than no
    /// HUD.
    pub opacity: f32,
    /// Pixels of glass per degree of camera turn.
    pub look_sensitivity: f32,
    /// Whether a drag on empty glass turns the camera at all.
    pub drag_to_look: bool,
    /// The density the layout is drawn at: dp to pixels. A phone's own, so a button
    /// that is 50 dp is the same physical size on every screen.
    pub density: f32,
}

impl Default for Layout {
    fn default() -> Self {
        Self::new(LayoutFile::default_pad())
    }
}

impl Layout {
    /// The pad the build ships with, for a platform that has never saved one.
    pub fn default_pad() -> Self {
        Self::new(LayoutFile::default_pad())
    }

    pub fn new(file: LayoutFile) -> Self {
        Self {
            file,
            scale: 1.0,
            opacity: 0.85,
            look_sensitivity: 6.0,
            drag_to_look: true,
            density: 1.0,
        }
    }

    /// Read the pad from disk, or the compiled-in default when there is none.
    ///
    /// A file written by an older build is not read. It has the same name and a
    /// different shape — a private model with 33 hand-placed buttons and none of the
    /// format's fields — and upgrading it would put that pad back on the glass, which
    /// is exactly what this launcher stopped shipping.
    pub fn load(path: &Path) -> Self {
        // The pad is the build's, not a file left on the device by an older build of
        // it. A saved layout is read only when the player arranged it themselves, which
        // is what `edited` records; a file without that mark is a default pad from a
        // build that no longer exists, and reading it is how a pad with a Debug button
        // and glyph names kept coming back after every install.
        let file = LayoutFile::load(path).filter(LayoutFile::is_current);
        match &file {
            Some(file) if file.edited => log::info!(
                "controls: reading the player's layout at {} ({} buttons)",
                path.display(),
                file.controls.len()
            ),
            _ => log::info!(
                "controls: using the build's pad; {} is not a layout the player made",
                path.display()
            ),
        }
        Self::new(file.filter(|file| file.edited).unwrap_or_else(LayoutFile::default_pad))
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let mut file = self.file.clone();
        // Saved from the arrange mode, so this is the player's pad from now on and is
        // read back on the next start.
        file.edited = true;
        file.save(path)
    }

    /// Where every button is on a `width`x`height` screen.
    pub fn buttons(&self, width: f32, height: f32) -> Vec<Placed> {
        self.file
            .controls
            .iter()
            .enumerate()
            .map(|(index, control)| {
                let [x, y] = control.position(width, height, self.density);
                let w = control.px_width(self.density) * self.scale;
                let h = control.px_height(self.density) * self.scale;
                // Scaling about the middle, so a button keeps the place it was put at
                // while it grows.
                let cx = x + control.px_width(self.density) / 2.0;
                let cy = y + control.px_height(self.density) / 2.0;
                let action = control.action();
                Placed {
                    action,
                    rect: Rect {
                        x: cx - w / 2.0,
                        y: cy - h / 2.0,
                        width: w,
                        height: h,
                    },
                    index,
                }
            })
            .collect()
    }

    /// The button a point lands on, topmost first. Later entries are drawn over
    /// earlier ones, so they win the tap.
    pub fn hit(&self, x: f32, y: f32, width: f32, height: f32) -> Option<Placed> {
        self.buttons(width, height)
            .into_iter()
            .rev()
            .find(|placed| placed.rect.contains(x, y))
    }

    pub fn control(&self, index: usize) -> Option<&ControlData> {
        self.file.controls.get(index)
    }

    /// The label a button draws, from its own name in the file.
    pub fn label(&self, index: usize) -> &str {
        self.file
            .controls
            .get(index)
            .map(|control| control.name.as_str())
            .unwrap_or("")
    }
}

/// A finger on the glass.
#[derive(Clone, Debug)]
struct Finger {
    id: u64,
    /// The button it landed on, if any.
    control: Option<Action>,
    /// Which entry of the file that button was.
    index: Option<usize>,
    /// Whether it is turning the camera.
    looking: bool,
    /// Whether it is dragging the virtual mouse's cursor.
    pointing: bool,
    /// Whether the button it holds is a toggle that this press turned on.
    toggled: bool,
    /// Whether it has left the button it started on. A finger that slides off a
    /// held button releases it, the way a physical button does.
    slipped: bool,
}

/// The pad, and every finger on it.
pub struct Hud {
    pub layout: Layout,
    /// The layout is on screen as live widgets, so a finger can drag it. While this
    /// is set nothing is sent to the game.
    pub edit: bool,
    /// The button the editor is pointed at.
    selected: Option<usize>,
    fingers: Vec<Finger>,
    held: BTreeSet<Action>,
    /// Edges to hand to the engine: `true` is a press, `false` a release. They
    /// outlive the frame that made them, because the engine reads them when it next
    /// ticks and a tap can begin and end inside one frame at 120 fps.
    edges: Vec<(Action, bool)>,
    look: [f32; 2],
    /// The pad is on the glass. The GUI button flips this, and while it is off every
    /// button goes except the two that can bring it back: the GUI button itself and
    /// the virtual mouse. That exception is the format's own rule, and without it the
    /// button that hides the pad would hide its own way back.
    pub visible: bool,
    /// The GUI scale in force, as the platform last drew a HUD. The hotbar strip is
    /// placed from it.
    gui_scale: f32,
    /// The virtual mouse is out.
    pub mouse: bool,
    cursor: [f32; 2],
    cursor_moved: [f32; 2],
    cursor_placed: bool,
    /// How far the cursor travels per pixel of finger, and how large it is drawn. Both
    /// are the player's: a thumb that sweeps the glass and a cursor that crosses the
    /// screen in one flick is a mouse that cannot be aimed.
    pub mouse_speed: f32,
    pub mouse_scale: f32,
    /// The hotbar slot the last touch on the strip selected, so sliding along it
    /// sends one key per slot crossed rather than one per frame.
    hotbar_slot: Option<u8>,
    /// When the strip was last tapped, and on which slot: a double tap swaps hands.
    hotbar_tap: Option<(u8, std::time::Instant)>,
    /// Where each looking finger was last frame, so a drag is the movement since
    /// then rather than the whole sweep from where it landed.
    positions: Vec<(u64, f32, f32)>,
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
            visible: true,
            gui_scale: 0.0,
            mouse: false,
            cursor: [0.0, 0.0],
            cursor_moved: [0.0, 0.0],
            cursor_placed: false,
            mouse_speed: 1.0,
            mouse_scale: 1.0,
            hotbar_slot: None,
            hotbar_tap: None,
            positions: Vec::new(),
        }
    }

    pub fn load(path: &Path) -> Self {
        Self::new(Layout::load(path))
    }

    pub fn save(&self, path: &Path) {
        if let Err(error) = self.layout.save(path) {
            log::warn!("controls: could not save {}: {error}", path.display());
        }
    }

    /// Tell the layout how many glass pixels one GUI pixel is worth this frame. The
    /// hotbar strip is placed from it, so the taps land on the slots the game drew.
    pub fn set_gui_scale(&mut self, scale: f32) {
        self.gui_scale = scale.max(0.0);
    }

    /// Where the game draws its hotbar, in glass pixels.
    ///
    /// The numbers are the game's own: a 182x22 bar centred at the bottom, one
    /// 20-pixel slot per item, all in GUI pixels and therefore multiplied by the
    /// scale. This is why the strip follows the GUI scale and the window rather than
    /// a fixed fraction of the screen.
    pub fn hotbar_rect(&self, width: f32, height: f32) -> Option<Rect> {
        if self.gui_scale <= 0.0 {
            return None;
        }
        let (gw, gh) = (width / self.gui_scale, height / self.gui_scale);
        let left = (gw / 2.0).floor() - 91.0;
        let top = gh - 22.0;
        Some(Rect {
            x: left * self.gui_scale,
            y: (top - 1.0) * self.gui_scale,
            width: 182.0 * self.gui_scale,
            height: 24.0 * self.gui_scale,
        })
    }

    /// A finger went down. Returns whether the pad took it: `false` means it is the
    /// game's, and a drag on it turns the camera.
    pub fn down(&mut self, id: u64, x: f32, y: f32, width: f32, height: f32) -> bool {
        // The hotbar strip first: it sits over the game's own bar, and it is the one
        // place a tap means a slot rather than a key.
        if let Some(strip) = self.hotbar_rect(width, height)
            && strip.contains(x, y)
            && !self.edit
        {
            let slot = hotbar_slot(strip, x);
            self.hotbar_slot = Some(slot);
            self.press(Action::Hotbar(slot));
            // A second tap on the same slot swaps hands, which is what a player
            // expects from a hotbar they can touch.
            let now = std::time::Instant::now();
            if let Some((last, at)) = self.hotbar_tap
                && last == slot
                && now.duration_since(at) < std::time::Duration::from_millis(300)
            {
                self.press(Action::SwapOffhand);
                self.hotbar_tap = None;
            } else {
                self.hotbar_tap = Some((slot, now));
            }
            self.fingers.push(Finger {
                id,
                control: None,
                index: None,
                looking: false,
                pointing: false,
                toggled: false,
                slipped: false,
            });
            return true;
        }

        let hit = self.layout.hit(x, y, width, height);
        if self.edit {
            // While arranging, a finger picks its button up instead of pressing it:
            // editing a HUD must not mine a block.
            if let Some(placed) = hit {
                self.selected = Some(placed.index);
                self.fingers.push(Finger {
                    id,
                    control: Some(placed.action),
                    index: Some(placed.index),
                    looking: false,
                    pointing: false,
                    toggled: false,
                    slipped: false,
                });
                return true;
            }
            self.fingers.push(Finger {
                id,
                control: None,
                index: None,
                looking: false,
                pointing: false,
                toggled: false,
                slipped: false,
            });
            return false;
        }
        if let Some(placed) = hit {
            let toggle = self
                .layout
                .control(placed.index)
                .is_some_and(|control| control.is_toggle);
            // A toggle flips on the press and stays flipped: the finger lifts, the key
            // does not. That is what makes sneak and sprint usable with a thumb.
            if toggle {
                if self.held.contains(&placed.action) {
                    self.release(placed.action);
                } else {
                    self.press(placed.action);
                }
            } else {
                self.press(placed.action);
            }
            self.fingers.push(Finger {
                id,
                control: Some(placed.action),
                index: Some(placed.index),
                looking: false,
                pointing: false,
                toggled: toggle,
                slipped: false,
            });
            return true;
        }
        // Bare glass: the cursor when the mouse is out, the camera otherwise. The
        // cursor does not jump to the finger — a touchpad drag moves the pointer by the
        // distance the finger went, which is the whole difference between a mouse and a
        // finger pressed on the thing it wants to click.
        let pointing = self.mouse;
        if pointing {
            if !self.cursor_placed {
                self.cursor = [x, y];
                self.cursor_placed = true;
            }
            self.remember_position(id, x, y);
        }
        let looking = !pointing && self.layout.drag_to_look;
        if looking {
            // Where the drag starts. Without it the first move has nothing to be a
            // movement from, and a drag would only turn the camera from its second
            // frame onwards.
            self.remember_position(id, x, y);
        }
        self.fingers.push(Finger {
            id,
            control: None,
            index: None,
            looking,
            pointing,
            toggled: false,
            slipped: false,
        });
        false
    }

    /// A finger moved.
    pub fn drag(&mut self, id: u64, x: f32, y: f32, width: f32, height: f32) {
        // Sliding along the hotbar strip sweeps the slots, one key per slot crossed.
        if let Some(strip) = self.hotbar_rect(width, height)
            && strip.contains(x, y)
            && self.hotbar_slot.is_some()
            && !self.edit
        {
            let slot = hotbar_slot(strip, x);
            if self.hotbar_slot != Some(slot) {
                self.hotbar_slot = Some(slot);
                self.press(Action::Hotbar(slot));
            }
            return;
        }
        let Some(index) = self.fingers.iter().position(|finger| finger.id == id) else {
            return;
        };
        let (looking, pointing) = {
            let finger = &self.fingers[index];
            (finger.looking, finger.pointing)
        };
        if self.edit {
            // Dragging the button the editor has selected moves it, and the move is
            // written back as a formula so it survives a different screen.
            if let Some(button) = self.selected {
                self.move_button(button, x, y, width, height);
            }
            return;
        }
        if pointing {
            // Relative: the movement since the last frame, times the player's own speed.
            // The cursor is clamped to the glass so it cannot be lost off an edge, and
            // the delta the game sees is the clamped movement, not the finger's.
            let last = self.last_position(id).unwrap_or((x, y));
            let speed = self.mouse_speed.max(0.05);
            let (mx, my) = ((x - last.0) * speed, (y - last.1) * speed);
            let before = self.cursor;
            self.cursor[0] = (self.cursor[0] + mx).clamp(0.0, width.max(1.0));
            self.cursor[1] = (self.cursor[1] + my).clamp(0.0, height.max(1.0));
            self.cursor_moved[0] += self.cursor[0] - before[0];
            self.cursor_moved[1] += self.cursor[1] - before[1];
            self.remember_position(id, x, y);
        }
        if looking {
            // The movement since the last frame, not the sweep from where the finger
            // landed: a drag that jumped the whole distance at once would turn the
            // camera by the distance travelled rather than by the speed.
            let last = self.last_position(id).unwrap_or((x, y));
            let (mx, my) = (x - last.0, y - last.1);
            let sensitivity = self.layout.look_sensitivity.max(0.01);
            self.look[0] += mx / sensitivity;
            self.look[1] += my / sensitivity;
            self.remember_position(id, x, y);
        }
        // A finger that leaves the button it was holding lets go of it, unless the
        // button is one that is meant to be swiped across.
        let control = self.fingers[index].control;
        let slipped = self.fingers[index].slipped;
        if let Some(action) = control
            && !slipped
            && let Some(placed) = self
                .layout
                .buttons(width, height)
                .into_iter()
                .find(|placed| placed.action == action)
            && !placed.rect.contains(x, y)
            && !self
                .layout
                .control(placed.index)
                .is_some_and(|control| control.is_swipeable)
        {
            // A hold releases; a toggle keeps the state the press gave it, because
            // that is the point of one.
            let toggled = self.fingers[index].toggled;
            self.fingers[index].slipped = true;
            if !toggled {
                self.release(action);
            }
        }
    }

    /// A finger lifted.
    pub fn up(&mut self, id: u64) {
        let Some(index) = self.fingers.iter().position(|finger| finger.id == id) else {
            return;
        };
        let finger = self.fingers.remove(index);
        if let Some(action) = finger.control
            && !finger.toggled
            && !finger.slipped
        {
            self.release(action);
        }
        if finger.looking {
            self.forget_position(id);
        }
        if self.fingers.is_empty() {
            self.hotbar_slot = None;
        }
    }

    /// Nothing is held any more: a surface going away sends no lifts.
    pub fn release_all(&mut self) {
        for action in std::mem::take(&mut self.held) {
            self.edges.push((action, false));
        }
        self.fingers.clear();
        self.hotbar_slot = None;
        self.hotbar_tap = None;
    }

    fn press(&mut self, action: Action) {
        if self.held.insert(action) {
            self.edges.push((action, true));
        }
    }

    fn release(&mut self, action: Action) {
        if self.held.remove(&action) {
            self.edges.push((action, false));
        }
    }

    /// The edges since the last call.
    pub fn take_edges(&mut self) -> Vec<(Action, bool)> {
        std::mem::take(&mut self.edges)
    }

    /// Degrees the camera has been asked to turn since the last call.
    pub fn take_look(&mut self) -> [f32; 2] {
        std::mem::replace(&mut self.look, [0.0, 0.0])
    }

    pub fn cursor(&self) -> [f32; 2] {
        self.cursor
    }

    /// Pixels the cursor has moved since the last call.
    pub fn take_cursor(&mut self) -> [f32; 2] {
        std::mem::replace(&mut self.cursor_moved, [0.0, 0.0])
    }

    /// Turn the virtual mouse on or off, and say which it is now.
    pub fn set_mouse(&mut self, on: bool, width: f32, height: f32) -> bool {
        if on && !self.mouse {
            if !self.cursor_placed {
                self.cursor = [width / 2.0, height / 2.0];
                self.cursor_placed = true;
            }
            self.mouse = true;
        } else if !on {
            self.mouse = false;
        }
        self.mouse
    }

    pub fn place_cursor(&mut self, x: f32, y: f32) {
        self.cursor_moved[0] += x - self.cursor[0];
        self.cursor_moved[1] += y - self.cursor[1];
        self.cursor = [x, y];
        self.cursor_placed = true;
    }

    /// Where every button is, for drawing. A hidden pad keeps the two buttons that
    /// can bring it back, which is the format's own rule.
    pub fn buttons(&self, width: f32, height: f32) -> Vec<Placed> {
        let mut buttons = self.layout.buttons(width, height);
        if !self.visible {
            buttons.retain(|placed| keeps_its_place_when_hidden(placed.action));
        }
        buttons
    }

    /// Whether the pad is showing, for the panel's own switch.
    pub fn visible(&self) -> bool {
        self.visible
    }

    pub fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }

    /// Show or hide the pad, and say which it is now.
    pub fn toggle_visible(&mut self) -> bool {
        self.visible = !self.visible;
        // A button that is no longer on the glass must not stay held: the finger on it
        // has nowhere to lift from.
        if !self.visible {
            let keep: BTreeSet<Action> = self
                .held
                .iter()
                .copied()
                .filter(|action| keeps_its_place_when_hidden(*action))
                .collect();
            for action in std::mem::replace(&mut self.held, keep.clone()) {
                if !keep.contains(&action) {
                    self.edges.push((action, false));
                }
            }
        }
        self.visible
    }

    pub fn grabbed(&self) -> Option<usize> {
        self.selected
    }

    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    pub fn set_selected(&mut self, index: Option<usize>) {
        self.selected = index;
    }

    /// Move a button to where the finger has it, written back as a formula.
    ///
    /// The formula is anchored the way the format's own editor anchors it: a button
    /// past the middle of the screen is measured from the far edge, so it stays on
    /// that edge when the screen changes, and one before it is measured from the
    /// near edge.
    pub fn move_button(&mut self, index: usize, x: f32, y: f32, width: f32, height: f32) {
        let Some(control) = self.layout.file.controls.get_mut(index) else {
            return;
        };
        let (w, h) = (
            control.px_width(self.layout.density),
            control.px_height(self.layout.density),
        );
        let left = (x - w / 2.0).clamp(0.0, (width - w).max(0.0));
        let top = (y - h / 2.0).clamp(0.0, (height - h).max(0.0));
        control.dynamic_x = anchor_x(left, width);
        control.dynamic_y = anchor_y(top, height);
    }

    pub fn control_opacity(&self, index: usize) -> Option<f32> {
        self.layout.control(index).map(|control| control.opacity)
    }

    /// Change one button's own opacity. The layout's slider multiplies this, so a
    /// button set to half stays half of whatever the pad is set to.
    pub fn set_control_opacity(&mut self, index: usize, opacity: f32) {
        if let Some(control) = self.layout.file.controls.get_mut(index) {
            control.opacity = opacity.clamp(0.0, 1.0);
        }
    }

    /// The selected button's name, for the panel's own line.
    pub fn selected_label(&self) -> Option<&str> {
        self.selected.map(|index| self.layout.label(index))
    }

    /// The actions a finger is holding right now.
    pub fn pressed_actions(&self) -> Vec<Action> {
        self.held.iter().copied().collect()
    }

    pub fn is_held(&self, action: Action) -> bool {
        self.held.contains(&action)
    }

    pub fn fingers(&self) -> usize {
        self.fingers.len()
    }

    /// The last drag position of a looking finger, kept beside the fingers rather
    /// than inside them so a `&mut self` move of the layout cannot alias it.
    fn last_position(&self, id: u64) -> Option<(f32, f32)> {
        self.positions.iter().find(|(finger, ..)| *finger == id).map(|(_, x, y)| (*x, *y))
    }

    fn remember_position(&mut self, id: u64, x: f32, y: f32) {
        match self.positions.iter_mut().find(|(finger, ..)| *finger == id) {
            Some(entry) => {
                entry.1 = x;
                entry.2 = y;
            }
            None => self.positions.push((id, x, y)),
        }
    }

    fn forget_position(&mut self, id: u64) {
        self.positions.retain(|(finger, ..)| *finger != id);
    }
}

/// Which buttons survive the pad being hidden: the two that can bring it back.
fn keeps_its_place_when_hidden(action: Action) -> bool {
    matches!(action, Action::ToggleControls | Action::MouseVirtual)
}

/// Which slot of the strip a point is over.
fn hotbar_slot(strip: Rect, x: f32) -> u8 {
    let fraction = ((x - strip.x) / strip.width).clamp(0.0, 0.999_9);
    (fraction * 9.0) as u8 + 1
}

/// A position on the left half is a fraction of the screen's width from the left.
fn anchor_x(left: f32, width: f32) -> String {
    if left + 1.0 > width / 2.0 {
        format!("{} * ${{screen_width}} - ${{width}}", left / width)
    } else {
        format!("{} * ${{screen_width}}", left / width)
    }
}

fn anchor_y(top: f32, height: f32) -> String {
    if top + 1.0 > height / 2.0 {
        format!("{} * ${{screen_height}} - ${{height}}", top / height)
    } else {
        format!("{} * ${{screen_height}}", top / height)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: f32 = 1920.0;
    const H: f32 = 953.0;

    fn hud() -> Hud {
        // The device's own density, so a 50 dp button is 66 pixels: the numbers the
        // tests aim at are the numbers a finger would land on.
        let mut layout = Layout::default_pad();
        layout.density = 1.3236;
        Hud::new(layout)
    }

    /// The middle of a named button, as a finger would aim at it.
    fn at(hud: &Hud, name: &str) -> [f32; 2] {
        let index = hud
            .layout
            .file
            .controls
            .iter()
            .position(|control| control.name == name)
            .unwrap_or_else(|| panic!("no button called {name}"));
        hud.buttons(W, H)[index].rect.center()
    }

    #[test]
    fn a_hold_sends_a_press_and_a_release() {
        let mut hud = hud();
        let pri = at(&hud, "PRI");
        assert!(hud.down(1, pri[0], pri[1], W, H));
        assert_eq!(hud.take_edges(), vec![(Action::MousePrimary, true)]);
        hud.up(1);
        assert_eq!(hud.take_edges(), vec![(Action::MousePrimary, false)]);
    }

    #[test]
    fn a_toggle_stays_on_after_the_finger_lifts() {
        let mut hud = hud();
        // The default pad's sneak is the only toggle in it.
        let sneak = at(&hud, "SNEAK");
        hud.down(1, sneak[0], sneak[1], W, H);
        assert_eq!(hud.take_edges(), vec![(Action::Sneak, true)]);
        hud.up(1);
        assert!(hud.take_edges().is_empty(), "a toggle must not release itself");
        // And the next tap turns it off.
        hud.down(2, sneak[0], sneak[1], W, H);
        assert_eq!(hud.take_edges(), vec![(Action::Sneak, false)]);
    }

    #[test]
    fn a_drag_on_empty_glass_turns_the_camera() {
        let mut hud = hud();
        assert!(!hud.down(1, 1200.0, 300.0, W, H));
        hud.drag(1, 1260.0, 320.0, W, H);
        let look = hud.take_look();
        assert!(look[0] > 0.0 && look[1] > 0.0, "{look:?}");
    }

    #[test]
    fn a_drag_off_a_held_button_releases_it() {
        let mut hud = hud();
        let pri = at(&hud, "PRI");
        hud.down(1, pri[0], pri[1], W, H);
        assert_eq!(hud.take_edges(), vec![(Action::MousePrimary, true)]);
        hud.drag(1, 1200.0, 200.0, W, H);
        assert_eq!(hud.take_edges(), vec![(Action::MousePrimary, false)]);
    }

    #[test]
    fn the_virtual_mouse_moves_the_cursor_instead_of_the_camera() {
        let mut hud = hud();
        hud.set_mouse(true, W, H);
        // It starts where the toggle put it, not under the finger: a tap on a corner
        // must not send the cursor across the screen.
        let start = hud.cursor();
        assert!(!hud.down(1, 1000.0, 400.0, W, H));
        hud.drag(1, 1040.0, 430.0, W, H);
        assert_eq!(hud.take_look(), [0.0, 0.0], "the camera must not turn");
        assert_eq!(hud.cursor(), [start[0] + 40.0, start[1] + 30.0]);
        assert_eq!(hud.take_cursor(), [40.0, 30.0]);
    }

    #[test]
    fn the_cursor_follows_its_own_drag_rather_than_the_finger() {
        let mut hud = hud();
        hud.set_mouse(true, W, H);
        hud.down(1, 200.0, 200.0, W, H);
        hud.drag(1, 260.0, 200.0, W, H);
        let first = hud.cursor();
        hud.drag(1, 320.0, 200.0, W, H);
        // Each drag moves the cursor by its own delta: two 60-pixel drags are 120
        // pixels of cursor, not a jump to the finger's position.
        assert!((hud.cursor()[0] - first[0] - 60.0).abs() < 0.5, "{:?}", hud.cursor());
    }

    #[test]
    fn a_hotbar_tap_picks_the_slot_under_the_finger() {
        let mut hud = hud();
        hud.set_gui_scale(2.0);
        let strip = hud.hotbar_rect(W, H).unwrap();
        // The middle of the third slot.
        let x = strip.x + strip.width * (2.5 / 9.0);
        let y = strip.y + strip.height / 2.0;
        assert!(hud.down(1, x, y, W, H));
        assert_eq!(hud.take_edges(), vec![(Action::Hotbar(3), true)]);
    }

    #[test]
    fn sliding_along_the_hotbar_sweeps_the_slots() {
        let mut hud = hud();
        hud.set_gui_scale(2.0);
        let strip = hud.hotbar_rect(W, H).unwrap();
        let y = strip.y + strip.height / 2.0;
        hud.down(1, strip.x + strip.width * (0.5 / 9.0), y, W, H);
        hud.take_edges();
        for slot in 2..=9u8 {
            let x = strip.x + strip.width * ((f32::from(slot) - 0.5) / 9.0);
            hud.drag(1, x, y, W, H);
            assert_eq!(hud.take_edges(), vec![(Action::Hotbar(slot), true)]);
        }
    }

    #[test]
    fn the_hotbar_follows_the_gui_scale() {
        let mut hud = hud();
        hud.set_gui_scale(1.0);
        let one = hud.hotbar_rect(W, H).unwrap();
        hud.set_gui_scale(3.0);
        let three = hud.hotbar_rect(W, H).unwrap();
        assert!(three.width > one.width * 2.5);
        assert!(three.y < one.y, "a bigger scale draws the bar higher");
    }

    #[test]
    fn a_double_tap_on_the_same_slot_swaps_hands() {
        let mut hud = hud();
        hud.set_gui_scale(2.0);
        let strip = hud.hotbar_rect(W, H).unwrap();
        let x = strip.x + strip.width * (0.5 / 9.0);
        let y = strip.y + strip.height / 2.0;
        hud.down(1, x, y, W, H);
        hud.up(1);
        hud.take_edges();
        hud.down(2, x, y, W, H);
        assert_eq!(hud.take_edges(), vec![(Action::SwapOffhand, true)]);
    }

    #[test]
    fn moving_a_button_writes_a_formula_that_puts_it_back() {
        let mut hud = hud();
        hud.edit = true;
        let index = hud
            .layout
            .file
            .controls
            .iter()
            .position(|control| control.name == "PRI")
            .unwrap();
        hud.move_button(index, 300.0, 500.0, W, H);
        let placed = hud.buttons(W, H)[index].rect.center();
        assert!((placed[0] - 300.0).abs() < 2.0, "{placed:?}");
        assert!((placed[1] - 500.0).abs() < 2.0, "{placed:?}");
    }

    #[test]
    fn every_control_has_an_action() {
        let hud = hud();
        for placed in hud.buttons(W, H) {
            assert_ne!(placed.action, Action::Raw(0), "{}", hud.layout.label(placed.index));
        }
    }
}
