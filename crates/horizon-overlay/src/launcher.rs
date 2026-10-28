//! The launcher's front end, drawn in imgui over the game's own surface.
//!
//! The desktop launcher is a GPUI app, and GPUI has no Android backend: a phone
//! runs one activity with one winit loop, so the launcher cannot be a second
//! program the way it is on a desktop. It is drawn here instead, in the imgui
//! layer that already paints the touch controls, so it shares the loop, the
//! device and the font atlas with the game.
//!
//! The shape is the one a Minecraft player already knows from the PC launcher: a
//! fixed left column of destinations, and a wide page beside it. The Play page is
//! the exception — it is not a page at all but the title panorama with a few
//! things laid over it, which is why the launcher leaves that half of the glass
//! unpainted and lets the engine's own cube map show through.
//!
//! The split is deliberate. This module knows how a launcher looks and nothing
//! about what is installed: the platform feeds it a [`LauncherModel`] of text and
//! numbers it has gathered from the engine, and it hands back [`LauncherEvent`]s
//! for the platform to act on. That keeps the crate free of the engine, the way
//! the rest of the overlay is.

use std::cell::Cell;

use imgui::{
    Condition, DrawListMut, FontConfig, FontId, FontSource, StyleColor, TextureId, Ui, WindowFlags,
};

thread_local! {
    /// The destination row the pointer was on last frame, if any.
    static NAV_HOVER: Cell<Option<LauncherPage>> = const { Cell::new(None) };

    /// The eased hover glow of each destination row, 0 to 1, one per [`PAGES`] row.
    /// `sidebar` runs every frame and imgui keeps no state for a fill of its own, so
    /// the one number that has to survive between frames lives here: a row's highlight
    /// fades in under the finger and out again when it leaves.
    static NAV_GLOW: Cell<[f32; NAV_ROWS]> = const { Cell::new([0.0; NAV_ROWS]) };

    /// Whether the platform wants animations. `draw` has the answer and the page
    /// functions do not, and threading a bool through thirty button calls would put
    /// the same argument on every line for the one thing they all do alike.
    static MOTION: Cell<bool> = const { Cell::new(true) };

    /// How far the Settings page is scrolled, in pixels. The page is one column of
    /// rows taller than a phone held in landscape, and imgui windows cannot scroll a
    /// column that is drawn with the window's own draw list, so the one number that
    /// has to survive between frames lives here.
    static SETTINGS_SCROLL: Cell<f32> = const { Cell::new(0.0) };

    /// Where the finger was on the last frame of a drag that scrolls the Settings
    /// page. `None` when no drag is in progress.
    static SETTINGS_DRAG: Cell<Option<f32>> = const { Cell::new(None) };

    /// Where the finger landed on the Settings page. A tap and a drag both begin with
    /// a press, and the only thing that tells them apart afterwards is how far the
    /// finger travelled, so the press point has to outlive the frame it happened on.
    static SETTINGS_PRESS: Cell<Option<[f32; 2]>> = const { Cell::new(None) };
}

/// The destinations in the left column, in the order they are drawn.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum LauncherPage {
    #[default]
    Play,
    Installations,
    Skins,
    PatchNotes,
    News,
    Mods,
    Profile,
    /// Reachable from Profile, not from the column: settings are a place you go to
    /// on purpose, and a seventh row would make the column longer than the screen
    /// on a phone held in landscape.
    Settings,
    /// The launcher's own files, reached from the FILES button. A phone cannot open a
    /// folder dialog the way a desktop does, so this lists what is in `import/` and
    /// offers the system file manager for the rest.
    Files,
    /// Horizon's own multiplayer: hosting a world and the address friends join on.
    /// Not the game's server list — that is a different thing, and the button on the
    /// front page used to open it by mistake.
    Multiplayer,
}

impl LauncherPage {
    pub fn label(self) -> &'static str {
        match self {
            LauncherPage::Play => "Play",
            LauncherPage::Installations => "Installations",
            LauncherPage::Skins => "Skins",
            LauncherPage::PatchNotes => "Patch Notes",
            LauncherPage::News => "News",
            LauncherPage::Mods => "Mods",
            LauncherPage::Profile => "Profile",
            LauncherPage::Settings => "Settings",
            LauncherPage::Files => "Files",
            LauncherPage::Multiplayer => "Multiplayer",
        }
    }
}

/// The left column's order. `Settings` is deliberately absent.
pub const PAGES: [LauncherPage; 7] = [
    LauncherPage::Play,
    LauncherPage::Installations,
    LauncherPage::Skins,
    LauncherPage::PatchNotes,
    LauncherPage::News,
    LauncherPage::Mods,
    LauncherPage::Profile,
];

/// How many rows the column has: the one number the hover glow's state is sized by.
const NAV_ROWS: usize = PAGES.len();

/// One line in a list: an installation, a version, a skin.
#[derive(Clone, Debug, Default)]
pub struct LauncherRow {
    pub id: u32,
    pub title: String,
    pub subtitle: String,
    /// A short right-aligned tag: "installed", "26.3", "not supported".
    pub badge: String,
    pub selected: bool,
    pub enabled: bool,
}

/// One slider or switch on the Settings page. The launcher owns how it looks and
/// the platform owns what it means: `key` is the `options.txt` name, and a value
/// that comes back is the platform's to write.
#[derive(Clone, Debug, Default)]
pub struct LauncherSetting {
    pub key: String,
    pub label: String,
    pub value: f32,
    pub min: f32,
    pub max: f32,
    /// A switch rather than a slider.
    pub toggle: bool,
    pub on: bool,
    /// What the number is measured in: "chunks", "fps", "°", "%". Empty for a bare
    /// number. A degree and a percent are set tight against the digits, everything
    /// else gets a space.
    pub unit: String,
    /// The heading this row sits under. The page draws one heading per run of the
    /// same group, so the order of `LauncherModel::settings` is the page's order.
    pub group: String,
    /// The smallest change the slider can make, and the precision its number is
    /// shown with. Zero means a hundredth of the range.
    pub step: f32,
    /// The stored value is 0..1 and the player reads it as a percentage. The number
    /// sent back is still the stored one; only the label is multiplied.
    pub percent: bool,
}

/// A card on News or Patch Notes: a heading, a tag, and a paragraph.
#[derive(Clone, Debug, Default)]
pub struct LauncherStory {
    pub title: String,
    pub tag: String,
    pub body: String,
}

/// Everything the launcher shows, gathered by the platform from the engine.
#[derive(Clone, Debug, Default)]
pub struct LauncherModel {
    pub page: LauncherPage,
    /// The instance the big Play button launches.
    pub instance: String,
    pub version: String,
    pub playable: bool,
    /// The list for the current page.
    pub rows: Vec<LauncherRow>,
    /// A line under the list: install or sound-pack status.
    pub status: String,
    /// 0.0 to 1.0 while something downloads, or `None` when idle.
    pub progress: Option<f32>,
    /// What the front button says, when it is not Play or Install. The bring-up card
    /// has no engine to hand the screen to, so once its install lands the one useful
    /// thing left is a restart, and the button has to say so.
    pub front: Option<String>,
    pub account: String,
    /// Extra one-line facts for the Profile page.
    pub facts: Vec<(String, String)>,
    /// The name field is open, which on a phone means the keyboard belongs to it.
    pub editing: bool,
    pub draft: String,
    /// The Settings page's controls, in the order they are drawn.
    pub settings: Vec<LauncherSetting>,
    /// The PNGs in `import/skins/`, by file name.
    pub skins: Vec<String>,
    /// The one the player picked, if any.
    pub chosen_skin: Option<String>,
    pub news: Vec<LauncherStory>,
    pub patches: Vec<LauncherStory>,
    /// The registry's mods, as the platform fetched them.
    pub mods: Vec<LauncherMod>,
    /// The `.hmod` files already installed, by name. Both the launcher's `import/mods/`
    /// and the game's `horizon-mods/` are listed, so a mod dropped in by hand shows up
    /// beside one installed from the registry.
    pub installed_mods: Vec<String>,
    /// The mod id being downloaded right now, if one is. The page names it and keeps
    /// the install buttons from starting a second download on top of the first.
    pub installing: Option<String>,
    /// Where the mod registry's website is, so the page can offer it.
    pub mods_site: String,
    /// The last registry fetch failed, so the page says why rather than showing an
    /// empty list that reads like "there is nothing to install".
    pub mods_failed: bool,
    /// The registry is being fetched on a worker right now. The page says so instead
    /// of freezing the whole launcher for the two or three seconds a network round
    /// trip costs.
    pub mods_loading: bool,
    /// The accounts this device knows, and which one Play uses.
    pub accounts: Vec<LauncherAccount>,
    /// The chosen skin's pixel size, so the player figure can map the PNG's own
    /// regions onto the model. The texture itself belongs to the overlay, which owns
    /// the device; this is only what the layout needs to know about it.
    pub skin_size: [u32; 2],
    /// Where the model is turned to, in radians. The platform advances it so the
    /// player turns on the page; a still figure is what made the old page look like a
    /// drawing rather than a model.
    pub skin_yaw: f32,
    /// Which built-in touch layout is in force: `"java"` or `"bedrock"`.
    pub controls_preset: String,
    /// While a world is hosted: the address friends join on.
    pub hosting: Option<String>,
    /// A Microsoft sign-in is waiting on the browser, so the page offers Cancel
    /// instead of a second sign-in.
    pub sign_in_waiting: bool,
}

/// One account the launcher can play as.
#[derive(Clone, Debug, Default)]
pub struct LauncherAccount {
    pub name: String,
    /// "Microsoft" or "Offline".
    pub kind: String,
    pub selected: bool,
}

/// A decoded skin, as the client read it from `import/skins/`.
#[derive(Clone, Debug, Default)]
pub struct SkinImage {
    pub width: u32,
    pub height: u32,
    /// RGBA8, `width * height * 4` bytes.
    pub rgba: Vec<u8>,
}

/// One mod from Horizon's registry: enough to show it, and enough to install it.
#[derive(Clone, Debug, Default)]
pub struct LauncherMod {
    pub id: String,
    pub name: String,
    pub summary: String,
    pub version: String,
    /// The file name the registry will install as, so the page can tell whether it is
    /// already on disk.
    pub file: String,
}

/// What the player asked for, for the platform to carry out.
#[derive(Clone, PartialEq, Debug)]
pub enum LauncherEvent {
    Goto(LauncherPage),
    Play,
    PlayRow(u32),
    SelectRow(u32),
    NewInstance,
    Install,
    /// The card's install landed but the loop it would need to play is already spent:
    /// the honest button left is one that ends the process so the next open starts clean.
    Restart,
    /// The account card was tapped: the platform opens a field and starts feeding it.
    RenameAccount,
    /// A new player name for the account card to keep.
    SetAccount(String),
    /// Open the launcher's own Settings page.
    OpenSettings,
    /// Open the skin picker.
    OpenSkins,
    /// A skin file from `import/skins/` was chosen.
    PickSkin(String),
    /// Write the settings archive into import/.
    ExportSettings,
    /// Read the newest settings archive from import/.
    ImportSettings,
    /// Pick a file of this kind from the system picker and copy it where the game
    /// reads it: "skin", "mod", "resourcepack", "settings".
    ImportFiles(String),
    /// Copy a folder of the game's own files out where a file manager can reach it.
    ExportFiles(String),
    Quit,
    /// A slider moved: `options.txt` key and its new value.
    SetOption(String, f32),
    /// A switch flipped.
    ToggleOption(String, bool),
    /// Install this mod from the registry.
    InstallMod(String),
    /// Delete this installed `.hmod` (a file name from the INSTALLED list) from both
    /// folders the launcher and the game read.
    UninstallMod(String),
    /// Fetch the registry again.
    RefreshMods,
    /// Open the mod registry's site in a browser.
    OpenModsSite,
    /// Ask the system file manager for `import/`.
    RevealFiles,
    /// Host this world on the local network.
    HostWorld(u32),
    /// Stop hosting.
    StopHosting,
    /// Create another account.
    AddAccount,
    /// Play as this account.
    SwitchAccount(u32),
    /// Forget this account.
    RemoveAccount(u32),
    /// Sign in with a Microsoft account (the device-code flow).
    SignInMicrosoft,
    /// Stop waiting for the Microsoft sign-in.
    CancelSignIn,
    /// Use this built-in touch layout: "java" or "bedrock".
    SetControls(String),
}

/// The palette: the launcher's own, not the game's menus. Cool near-black plates,
/// one green for the thing that starts the game, and a dim grey for everything
/// that is only a label. Colours are `0xRRGGBB`.
pub struct Css {
    /// The sidebar and the page behind a list.
    pub page: u32,
    /// The sidebar's own face, a shade off the page so the seam reads.
    pub side: u32,
    /// A card, a row, a field.
    pub panel: u32,
    /// A hovered or selected row.
    pub raised: u32,
    /// The raised face of a button.
    pub button: u32,
    /// A sunken well: the search field, the dropdown, a slider track.
    pub well: u32,
    pub rule: u32,
    pub bevel: u32,
    pub ink: u32,
    pub ink_soft: u32,
    pub muted: u32,
    /// The green: Play, the selected destination, the active switch.
    pub accent: u32,
    /// The green's darker bottom lip, which is what makes a button look raised.
    pub accent_lip: u32,
    pub on_accent: u32,
    pub danger: u32,
}

pub const THEME: Css = Css {
    page: 0x0c0c0e,
    side: 0x131315,
    panel: 0x1b1b1e,
    raised: 0x242428,
    button: 0x2a2a2e,
    well: 0x0a0a0c,
    rule: 0x2e2e33,
    bevel: 0x3a3a40,
    ink: 0xf2f2f4,
    ink_soft: 0xc9c9cf,
    muted: 0x8e8e97,
    accent: 0x3fa33f,
    accent_lip: 0x2c7a2c,
    on_accent: 0xffffff,
    danger: 0xe06a5a,
};

/// The one green the selected destination and Play share, and the two greys that
/// stand in for a disabled control.
const ACCENT_HOVER: u32 = 0x4cb84c;

fn rgba(c: u32, a: f32) -> [f32; 4] {
    [
        ((c >> 16) & 0xff) as f32 / 255.0,
        ((c >> 8) & 0xff) as f32 / 255.0,
        (c & 0xff) as f32 / 255.0,
        a,
    ]
}

/// One frame of a hover glow: `current` moving towards `target` at a rate set by the
/// frame's own time, so the same fade takes the same tenth of a second at any refresh
/// rate. Exponential rather than linear because a linear ramp visibly stops; this one
/// arrives.
fn ease_glow(current: f32, target: f32, dt: f32) -> f32 {
    if dt <= 0.0 {
        return current;
    }
    let k = 1.0 - (-dt / NAV_GLOW_TAU).exp();
    (current + (target - current) * k).clamp(0.0, 1.0)
}

/// The faces the launcher draws with: Geist for body, Pixelify Sans for the
/// blocky wordmark, headings, navigation and the primary action.
pub struct Fonts {
    pub body: FontId,
    pub pixel: FontId,
    pub title: FontId,
    /// A smaller pixel face for the dense rows of a list.
    pub small: FontId,
}

/// Load the fonts and set the base style. Called once, where the fonts are added.
pub fn theme(imgui: &mut imgui::Context, dpi: f32) -> Fonts {
    let body = imgui.fonts().add_font(&[FontSource::TtfData {
        data: horizon_theme::fonts::GEIST_REGULAR,
        size_pixels: 15.0 * dpi,
        config: Some(FontConfig {
            oversample_h: 2,
            oversample_v: 1,
            ..Default::default()
        }),
    }]);
    let pixel = imgui.fonts().add_font(&[FontSource::TtfData {
        data: horizon_theme::fonts::PIXEL_SEMIBOLD,
        size_pixels: 17.0 * dpi,
        config: Some(FontConfig::default()),
    }]);
    let title = imgui.fonts().add_font(&[FontSource::TtfData {
        data: horizon_theme::fonts::PIXEL_BOLD,
        size_pixels: 34.0 * dpi,
        config: Some(FontConfig::default()),
    }]);
    let small = imgui.fonts().add_font(&[FontSource::TtfData {
        data: horizon_theme::fonts::PIXEL_MEDIUM,
        size_pixels: 13.0 * dpi,
        config: Some(FontConfig::default()),
    }]);

    let c = &THEME;
    let style = imgui.style_mut();
    style.window_rounding = 0.0;
    style.frame_rounding = horizon_theme::RADIUS;
    style.scrollbar_rounding = horizon_theme::RADIUS;
    style.grab_rounding = horizon_theme::RADIUS;
    style.window_border_size = 0.0;
    style.frame_border_size = 0.0;
    style.scrollbar_size = 8.0 * dpi;
    style.item_spacing = [0.0, 8.0 * dpi];
    style.window_padding = [0.0, 0.0];
    let colors = &mut style.colors;
    colors[StyleColor::Text as usize] = rgba(c.ink, 1.0);
    colors[StyleColor::WindowBg as usize] = rgba(c.page, 1.0);
    colors[StyleColor::ChildBg as usize] = [0.0, 0.0, 0.0, 0.0];
    colors[StyleColor::ScrollbarBg as usize] = rgba(c.page, 0.0);
    colors[StyleColor::ScrollbarGrab as usize] = rgba(c.rule, 1.0);
    colors[StyleColor::ScrollbarGrabHovered as usize] = rgba(c.muted, 1.0);
    colors[StyleColor::ScrollbarGrabActive as usize] = rgba(c.ink, 1.0);
    Fonts {
        body,
        pixel,
        title,
        small,
    }
}

/// The column's width, the height of a destination, and the air around them, in
/// logical pixels. These are measured off the reference launcher at 1920x953 and
/// divided by the dpi it reports (1.3236), so the same proportions come back at any
/// screen size: a column of 397 px, rows of 62 with 9 between them, an avatar of 46.
const SIDE_FRACTION: f32 = 0.207;
const NAV_H: f32 = 46.8;
const NAV_GAP: f32 = 6.8;
const NAV_INSET: f32 = 13.6;
const PAD: f32 = 20.4;
/// The avatar's face, the play button and the version dropdown, likewise measured.
const FACE: f32 = 34.8;
const PLAY_W: f32 = 347.6;
const PLAY_H: f32 = 54.4;
const DROP_W: f32 = 257.0;
const DROP_H: f32 = 59.0;

/// How far a page's content slides in when the page changes, and how heavy the veil
/// over it starts. Both are spent by the time `MOVE` is, so a page change is felt as a
/// settle rather than as a transition.
const PAGE_SLIDE: f32 = 8.0;
const PAGE_VEIL: f32 = 0.4;
/// How far a raised button's face drops under a finger, in logical pixels.
const PRESS_DROP: f32 = 2.0;
/// The hover glow of a column row: the weight of the fill at full strength, and the
/// time constant of its approach. The glow is a settle rather than a fade — a tenth of
/// a second is about the shortest that still reads as movement.
const NAV_GLOW_ALPHA: f32 = 0.18;
const NAV_GLOW_TAU: f32 = 0.09;

/// The one geometry every element is placed from: the sidebar's edge is where the
/// page begins, and the page's own gutter is the same number everywhere.
#[derive(Clone, Copy)]
struct Grid {
    /// The page's left edge: just past the sidebar.
    x: f32,
    /// The page's width.
    w: f32,
    /// The gutter inside the page.
    pad: f32,
    /// The sidebar's width.
    side: f32,
}

impl Grid {
    fn new(width: f32, dpi: f32) -> Self {
        // A phone in landscape has room for a column; a narrow one gets a third of
        // the glass rather than a column that leaves no page.
        let side = (width * SIDE_FRACTION).max(220.0 * dpi).min(width * 0.42);
        Self {
            x: side,
            w: width - side,
            pad: PAD * dpi,
            side,
        }
    }

    /// Where a right-aligned control of width `w` starts.
    fn right(&self, w: f32) -> f32 {
        self.x + self.w - self.pad - w
    }
}

fn fill(draw: &DrawListMut<'_>, min: [f32; 2], max: [f32; 2], col: u32, a: f32, r: f32) {
    if max[0] <= min[0] || max[1] <= min[1] {
        return;
    }
    draw.add_rect(min, max, rgba(col, a))
        .filled(true)
        .rounding(r)
        .build();
}

fn stroke(
    draw: &DrawListMut<'_>,
    min: [f32; 2],
    max: [f32; 2],
    col: u32,
    a: f32,
    r: f32,
    t: f32,
) {
    if max[0] <= min[0] || max[1] <= min[1] {
        return;
    }
    draw.add_rect(min, max, rgba(col, a))
        .rounding(r)
        .thickness(t)
        .build();
}

/// A card: its face, and the one-pixel rule that separates it from the page.
fn panel(draw: &DrawListMut<'_>, c: &Css, min: [f32; 2], max: [f32; 2], face: u32, radius: f32) {
    fill(draw, min, max, face, 1.0, radius);
    stroke(draw, min, max, c.rule, 1.0, radius, 1.0);
}

/// A raised control: the face, a light top edge and a dark lip, which is the whole
/// trick behind a button that looks pressable on a flat screen. `lit` is the light top
/// edge: a face under a finger drops and loses it, because a bevel on a pressed button
/// is what makes it read as still raised.
fn raised_face(
    draw: &DrawListMut<'_>,
    min: [f32; 2],
    max: [f32; 2],
    face: u32,
    bevel: u32,
    lip: u32,
    radius: f32,
    dpi: f32,
    lit: bool,
) {
    fill(draw, min, max, face, 1.0, radius);
    // The bevel is the top two pixels; the lip is the bottom three.
    if lit {
        fill(
            draw,
            [min[0] + radius * 0.5, min[1]],
            [max[0] - radius * 0.5, min[1] + 2.0 * dpi],
            bevel,
            1.0,
            0.0,
        );
    }
    fill(
        draw,
        [min[0] + radius * 0.5, max[1] - 3.0 * dpi],
        [max[0] - radius * 0.5, max[1]],
        lip,
        1.0,
        0.0,
    );
}

/// A colour at 78% of its own brightness, which is the face of a button under a
/// finger: darker without being a different colour, so a green Play is still green.
fn darker(c: u32) -> u32 {
    let scale = |shift: u32| (((c >> shift) & 0xff) as f32 * 0.78) as u32;
    (scale(16) << 16) | (scale(8) << 8) | scale(0)
}

/// The width of `text` in a given face, without leaving that face pushed.
fn text_width(ui: &Ui, font: FontId, text: &str) -> f32 {
    let _f = ui.push_font(font);
    ui.calc_text_size(text)[0]
}

/// The text cut to what fits, with an ellipsis. A status line is the one string the
/// launcher does not get to choose the length of — an install failure carries a URL.
fn fit(ui: &Ui, text: &str, max_w: f32) -> String {
    if max_w <= 0.0 {
        return String::new();
    }
    if ui.calc_text_size(text)[0] <= max_w {
        return text.to_string();
    }
    let mut out = String::new();
    for ch in text.chars() {
        out.push(ch);
        let mut probe = out.clone();
        probe.push_str("...");
        if ui.calc_text_size(&probe)[0] > max_w {
            out.pop();
            break;
        }
    }
    out.push_str("...");
    out
}

/// Greedy word wrap, because a paragraph in a card has to end inside it and imgui's
/// draw list has no wrapping of its own. The font it measures in is whichever one is
/// current, so it is only ever called outside a font push.
fn wrap(ui: &Ui, text: &str, max_w: f32) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        let mut probe = line.clone();
        if !probe.is_empty() {
            probe.push(' ');
        }
        probe.push_str(word);
        if !line.is_empty() && ui.calc_text_size(&probe)[0] > max_w {
            lines.push(std::mem::take(&mut line));
            line.push_str(word);
        } else {
            line = probe;
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

/// The events a page may raise this frame. A tap that lands while the opening still
/// covers the glass is swallowed here rather than in every card, so nothing behind
/// the dim can be pressed by a finger that could not see it yet.
struct Sink<'a> {
    events: &'a mut Vec<LauncherEvent>,
    live: bool,
}

impl<'a> Sink<'a> {
    fn push(&mut self, event: LauncherEvent) {
        if self.live {
            self.events.push(event);
        }
    }

    fn live(&self) -> bool {
        self.live
    }
}

/// The whole launcher, edge to edge.
///
/// `opening` is `Some(t)` while the opening animation runs, with `t` in 0..1; it is
/// `None` once spent. `fade` is how far a page change has got, 0 to 1: the page's
/// content slides and fades in with it while the column stays put, because the column
/// is the thing the finger is still on. `panorama` says the engine has already drawn
/// the title's cube map behind this window, so the front page must leave that half of
/// the glass alone. Navigation is applied to `model` in place, so the destination that
/// was pressed and the page it switched to are drawn in the same pass.
#[allow(clippy::too_many_arguments)]
pub fn draw(
    ui: &Ui,
    model: &mut LauncherModel,
    fonts: &Fonts,
    dpi: f32,
    width: f32,
    height: f32,
    opening: Option<f32>,
    // A Play transition in progress, 0 to 1: the page zooms in, the wordmark fades,
    // and the last frame is the world's own loading screen.
    zoom: Option<f32>,
    // How far a page change has got, 0 to 1, as the overlay timed it.
    fade: f32,
    // The system's "remove animations" setting. Everything time-based in here is
    // skipped when it is set: the page slide and veil, the button press, the hover
    // glow. The overlay has already done the same for the opening and the zoom.
    reduce_motion: bool,
    panorama: bool,
    skin: Option<TextureId>,
    out: &mut Vec<LauncherEvent>,
) {
    let c = &THEME;
    // The buttons and the column are called from here and cannot see this argument, so
    // it goes in the one place they can read: the flag the press and the glow consult.
    MOTION.with(|m| m.set(!reduce_motion));
    if reduce_motion {
        // Nothing is arriving and nothing is held: clear the eased values so the first
        // animated frame after the setting goes back to off starts from rest.
        NAV_GLOW.with(|g| g.set([0.0; NAV_ROWS]));
        NAV_HOVER.with(|h| h.set(None));
    }
    // The opening owns the glass until it is spent. Its last frame is the first one
    // where a finger is allowed to land on anything. A Play transition locks it out
    // too: the world is opening behind the zoom, and a second tap has nowhere to go.
    let live = !matches!(opening, Some(t) if t < 1.0) && zoom.is_none();
    let mut sink = Sink { events: out, live };
    let g = Grid::new(width, dpi);
    // The interface arrives by rising a little and fading in; the rise is one number
    // the whole layout is offset by, so the column and the page move together.
    let rise = match opening {
        Some(t) => horizon_theme::RISE * (1.0 - horizon_theme::ease_out_cubic(t)) * dpi,
        None => 0.0,
    };
    // The page change, eased here rather than in the overlay: the overlay knows the
    // time, this knows the curve the rest of the design system moves on. `MOVE` is the
    // theme's own curve for selection and page changes.
    let page_ease = if reduce_motion {
        1.0
    } else {
        horizon_theme::MOVE.ease(fade)
    };
    let slide = if page_ease < 1.0 {
        PAGE_SLIDE * dpi * (1.0 - page_ease)
    } else {
        0.0
    };
    // The content is drawn a little to the right and arrives at its place. The column
    // is not: it was already there, and moving it would undo the highlight the finger
    // is looking at. The width comes in by the same amount, so the page's right gutter
    // — where Play and the version dropdown are pinned — does not move at all.
    let page_g = Grid {
        x: g.x + slide,
        w: g.w - slide,
        ..g
    };
    let _ = ui
        .window("##launcher")
        .flags(
            WindowFlags::NO_TITLE_BAR
                | WindowFlags::NO_RESIZE
                | WindowFlags::NO_MOVE
                | WindowFlags::NO_COLLAPSE
                | WindowFlags::NO_SCROLLBAR
                | WindowFlags::NO_SCROLL_WITH_MOUSE
                | WindowFlags::NO_BACKGROUND
                | WindowFlags::NO_FOCUS_ON_APPEARING
                | WindowFlags::NO_BRING_TO_FRONT_ON_FOCUS,
        )
        .position([0.0, 0.0], Condition::Always)
        .size([width, height], Condition::Always)
        .build(|| {
            if matches!(opening, Some(t) if t <= 0.0) {
                // The first frame of a launcher open: whatever glow the column had
                // when it was last on screen belongs to a finger that is long gone.
                NAV_GLOW.with(|g| g.set([0.0; NAV_ROWS]));
                NAV_HOVER.with(|h| h.set(None));
                splash(ui, fonts, c, dpi, width, height);
                return;
            }
            // A Play transition: the page itself is drawn, then a zoom and a fade go
            // over it. The last frame is opaque, so the world's loading screen arrives
            // under a dark glass rather than with a cut.
            if let Some(t) = zoom {
                zoom_out(ui, fonts, c, dpi, width, height, t, model, panorama);
                return;
            }
            // The page the *model* was built for. The column below applies a tab tap
            // to `model.page` in this same pass, and the platform only learns about it
            // on the next frame — so the content drawn here has to be the page whose
            // data actually arrived, not the one the finger just chose. Drawing the
            // new page with the old page's model is what made the Mods page show the
            // Play page's status line and an empty list.
            let page = model.page;
            sidebar(ui, model, fonts, c, dpi, width, height, rise, &mut sink);
            // The Play page is the title panorama with things laid over it, so the
            // launcher must not paint over the glass there: the engine's cube map is
            // already drawn behind this window. Every other page is opaque. When there
            // is no panorama behind it — the bring-up card — the page paints itself.
            let over_panorama = panorama && page == LauncherPage::Play;
            if !over_panorama {
                let draw = ui.get_window_draw_list();
                // Only the page's own region: the column has already painted itself,
                // and covering it here would undo the selection highlight.
                fill(&draw, [g.x, 0.0], [width, height], c.page, 1.0, 0.0);
            }
            match page {
                LauncherPage::Play => play_page(
                    ui,
                    model,
                    fonts,
                    c,
                    dpi,
                    &page_g,
                    height,
                    rise,
                    over_panorama,
                    &mut sink,
                ),
                LauncherPage::Installations => {
                    installations_page(ui, model, fonts, c, dpi, &page_g, height, rise, &mut sink)
                }
                LauncherPage::Skins => {
                    skins_page(ui, model, fonts, c, dpi, &page_g, height, rise, skin, &mut sink)
                }
                LauncherPage::PatchNotes => {
                    stories_page(
                        ui,
                        &model.patches,
                        "PATCH NOTES",
                        "What changed in each Horizon build.",
                        fonts,
                        c,
                        dpi,
                        &page_g,
                        height,
                        rise,
                    )
                }
                LauncherPage::News => stories_page(
                    ui,
                    &model.news,
                    "NEWS",
                    "Horizon news and updates.",
                    fonts,
                    c,
                    dpi,
                    &page_g,
                    height,
                    rise,
                ),
                LauncherPage::Mods => {
                    mods_page(ui, model, fonts, c, dpi, &page_g, height, rise, &mut sink)
                }
                LauncherPage::Profile => {
                    profile_page(ui, model, fonts, c, dpi, &page_g, height, rise, &mut sink)
                }
                LauncherPage::Settings => {
                    settings_page(ui, model, fonts, c, dpi, &page_g, height, rise, &mut sink)
                }
                LauncherPage::Files => {
                    files_page(ui, model, fonts, c, dpi, &page_g, height, rise, &mut sink)
                }
                LauncherPage::Multiplayer => {
                    multiplayer_page(ui, model, fonts, c, dpi, &page_g, height, rise, &mut sink)
                }
            }
            // The other half of a page change: the content fades up out of the page's
            // own colour. Only the page's region, so the column the finger is on does
            // not dim under it.
            if page_ease < 1.0 {
                let draw = ui.get_window_draw_list();
                fill(
                    &draw,
                    [g.x, 0.0],
                    [width, height],
                    c.page,
                    (1.0 - page_ease) * PAGE_VEIL,
                    0.0,
                );
            }
            if let Some(t) = opening {
                // The reliable half of the entrance: the interface is already drawn,
                // so the veil is what fades. The rise above is the polish on top of it.
                let ease = horizon_theme::ease_out_cubic(t);
                if ease < 1.0 {
                    let draw = ui.get_window_draw_list();
                    fill(&draw, [0.0, 0.0], [width, height], c.page, 1.0 - ease, 0.0);
                }
            }
        });
    if model.editing && live {
        rename_dialog(ui, model, fonts, c, dpi, width, height, &mut sink);
    }
}

/// The Play transition, drawn in place of the page: the panorama pushes in towards
/// the middle while the wordmark and the controls fade, and the last frame is a dark
/// glass the world's own loading screen appears behind.
///
/// imgui has no transform for a whole page, so the zoom is the panorama's own frame:
/// the art is already behind this window and cannot be scaled from here, so the effect
/// is made of what can move — a vignette that closes in, the wordmark growing, and the
/// scrim deepening. That reads as a push into the world without needing a second pass
/// over the engine's cube map.
///
/// Everything here is one number. `horizon_theme` has no `ease_in_out_cubic`, so the
/// zoom moves on the shared `ease_out_cubic`, and the vignette and the veil are both
/// read off that same eased value: two curves on two layers is what makes a transition
/// look like two transitions. The vignette also starts at nothing — it used to open at
/// 55% dark, which was a step on the very first frame rather than the start of a push.
fn zoom_out(
    ui: &Ui,
    fonts: &Fonts,
    c: &Css,
    dpi: f32,
    width: f32,
    height: f32,
    t: f32,
    model: &LauncherModel,
    panorama: bool,
) {
    let ease = horizon_theme::ease_out_cubic(t);
    let draw = ui.get_window_draw_list();
    // The glass the page was on: the panorama shows through at the start, and by the
    // end this is the same near-black the loading screen sits on.
    if !panorama {
        fill(&draw, [0.0, 0.0], [width, height], c.page, 1.0, 0.0);
    }
    // A closing vignette: four quads whose inner edges march inwards, which is what a
    // camera pushing into a scene does to its edges. The bands' own weight grows with
    // the same ease, so the first frame is the page and not a dark frame.
    let inset = (0.5 - 0.5 * ease) * height * 0.55;
    let dark = ease;
    draw.add_rect_filled_multicolor(
        [0.0, 0.0],
        [width, inset.max(1.0)],
        rgba(c.page, dark),
        rgba(c.page, dark),
        rgba(c.page, 0.0),
        rgba(c.page, 0.0),
    );
    draw.add_rect_filled_multicolor(
        [0.0, height - inset.max(1.0)],
        [width, height],
        rgba(c.page, 0.0),
        rgba(c.page, 0.0),
        rgba(c.page, dark),
        rgba(c.page, dark),
    );
    // The wordmark: it grows a little and fades, which is the part of the page that
    // says what the player is leaving.
    let scale = 1.0 + 0.35 * ease;
    let alpha = (1.0 - ease).powf(1.6);
    if alpha > 0.01 {
        let _f = ui.push_font(fonts.title);
        let [tw, th] = ui.calc_text_size("MINECRAFT");
        let cx = width / 2.0 - tw * scale / 2.0;
        let cy = height * 0.40 - th * scale / 2.0;
        // The font cannot be scaled here, so the growth is the shadow behind it: two
        // offset copies widen as the ease runs, which reads as the word coming at the
        // viewer.
        let spread = 1.0 + 6.0 * ease * dpi;
        draw.add_text(
            [cx - spread, cy + spread],
            rgba(0x000000, 0.45 * alpha),
            "MINECRAFT",
        );
        draw.add_text([cx, cy], rgba(0xffffff, alpha), "MINECRAFT");
    }
    // The last stretch is nearly opaque, so the loading screen behind it is a fade and
    // not a cut. Same `ease` as the vignette: one curve, two layers.
    if ease > 0.55 {
        let veil = ((ease - 0.55) / 0.45).powf(1.4);
        fill(&draw, [0.0, 0.0], [width, height], c.page, veil * 0.92, 0.0);
    }
    let _ = (dpi, model);
}

/// The hold before the interface arrives: the wordmark on the glass and nothing else.
fn splash(ui: &Ui, fonts: &Fonts, c: &Css, dpi: f32, width: f32, height: f32) {
    let draw = ui.get_window_draw_list();
    fill(&draw, [0.0, 0.0], [width, height], c.page, 1.0, 0.0);
    let cx = width / 2.0;
    let bar = 96.0 * dpi;
    let under = {
        let _f = ui.push_font(fonts.title);
        let [mw, mh] = ui.calc_text_size("HORIZON");
        let top = (height - mh) / 2.0 - 26.0 * dpi;
        draw.add_text([cx - mw / 2.0, top], rgba(c.ink, 1.0), "HORIZON");
        top + mh + 16.0 * dpi
    };
    fill(
        &draw,
        [cx - bar / 2.0, under],
        [cx + bar / 2.0, under + 3.0 * dpi],
        c.accent,
        1.0,
        1.5 * dpi,
    );
    {
        let _f = ui.push_font(fonts.body);
        let [lw, _] = ui.calc_text_size("LAUNCHER");
        draw.add_text(
            [cx - lw / 2.0, under + 14.0 * dpi],
            rgba(c.muted, 1.0),
            "LAUNCHER",
        );
    }
}

/// The left column: who is playing, the seven destinations, and the way out.
///
/// It is opaque on every page. On the Play page it is the only thing painted over
/// the panorama, which is what gives that page its full-bleed look.
fn sidebar(
    ui: &Ui,
    model: &mut LauncherModel,
    fonts: &Fonts,
    c: &Css,
    dpi: f32,
    width: f32,
    height: f32,
    rise: f32,
    sink: &mut Sink<'_>,
) {
    let side = (width * SIDE_FRACTION).max(220.0 * dpi).min(width * 0.42);
    let pad = 20.0 * dpi;
    let face = FACE * dpi;
    let fy = 18.0 * dpi + rise;
    // Scoped: imgui allows one live `DrawListMut` per window, and the nav loop below
    // asks for its own.
    {
        let draw = ui.get_window_draw_list();
        fill(&draw, [0.0, rise], [side, height + rise], c.side, 1.0, 0.0);
        // The seam. On the Play page this is the edge between an opaque column and
        // the panorama, so it is drawn on every page rather than only on the opaque
        // ones.
        fill(
            &draw,
            [side - 1.0, rise],
            [side, height + rise],
            c.rule,
            1.0,
            0.0,
        );
        // The avatar: a square block face, because that is what a Minecraft player
        // recognises as themselves.
        fill(&draw, [pad, fy], [pad + face, fy + face], c.face(), 1.0, 2.0 * dpi);
        {
            let _f = ui.push_font(fonts.pixel);
            let initial: String = model
                .account
                .chars()
                .take(1)
                .collect::<String>()
                .to_uppercase();
            let initial = if initial.is_empty() {
                "P".to_string()
            } else {
                initial
            };
            let [iw, ih] = ui.calc_text_size(initial.as_str());
            draw.add_text(
                [pad + (face - iw) / 2.0, fy + (face - ih) / 2.0],
                rgba(c.on_accent, 1.0),
                initial.as_str(),
            );
        }
        // The name is the largest text in the column, and the account kind under it is
        // the quietest: the reference gives the player's own name that weight.
        {
            let _f = ui.push_font(fonts.pixel);
            draw.add_text(
                [pad + face + 11.0 * dpi, fy + 1.0 * dpi],
                rgba(c.ink, 1.0),
                fit(ui, &model.account, side - pad * 2.0 - face - 11.0 * dpi),
            );
        }
        {
            let _f = ui.push_font(fonts.small);
            draw.add_text(
                [pad + face + 11.0 * dpi, fy + 21.0 * dpi],
                rgba(c.muted, 1.0),
                "(Offline)",
            );
        }
    }
    // The whole header is the rename target, because it looks like the account.
    ui.set_cursor_screen_pos([pad, fy]);
    if ui.invisible_button("##account", [side - 2.0 * pad, face]) {
        if sink.live() {
            model.editing = true;
        }
        sink.push(LauncherEvent::RenameAccount);
    }

    // The destinations. Rows are fixed height and evenly spaced from the header, so
    // the column reads as a list rather than as a set of buttons that happen to be
    // stacked; the reference leaves the space below the last row empty.
    let row_h = NAV_H * dpi;
    let gap = NAV_GAP * dpi;
    let mut y = fy + face + 30.0 * dpi;
    // The glow eases by time rather than by frame, so it takes the same tenth of a
    // second whether the phone is at 60 Hz or 120.
    let dt = ui.io().delta_time;
    // Where the pointer was on the last frame. A row's hover starts from this rather
    // than from the row beside it, so the highlight fades in under the finger instead
    // of sliding across the column.
    let before = NAV_HOVER.with(|h| h.get());
    // Which row the pointer lands on this frame, learned below from the button that
    // owns it. The glow is drawn from the eased value and stepped at the end of the
    // loop, which is the one frame of lag that makes a hover fade rather than snap.
    let mut hovered: Option<LauncherPage> = None;
    for (i, page) in PAGES.iter().enumerate() {
        let page = *page;
        let selected = page == model.page
            // Settings is reached from Profile, so the column keeps Profile lit while
            // it is open rather than lighting nothing.
            || (page == LauncherPage::Profile && model.page == LauncherPage::Settings);
        let inset = NAV_INSET * dpi;
        let x = inset;
        let w = side - inset * 2.0;
        if y + row_h > height + rise {
            break;
        }
        // This row's glow as the last frame left it.
        let glow = NAV_GLOW.with(|g| g.get()[i]);
        {
            let draw = ui.get_window_draw_list();
            if selected {
                // The reference's selection is a green outline with a dark face, not a
                // filled green bar: the label stays white and the block is the mark.
                fill(&draw, [x, y], [x + w, y + row_h], c.raised, 1.0, 3.0 * dpi);
                stroke(&draw, [x, y], [x + w, y + row_h], c.accent, 1.0, 3.0 * dpi, 2.0);
            } else if glow > 0.002 {
                // A hover on a row that is not the current page: the same raised face as
                // the selection, at a fraction of its weight, so a finger resting on the
                // column can see where it would land.
                fill(
                    &draw,
                    [x, y],
                    [x + w, y + row_h],
                    c.raised,
                    glow * NAV_GLOW_ALPHA,
                    3.0 * dpi,
                );
            }
            let icon = 22.0 * dpi;
            nav_icon(&draw, page, x + 14.0 * dpi, y + row_h / 2.0, icon, c, selected);
            let _f = ui.push_font(fonts.pixel);
            let label_x = x + 14.0 * dpi + icon + 13.0 * dpi;
            draw.add_text(
                [label_x, y + row_h / 2.0 - 9.0 * dpi],
                rgba(if selected { c.ink } else { c.ink_soft }, 1.0),
                page.label(),
            );
        }
        ui.set_cursor_screen_pos([x, y]);
        let id = format!("##nav{i}");
        let clicked = ui.invisible_button(id.as_str(), [w, row_h]);
        // The button has just been laid out, so this is the row the pointer is on.
        if MOTION.with(|m| m.get()) && (ui.is_item_hovered() || ui.is_item_active()) {
            hovered = Some(page);
        }
        if clicked {
            if sink.live() {
                // Applied here, not on a round trip to the engine: the loading
                // surface redraws on demand, so the page the finger landed on has to
                // be the page drawn by the pass that is already under way.
                model.page = page;
            }
            sink.push(LauncherEvent::Goto(page));
        }
        y += row_h + gap;
    }
    // One eased step for the whole column, after the finger has been read: the row it
    // is on climbs, every other row falls back. With animations off `hovered` is
    // always `None`, so the glow drains and stays there.
    NAV_GLOW.with(|g| {
        let mut all = g.get();
        for (i, page) in PAGES.iter().enumerate() {
            let on = hovered == Some(*page);
            let target = if on { 1.0 } else { 0.0 };
            // A row the finger has just moved onto starts from rest: a hover is not a
            // continuation of the row beside it.
            let from = if on && before != Some(*page) {
                0.0
            } else {
                all[i]
            };
            all[i] = ease_glow(from, target, dt);
        }
        g.set(all);
    });
    NAV_HOVER.with(|h| h.set(hovered));

    // The footer: the build and the way out, on one line at the very bottom, which is
    // where the reference puts them and where a thumb in landscape cannot reach by
    // accident.
    let fy = height + rise - 26.0 * dpi;
    let quit_w = text_width(ui, fonts.small, "Quit") + 18.0 * dpi;
    {
        let draw = ui.get_window_draw_list();
        let _f = ui.push_font(fonts.small);
        draw.add_text(
            [pad, fy],
            rgba(c.muted, 1.0),
            fit(ui, &format!("Horizon {} · native", model.version), side - pad * 2.0 - quit_w),
        );
        draw.add_text(
            [side - pad - quit_w + 9.0 * dpi, fy],
            rgba(c.muted, 1.0),
            "Quit",
        );
    }
    ui.set_cursor_screen_pos([side - pad - quit_w, fy - 4.0 * dpi]);
    if ui.invisible_button("##quit", [quit_w, 22.0 * dpi]) {
        sink.push(LauncherEvent::Quit);
    }
}

/// A destination's mark, drawn as blocks rather than loaded as an image: the
/// palette is four colours and the shapes are a handful of rects each, which is
/// both smaller than a texture and exactly as sharp as the pixel font beside it.
fn nav_icon(draw: &DrawListMut<'_>, page: LauncherPage, x: f32, cy: f32, s: f32, c: &Css, lit: bool) {
    // A 16x16 grid, the size Minecraft's own icons are drawn on. Each mark is a few
    // rects placed on that grid, so it stays crisp at any dpi and costs no texture.
    let u = s / 16.0;
    let y = cy - s / 2.0;
    let px = |gx: f32, gy: f32, gw: f32, gh: f32| {
        [x + gx * u, y + gy * u, x + (gx + gw) * u, y + (gy + gh) * u]
    };
    let block = |gx: f32, gy: f32, gw: f32, gh: f32, col: u32| {
        let r = px(gx, gy, gw, gh);
        fill(draw, [r[0], r[1]], [r[2], r[3]], col, 1.0, 0.0);
    };
    let _ = (c, lit);
    match page {
        // A grass block seen at an angle: green lid, dirt body, a dark seam.
        LauncherPage::Play => {
            block(1.0, 2.0, 14.0, 5.0, 0x5bbf4a);
            block(1.0, 2.0, 14.0, 1.5, 0x74d95f);
            block(1.0, 7.0, 14.0, 7.0, 0x7a5433);
            block(2.0, 9.0, 3.0, 2.0, 0x63421f);
            block(9.0, 11.0, 4.0, 2.0, 0x63421f);
            block(5.0, 8.0, 2.0, 1.5, 0x8c6440);
        }
        // Three stacked crates with a lid on each.
        LauncherPage::Installations => {
            for i in 0..3 {
                let top = 1.0 + i as f32 * 5.0;
                block(2.0, top, 12.0, 4.0, 0x9a6a3a);
                block(2.0, top, 12.0, 1.2, 0xc08b52);
                block(7.0, top, 2.0, 4.0, 0x6f4824);
            }
        }
        // A player head: hair, skin, eyes, mouth.
        LauncherPage::Skins => {
            block(3.0, 2.0, 10.0, 11.0, 0xc79a72);
            block(3.0, 2.0, 10.0, 3.0, 0x4a3423);
            block(5.0, 6.0, 2.0, 2.0, 0x2b2b30);
            block(9.0, 6.0, 2.0, 2.0, 0x2b2b30);
            block(6.0, 10.0, 4.0, 1.2, 0x8a5a44);
        }
        // A page with ruled lines and a turned corner.
        LauncherPage::PatchNotes => {
            block(3.0, 1.0, 10.0, 14.0, 0xd9d3c6);
            block(10.0, 1.0, 3.0, 3.0, 0x9c968a);
            block(5.0, 6.0, 6.0, 1.0, 0x6d6759);
            block(5.0, 8.5, 6.0, 1.0, 0x6d6759);
            block(5.0, 11.0, 4.0, 1.0, 0x6d6759);
        }
        // A folded newspaper.
        LauncherPage::News => {
            block(1.0, 3.0, 14.0, 10.0, 0xd9d3c6);
            block(3.0, 5.0, 5.0, 4.0, 0x8c8578);
            block(9.0, 5.0, 4.0, 1.0, 0x8c8578);
            block(9.0, 7.0, 4.0, 1.0, 0x8c8578);
            block(9.0, 9.0, 4.0, 1.0, 0x8c8578);
            block(3.0, 11.0, 10.0, 1.0, 0x8c8578);
        }
        // A pickaxe: handle and head.
        LauncherPage::Mods => {
            block(7.0, 5.0, 2.0, 10.0, 0x8a6a45);
            block(2.0, 2.0, 12.0, 2.0, 0x9aa0a8);
            block(2.0, 2.0, 2.0, 5.0, 0x9aa0a8);
            block(12.0, 2.0, 2.0, 5.0, 0x9aa0a8);
        }
        // A player head, the same one the account uses.
        LauncherPage::Profile => {
            block(2.0, 2.0, 12.0, 12.0, 0xc79a72);
            block(2.0, 2.0, 12.0, 3.0, 0x4a3423);
            block(4.5, 6.5, 2.0, 2.0, 0x2b2b30);
            block(9.5, 6.5, 2.0, 2.0, 0x2b2b30);
            block(6.0, 10.0, 4.0, 1.2, 0x8a5a44);
        }
        LauncherPage::Settings => {
            block(2.0, 6.0, 12.0, 4.0, 0x9aa0a8);
            block(6.0, 3.0, 4.0, 10.0, 0x9aa0a8);
        }
        // A folder: a tab on the lid and a body.
        LauncherPage::Files => {
            block(2.0, 3.0, 6.0, 2.0, 0xc0a24a);
            block(2.0, 5.0, 12.0, 9.0, 0xd8b95c);
            block(2.0, 7.0, 12.0, 1.0, 0xb99a3f);
        }
        // A signal: two arcs and a dot, the mark a host wears.
        LauncherPage::Multiplayer => {
            block(7.0, 9.0, 2.0, 2.0, 0x5bbf4a);
            block(5.0, 6.0, 1.5, 1.5, 0x5bbf4a);
            block(9.5, 6.0, 1.5, 1.5, 0x5bbf4a);
            block(3.5, 4.0, 1.5, 1.5, 0x5bbf4a);
            block(11.0, 4.0, 1.5, 1.5, 0x5bbf4a);
            block(6.0, 8.0, 4.0, 1.0, 0x5bbf4a);
        }
    }
}

/// The Play page: the panorama, a scrim so text reads over it, the version
/// dropdown, and the one button that starts the game.
#[allow(clippy::too_many_arguments)]
fn play_page(
    ui: &Ui,
    model: &LauncherModel,
    fonts: &Fonts,
    c: &Css,
    dpi: f32,
    g: &Grid,
    height: f32,
    rise: f32,
    over_panorama: bool,
    sink: &mut Sink<'_>,
) {
    let playable = model.playable && !model.instance.is_empty();
    {
        let draw = ui.get_window_draw_list();
        // The wordmark sits over the panorama's own art, in the middle of the page and
        // above the scrim, which is where the reference puts it: it is the one element
        // that says what the page is, and it needs the least competition.
        {
            let _f = ui.push_font(fonts.title);
            let [tw, th] = ui.calc_text_size("MINECRAFT");
            let cx = g.x + (g.w - tw) / 2.0;
            let cy = height * 0.16 + rise;
            // A drop shadow first, offset down, so the letters read over a bright sky.
            draw.add_text([cx + 3.0 * dpi, cy + 4.0 * dpi], rgba(0x000000, 0.55), "MINECRAFT");
            draw.add_text([cx, cy], rgba(0xffffff, 1.0), "MINECRAFT");
            {
                let _g = ui.push_font(fonts.small);
                let sub = "JAVA EDITION";
                let [sw, _] = ui.calc_text_size(sub);
                draw.add_text(
                    [g.x + (g.w - sw) / 2.0, cy + th + 4.0 * dpi],
                    rgba(0xd8d8d8, 0.95),
                    sub,
                );
            }
        }
    }
    {
        let draw = ui.get_window_draw_list();
        // Over the panorama this is a scrim: the art stays visible at the top where the
        // title is, and the bottom goes dark enough for a green button and a line of
        // text. Without a panorama there is nothing to keep, so it is a wash of the
        // page's own colour at the same weight.
        //
        // One quad, not a stack of bands. imgui interpolates the four corner colours
        // across the rectangle in the shader, so the ramp is continuous at every pixel;
        // the bands this replaces were fifteen separate rectangles, and each seam
        // between two of them was a visible stripe with a gap under it.
        let top = height * 0.26;
        let bottom = height + rise;
        let (top_a, mid_a) = if over_panorama { (0.0, 0.72) } else { (0.42, 0.86) };
        let mid = top + (bottom - top) * 0.45;
        let clear = rgba(c.page, 0.0);
        // Upper half: clear to the scrim's weight.
        draw.add_rect_filled_multicolor(
            [g.x, top],
            [g.x + g.w, mid],
            clear,
            clear,
            rgba(c.page, mid_a),
            rgba(c.page, mid_a),
        );
        // Lower half: the scrim's weight to nearly solid at the bottom edge.
        draw.add_rect_filled_multicolor(
            [g.x, mid],
            [g.x + g.w, bottom],
            rgba(c.page, mid_a),
            rgba(c.page, mid_a),
            rgba(c.page, 0.96),
            rgba(c.page, 0.96),
        );
        if !over_panorama {
            fill(&draw, [g.x, 0.0], [g.x + g.w, top], c.page, top_a, 0.0);
        }
        // The soft edge under the column: one gradient from the column's own face to
        // nothing, so the seam reads as a shadow rather than as a cut with steps in it.
        let fade = 120.0 * dpi;
        draw.add_rect_filled_multicolor(
            [g.x, 0.0],
            [g.x + fade, height + rise],
            rgba(c.page, 0.55),
            rgba(c.page, 0.0),
            rgba(c.page, 0.0),
            rgba(c.page, 0.55),
        );
    }

    // Top right: the two things a player reaches for besides Play. They are pinned to
    // the page's top-right corner in the reference, not to the content column, which
    // is why they sit above the art rather than beside the title.
    let bw = 108.0 * dpi;
    let bh = 32.0 * dpi;
    let by = 12.0 * dpi + rise;
    let mut bx = g.x + g.w - g.pad - bw;
    if raised_button(ui, fonts, c, dpi, bx, by, bw, bh, "MULTIPLAYER", c.accent, c.accent_lip, c.on_accent) {
        // Horizon's own multiplayer: hosting this device's world. The game's server
        // list is for joining somebody else's server and is reached from inside the
        // game, not from the launcher's front page.
        sink.push(LauncherEvent::Goto(LauncherPage::Multiplayer));
    }
    bx -= bw + 8.0 * dpi;
    if raised_button(ui, fonts, c, dpi, bx, by, bw, bh, "FILES", c.button, c.bevel, c.ink) {
        sink.push(LauncherEvent::Goto(LauncherPage::Files));
    }

    // Bottom left: which installation Play would start. The reference's card is dark
    // with the label small and the version large, and a chevron on the right.
    let dw = DROP_W * dpi;
    let dh = DROP_H * dpi;
    let dx = g.x + g.pad;
    let dy = height + rise - g.pad - dh;
    {
        let draw = ui.get_window_draw_list();
        panel(&draw, c, [dx, dy], [dx + dw, dy + dh], c.panel, 3.0 * dpi);
        {
            let _f = ui.push_font(fonts.small);
            draw.add_text(
                [dx + 14.0 * dpi, dy + 11.0 * dpi],
                rgba(c.muted, 1.0),
                "Latest Release",
            );
        }
        {
            let _f = ui.push_font(fonts.pixel);
            draw.add_text(
                [dx + 14.0 * dpi, dy + 30.0 * dpi],
                rgba(c.ink, 1.0),
                fit(ui, &model.version, dw - 56.0 * dpi),
            );
        }
        // A chevron, drawn as two strokes rather than a glyph.
        let cx = dx + dw - 22.0 * dpi;
        let cy = dy + dh / 2.0;
        draw.add_line([cx - 4.5 * dpi, cy - 2.0 * dpi], [cx, cy + 3.5 * dpi], rgba(c.muted, 1.0))
            .thickness(2.0)
            .build();
        draw.add_line([cx, cy + 3.5 * dpi], [cx + 4.5 * dpi, cy - 2.0 * dpi], rgba(c.muted, 1.0))
            .thickness(2.0)
            .build();
    }
    ui.set_cursor_screen_pos([dx, dy]);
    if ui.invisible_button("##version", [dw, dh]) {
        sink.push(LauncherEvent::Goto(LauncherPage::Installations));
    }

    // The status line and the download bar sit above the button, where a player
    // looking at Play cannot miss them.
    let btn_w = PLAY_W * dpi;
    let btn_h = PLAY_H * dpi;
    let btn_y = height + rise - g.pad - btn_h;
    let btn_x = g.right(btn_w);
    if !model.status.is_empty() || model.progress.is_some() {
        let text_w = (btn_x - dx - dw - g.pad * 2.0).max(120.0 * dpi);
        let draw = ui.get_window_draw_list();
        if !model.status.is_empty() {
            draw.add_text(
                [dx, btn_y - 24.0 * dpi],
                rgba(c.ink_soft, 1.0),
                fit(ui, &model.status, text_w),
            );
        }
        if let Some(p) = model.progress {
            let p = p.clamp(0.0, 1.0);
            let (l, r) = (dx, dx + text_w.min(420.0 * dpi));
            let t = btn_y - 8.0 * dpi;
            fill(&draw, [l, t], [r, t + 4.0 * dpi], c.well, 1.0, 2.0 * dpi);
            if p > 0.0 {
                fill(&draw, [l, t], [l + (r - l) * p, t + 4.0 * dpi], c.accent, 1.0, 2.0 * dpi);
            }
        }
    }

    let label = match &model.front {
        Some(text) => text.clone(),
        None if playable => "PLAY".to_string(),
        None => "INSTALL".to_string(),
    };
    let face = if playable { c.accent } else { c.button };
    let lip = if playable { c.accent_lip } else { c.bevel };
    let ink = if playable { c.on_accent } else { c.muted };
    if raised_button(ui, fonts, c, dpi, btn_x, btn_y, btn_w, btn_h, &label, face, lip, ink) {
        sink.push(match model.front.as_deref() {
            Some(_) => LauncherEvent::Restart,
            None if playable => LauncherEvent::Play,
            None => LauncherEvent::Install,
        });
    }
}

/// A raised button: the face, a light top edge, a dark lip, and the pixel face
/// centred on it. Returns whether it was released under the finger this frame.
///
/// The widget is laid out first and painted second. That order is the whole point: a
/// face that drops under a finger cannot be drawn before the widget that knows the
/// finger is there, and `invisible_button` is what tells imgui the rectangle exists.
/// The click it returns is the click every caller already acts on.
#[allow(clippy::too_many_arguments)]
fn raised_button(
    ui: &Ui,
    fonts: &Fonts,
    c: &Css,
    dpi: f32,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    label: &str,
    face: u32,
    lip: u32,
    ink: u32,
) -> bool {
    ui.set_cursor_screen_pos([x, y]);
    let clicked = ui.invisible_button(label, [w, h]);
    // Under a finger the face sits a couple of pixels lower and loses its bevel: that
    // is a press, where a still face is only a hover. With animations off the press is
    // a tap that lands between frames, so there would be nothing to see anyway.
    let down = MOTION.with(|m| m.get()) && ui.is_item_active();
    let drop = if down { PRESS_DROP * dpi } else { 0.0 };
    let top = y + drop;
    {
        // Scoped: `raised_face` asks for the same draw list, and imgui allows one live
        // borrow of it per window.
        let draw = ui.get_window_draw_list();
        raised_face(
            &draw,
            [x, top],
            [x + w, top + h],
            if down { darker(face) } else { face },
            c.bevel,
            lip,
            4.0 * dpi,
            dpi,
            !down,
        );
        {
            let _f = ui.push_font(fonts.pixel);
            let [tw, th] = ui.calc_text_size(label);
            draw.add_text(
                [x + (w - tw) / 2.0, top + (h - th) / 2.0 - 1.0 * dpi],
                rgba(ink, 1.0),
                label,
            );
        }
    }
    clicked
}

/// The page's heading, and the y its content starts at. Every page begins on the
/// same line so moving between them does not make the interface jump.
fn heading(
    ui: &Ui,
    fonts: &Fonts,
    c: &Css,
    dpi: f32,
    g: &Grid,
    title: &str,
    rise: f32,
) -> f32 {
    let y = 26.0 * dpi + rise;
    let draw = ui.get_window_draw_list();
    {
        let _f = ui.push_font(fonts.title);
        draw.add_text([g.x + g.pad, y], rgba(c.ink, 1.0), title);
    }
    y + 56.0 * dpi
}

fn installations_page(
    ui: &Ui,
    model: &LauncherModel,
    fonts: &Fonts,
    c: &Css,
    dpi: f32,
    g: &Grid,
    height: f32,
    rise: f32,
    sink: &mut Sink<'_>,
) {
    let mut y = heading(ui, fonts, c, dpi, g, "INSTALLATIONS", rise);
    let btn_w = 118.0 * dpi;
    let btn_h = 42.0 * dpi;
    let mut bx = g.x + g.pad;
    if raised_button(ui, fonts, c, dpi, bx, y, btn_w, btn_h, "+ New...", c.button, c.bevel, c.ink) {
        sink.push(LauncherEvent::NewInstance);
    }
    bx += btn_w + 10.0 * dpi;
    if raised_button(ui, fonts, c, dpi, bx, y, btn_w + 26.0 * dpi, btn_h, "+ Modpack", c.button, c.bevel, c.ink) {
        sink.push(LauncherEvent::NewInstance);
    }
    // FILES, here as well as on the front page: this is the page a player lands on
    // when they want to see what the install actually put on their phone, so the
    // button that opens the folder belongs beside the list of what is in it.
    bx += btn_w + 36.0 * dpi;
    if raised_button(ui, fonts, c, dpi, bx, y, btn_w, btn_h, "FILES", c.button, c.bevel, c.ink) {
        sink.push(LauncherEvent::RevealFiles);
    }
    y += btn_h + 22.0 * dpi;

    // The version filter. These are labels on this build: the engine plays one
    // version, so a filter that promised snapshots would be a control that does
    // nothing, which is worse than not drawing it. What is here says what the build
    // is instead.
    {
        let draw = ui.get_window_draw_list();
        let _f = ui.push_font(fonts.small);
        draw.add_text(
            [g.x + g.pad, y + 4.0 * dpi],
            rgba(c.muted, 1.0),
            "One version per build: the engine's protocol, registry and block tables are \
             generated for it. Every installation below is that version.",
        );
    }
    y += 36.0 * dpi;

    let row_h = 68.0 * dpi;
    if model.rows.is_empty() {
        let h = 96.0 * dpi;
        let draw = ui.get_window_draw_list();
        panel(&draw, c, [g.x + g.pad, y], [g.x + g.w - g.pad, y + h], c.panel, 4.0 * dpi);
        {
            let _f = ui.push_font(fonts.pixel);
            let [tw, _] = ui.calc_text_size("NO INSTALLATIONS FOUND");
            draw.add_text(
                [g.x + (g.w - tw) / 2.0, y + 26.0 * dpi],
                rgba(c.ink, 1.0),
                "NO INSTALLATIONS FOUND",
            );
        }
        let sub = "Click '+ New...' above to download and install Minecraft.";
        let [sw, _] = ui.calc_text_size(sub);
        draw.add_text(
            [g.x + (g.w - sw) / 2.0, y + 58.0 * dpi],
            rgba(c.muted, 1.0),
            sub,
        );
        return;
    }
    for row in &model.rows {
        if y + row_h > height + rise {
            break;
        }
        row_card(ui, fonts, c, dpi, g, y, row_h, row, sink);
        y += row_h + 10.0 * dpi;
    }
    if !model.status.is_empty() && y + 18.0 * dpi <= height + rise {
        let draw = ui.get_window_draw_list();
        let _f = ui.push_font(fonts.small);
        for (i, line) in wrap(ui, &model.status, g.w - 2.0 * g.pad).iter().enumerate() {
            draw.add_text(
                [g.x + g.pad, y + i as f32 * 17.0 * dpi],
                rgba(c.muted, 1.0),
                line.as_str(),
            );
        }
    }
}

/// Files: what the install actually put on the phone, in the one folder a file
/// manager can reach. A phone has no `open folder` dialog, so this lists the
/// subfolders the game reads and offers the system picker for the rest.
fn files_page(
    ui: &Ui,
    model: &LauncherModel,
    fonts: &Fonts,
    c: &Css,
    dpi: f32,
    g: &Grid,
    height: f32,
    rise: f32,
    sink: &mut Sink<'_>,
) {
    let mut y = heading(ui, fonts, c, dpi, g, "FILES", rise);
    {
        let draw = ui.get_window_draw_list();
        let _f = ui.push_font(fonts.small);
        let blurb = "The folder the game reads, where a file manager can reach it.";
        draw.add_text([g.x + g.pad, y - 20.0 * dpi], rgba(c.muted, 1.0), blurb);
    }
    // The four things a player does with files, each a real system dialog: import
    // opens the picker and copies the result where the game reads it, export copies a
    // folder out where a file manager can reach it. These used to be a toast naming a
    // path, which is not a button a phone can act on.
    let btn_h = 42.0 * dpi;
    let btn_w = 176.0 * dpi;
    let gap = 10.0 * dpi;
    let mut bx = g.x + g.pad;
    if raised_button(ui, fonts, c, dpi, bx, y, btn_w, btn_h, "IMPORT SKIN", c.accent, c.accent_lip, c.on_accent) {
        sink.push(LauncherEvent::ImportFiles("skin".into()));
    }
    bx += btn_w + gap;
    if raised_button(ui, fonts, c, dpi, bx, y, btn_w, btn_h, "IMPORT MOD", c.accent, c.accent_lip, c.on_accent) {
        sink.push(LauncherEvent::ImportFiles("mod".into()));
    }
    bx += btn_w + gap;
    if raised_button(ui, fonts, c, dpi, bx, y, btn_w, btn_h, "IMPORT PACK", c.button, c.bevel, c.ink) {
        sink.push(LauncherEvent::ImportFiles("resourcepack".into()));
    }
    bx += btn_w + gap;
    if raised_button(ui, fonts, c, dpi, bx, y, btn_w, btn_h, "IMPORT SETTINGS", c.button, c.bevel, c.ink) {
        sink.push(LauncherEvent::ImportFiles("settings".into()));
    }
    y += btn_h + gap;

    let mut bx = g.x + g.pad;
    if raised_button(ui, fonts, c, dpi, bx, y, btn_w, btn_h, "EXPORT SKINS", c.button, c.bevel, c.ink) {
        sink.push(LauncherEvent::ExportFiles("skins".into()));
    }
    bx += btn_w + gap;
    if raised_button(ui, fonts, c, dpi, bx, y, btn_w, btn_h, "EXPORT MODS", c.button, c.bevel, c.ink) {
        sink.push(LauncherEvent::ExportFiles("mods".into()));
    }
    bx += btn_w + gap;
    if raised_button(ui, fonts, c, dpi, bx, y, btn_w, btn_h, "EXPORT SETTINGS", c.button, c.bevel, c.ink) {
        sink.push(LauncherEvent::ExportSettings);
    }
    bx += btn_w + gap;
    if raised_button(ui, fonts, c, dpi, bx, y, btn_w, btn_h, "OPEN FOLDER", c.button, c.bevel, c.ink) {
        sink.push(LauncherEvent::RevealFiles);
    }
    y += btn_h + 22.0 * dpi;

    let row_h = 56.0 * dpi;
    if model.rows.is_empty() {
        let draw = ui.get_window_draw_list();
        let _f = ui.push_font(fonts.small);
        draw.add_text(
            [g.x + g.pad, y],
            rgba(c.muted, 1.0),
            "Nothing here yet. The install fills this in.",
        );
        return;
    }
    for row in &model.rows {
        if y + row_h > height + rise {
            break;
        }
        let x = g.x + g.pad;
        let w = g.w - 2.0 * g.pad;
        let draw = ui.get_window_draw_list();
        panel(&draw, c, [x, y], [x + w, y + row_h], c.panel, 3.0 * dpi);
        {
            let _f = ui.push_font(fonts.pixel);
            draw.add_text(
                [x + 16.0 * dpi, y + 10.0 * dpi],
                rgba(c.ink, 1.0),
                fit(ui, &row.title, w - 32.0 * dpi),
            );
        }
        {
            let _f = ui.push_font(fonts.small);
            draw.add_text(
                [x + 16.0 * dpi, y + 32.0 * dpi],
                rgba(c.muted, 1.0),
                fit(ui, &row.subtitle, w - 32.0 * dpi),
            );
        }
        y += row_h + 8.0 * dpi;
    }
}

/// Horizon's own multiplayer: hosting this device's world for friends on the same
/// network, and the address they type to join. The game's server list is a different
/// thing — it is for joining somebody else's server — and the front page's button
/// used to open that by mistake.
fn multiplayer_page(
    ui: &Ui,
    model: &LauncherModel,
    fonts: &Fonts,
    c: &Css,
    dpi: f32,
    g: &Grid,
    height: f32,
    rise: f32,
    sink: &mut Sink<'_>,
) {
    let mut y = heading(ui, fonts, c, dpi, g, "MULTIPLAYER", rise);
    {
        let draw = ui.get_window_draw_list();
        let _f = ui.push_font(fonts.small);
        draw.add_text(
            [g.x + g.pad, y - 20.0 * dpi],
            rgba(c.muted, 1.0),
            "Host a world for friends on this network, then give them the address below.",
        );
    }
    // The state: hosting an address, or not.
    let card_h = 92.0 * dpi;
    {
        let draw = ui.get_window_draw_list();
        panel(&draw, c, [g.x + g.pad, y], [g.x + g.w - g.pad, y + card_h], c.panel, 4.0 * dpi);
        {
            let _f = ui.push_font(fonts.small);
            draw.add_text([g.x + g.pad + 18.0 * dpi, y + 16.0 * dpi], rgba(c.muted, 1.0), "THIS DEVICE");
        }
        {
            let _f = ui.push_font(fonts.pixel);
            let line = model.hosting.as_deref().unwrap_or("Not hosting");
            draw.add_text(
                [g.x + g.pad + 18.0 * dpi, y + 38.0 * dpi],
                rgba(if model.hosting.is_some() { c.accent } else { c.ink }, 1.0),
                fit(ui, line, g.w - 2.0 * g.pad - 36.0 * dpi),
            );
        }
        {
            let _f = ui.push_font(fonts.small);
            draw.add_text(
                [g.x + g.pad + 18.0 * dpi, y + 64.0 * dpi],
                rgba(c.muted, 1.0),
                match &model.hosting {
                    Some(_) => "Friends on the same network can join with the address above.",
                    None => "Open a world first, then press Host. The world stays on this device.",
                },
            );
        }
    }
    y += card_h + 16.0 * dpi;

    let btn_w = 200.0 * dpi;
    let btn_h = 44.0 * dpi;
    if model.hosting.is_some() {
        if raised_button(ui, fonts, c, dpi, g.x + g.pad, y, btn_w, btn_h, "STOP HOSTING", c.button, c.bevel, c.ink) {
            sink.push(LauncherEvent::StopHosting);
        }
    } else if raised_button(ui, fonts, c, dpi, g.x + g.pad, y, btn_w, btn_h, "HOST THIS WORLD", c.accent, c.accent_lip, c.on_accent) {
        // The first world row is the one the launcher can open; the platform decides
        // which folder that is, so this asks it rather than naming one here.
        sink.push(LauncherEvent::HostWorld(0));
    }
    y += btn_h + 24.0 * dpi;

    // The worlds that can be hosted, and the fact that the engine's own list is a
    // different screen for a different job.
    if y + 20.0 * dpi <= height + rise {
        let draw = ui.get_window_draw_list();
        let _f = ui.push_font(fonts.small);
        draw.add_text([g.x + g.pad, y], rgba(c.muted, 1.0), "WORLDS ON THIS DEVICE");
        y += 26.0 * dpi;
    }
    let row_h = 56.0 * dpi;
    for row in &model.rows {
        if y + row_h > height + rise {
            break;
        }
        let x = g.x + g.pad;
        let w = g.w - 2.0 * g.pad;
        // Scoped: the Host button below asks for its own draw list.
        {
            let draw = ui.get_window_draw_list();
            panel(&draw, c, [x, y], [x + w, y + row_h], c.panel, 3.0 * dpi);
            {
                let _f = ui.push_font(fonts.pixel);
                draw.add_text(
                    [x + 16.0 * dpi, y + 10.0 * dpi],
                    rgba(c.ink, 1.0),
                    fit(ui, &row.title, w - 200.0 * dpi),
                );
            }
            {
                let _f = ui.push_font(fonts.small);
                draw.add_text(
                    [x + 16.0 * dpi, y + 32.0 * dpi],
                    rgba(c.muted, 1.0),
                    fit(ui, &row.subtitle, w - 200.0 * dpi),
                );
            }
        }
        let bw = 120.0 * dpi;
        if raised_button(
            ui,
            fonts,
            c,
            dpi,
            x + w - 16.0 * dpi - bw,
            y + (row_h - 32.0 * dpi) / 2.0,
            bw,
            32.0 * dpi,
            "Host",
            c.button,
            c.bevel,
            c.ink,
        ) {
            sink.push(LauncherEvent::HostWorld(row.id));
        }
        y += row_h + 8.0 * dpi;
    }
    if model.rows.is_empty() {
        let draw = ui.get_window_draw_list();
        let _f = ui.push_font(fonts.small);
        draw.add_text(
            [g.x + g.pad, y],
            rgba(c.muted, 1.0),
            "No worlds yet. Play once to make one.",
        );
    }
}

/// One installation row: a block tile, the name, what it is, and its tag. The whole
/// row is the hit target, and it is one rect, so the thing that looks tappable and
/// the thing that answers are the same.
fn row_card(
    ui: &Ui,
    fonts: &Fonts,
    c: &Css,
    dpi: f32,
    g: &Grid,
    y: f32,
    h: f32,
    row: &LauncherRow,
    sink: &mut Sink<'_>,
) {
    let x = g.x + g.pad;
    let w = g.w - 2.0 * g.pad;
    let draw = ui.get_window_draw_list();
    panel(
        &draw,
        c,
        [x, y],
        [x + w, y + h],
        if row.selected { c.raised } else { c.panel },
        4.0 * dpi,
    );
    let tile = 40.0 * dpi;
    fill(
        &draw,
        [x + 14.0 * dpi, y + (h - tile) / 2.0],
        [x + 14.0 * dpi + tile, y + (h + tile) / 2.0],
        c.well,
        1.0,
        3.0 * dpi,
    );
    let badge_w = if row.badge.is_empty() {
        0.0
    } else {
        text_width(ui, fonts.small, &row.badge) + 22.0 * dpi
    };
    let text_x = x + 14.0 * dpi + tile + 16.0 * dpi;
    let text_w = (x + w - 16.0 * dpi - badge_w - text_x).max(0.0);
    {
        let _f = ui.push_font(fonts.pixel);
        draw.add_text(
            [text_x, y + 14.0 * dpi],
            rgba(if row.enabled { c.ink } else { c.muted }, 1.0),
            fit(ui, &row.title, text_w),
        );
    }
    draw.add_text(
        [text_x, y + 38.0 * dpi],
        rgba(c.muted, 1.0),
        fit(ui, &row.subtitle, text_w),
    );
    if !row.badge.is_empty() {
        let bx = x + w - 16.0 * dpi - badge_w;
        fill(
            &draw,
            [bx, y + h / 2.0 - 12.0 * dpi],
            [bx + badge_w, y + h / 2.0 + 12.0 * dpi],
            c.well,
            1.0,
            3.0 * dpi,
        );
        let _f = ui.push_font(fonts.small);
        draw.add_text(
            [bx + 11.0 * dpi, y + h / 2.0 - 7.0 * dpi],
            rgba(c.muted, 1.0),
            &row.badge,
        );
    }
    ui.set_cursor_screen_pos([x, y]);
    let id = format!("##row{}", row.id);
    if ui.invisible_button(id.as_str(), [w, h]) && row.enabled {
        sink.push(LauncherEvent::SelectRow(row.id));
    }
}

/// Skins: the player on the left, the wardrobe on the right. The figure is drawn
/// from the chosen PNG's own pixels, so what the page shows is the skin, not a
/// stand-in for it.
#[allow(clippy::too_many_arguments)]
fn skins_page(
    ui: &Ui,
    model: &LauncherModel,
    fonts: &Fonts,
    c: &Css,
    dpi: f32,
    g: &Grid,
    height: f32,
    rise: f32,
    skin: Option<TextureId>,
    sink: &mut Sink<'_>,
) {
    let y = heading(ui, fonts, c, dpi, g, "SKINS & WARDROBE", rise);
    let avail_h = height + rise - y - g.pad;
    let side_by_side = g.w > 900.0 * dpi;
    let model_w = if side_by_side {
        (g.w * 0.34).max(220.0 * dpi)
    } else {
        g.w - 2.0 * g.pad
    };
    let model_h = if side_by_side {
        avail_h
    } else {
        (avail_h * 0.45).max(160.0 * dpi)
    };
    let mx = g.x + g.pad;
    {
        let draw = ui.get_window_draw_list();
        panel(&draw, c, [mx, y], [mx + model_w, y + model_h], c.panel, 4.0 * dpi);
        // The figure: the skin's own pixels when there is one, the flat figure only
        // when there is nothing to sample. This is the difference between a preview
        // and a picture of a player.
        let figure_h = (model_h - 110.0 * dpi).max(80.0 * dpi);
        match skin {
            Some(texture) => skin_figure(
                ui,
                &draw,
                texture,
                model.skin_size,
                mx + model_w / 2.0,
                y + 26.0 * dpi,
                figure_h,
                model.skin_yaw,
            ),
            None => player_figure(
                &draw,
                c,
                dpi,
                mx + model_w / 2.0,
                y + model_h * 0.46,
                (model_h * 0.34).min(model_w * 0.5),
            ),
        }
        {
            let _f = ui.push_font(fonts.pixel);
            let name = fit(ui, &model.account, model_w - 32.0 * dpi);
            let [nw, _] = ui.calc_text_size(name.as_str());
            draw.add_text(
                [mx + (model_w - nw) / 2.0, y + model_h - 62.0 * dpi],
                rgba(c.ink, 1.0),
                name.as_str(),
            );
        }
        let _f = ui.push_font(fonts.small);
        let line = match &model.chosen_skin {
            Some(name) => format!("{name} · the skin in game"),
            None => "No skin chosen · drop a PNG in import/skins".to_string(),
        };
        let line = fit(ui, &line, model_w - 32.0 * dpi);
        let [lw, _] = ui.calc_text_size(line.as_str());
        draw.add_text(
            [mx + (model_w - lw) / 2.0, y + model_h - 36.0 * dpi],
            rgba(if model.chosen_skin.is_some() { c.accent } else { c.muted }, 1.0),
            line.as_str(),
        );
    }

    let wx = if side_by_side {
        mx + model_w + g.pad
    } else {
        g.x + g.pad
    };
    let ww = if side_by_side {
        g.x + g.w - g.pad - wx
    } else {
        g.w - 2.0 * g.pad
    };
    let mut wy = if side_by_side {
        y
    } else {
        y + model_h + g.pad
    };

    // The way in: the system picker, which copies the PNG into import/skins. This
    // stays in the launcher — handing the player to the game's own skin screen is a
    // different page in a different program, and the file would not land where this
    // page lists it.
    let seg_h = 44.0 * dpi;
    if wy + seg_h <= height + rise
        && raised_button(
            ui,
            fonts,
            c,
            dpi,
            wx,
            wy,
            ww,
            seg_h,
            "+ BROWSE CUSTOM SKIN (PNG)",
            c.accent,
            c.accent_lip,
            c.on_accent,
        )
    {
        sink.push(LauncherEvent::OpenSkins);
    }
    wy += seg_h + 20.0 * dpi;

    // The library: every PNG in import/skins, drawn with the PNG itself, so the grid
    // shows the skins rather than their file names.
    if wy + 20.0 * dpi <= height + rise {
        let draw = ui.get_window_draw_list();
        let _f = ui.push_font(fonts.small);
        draw.add_text([wx, wy], rgba(c.muted, 1.0), "SKIN LIBRARY");
        wy += 26.0 * dpi;
    }
    let cell_w = 104.0 * dpi;
    let cell_h = 124.0 * dpi;
    let mut cx = wx;
    let mut cy = wy;
    for name in &model.skins {
        if cx + cell_w > wx + ww {
            cx = wx;
            cy += cell_h + 10.0 * dpi;
        }
        if cy + cell_h > height + rise {
            break;
        }
        let chosen = model.chosen_skin.as_deref() == Some(name.as_str());
        let draw = ui.get_window_draw_list();
        fill(&draw, [cx, cy], [cx + cell_w, cy + cell_h], c.panel, 1.0, 3.0 * dpi);
        stroke(
            &draw,
            [cx, cy],
            [cx + cell_w, cy + cell_h],
            if chosen { c.accent } else { c.rule },
            1.0,
            3.0 * dpi,
            if chosen { 2.0 } else { 1.0 },
        );
        fill(
            &draw,
            [cx + 14.0 * dpi, cy + 12.0 * dpi],
            [cx + cell_w - 14.0 * dpi, cy + cell_h - 40.0 * dpi],
            c.well,
            1.0,
            2.0 * dpi,
        );
        // The thumbnail is the chosen skin's own head when it is this one; for the
        // others the page has no texture of theirs uploaded, so the file name stands
        // in and the tap applies it — the preview above is what confirms it.
        if chosen && let Some(texture) = skin {
            let (w, h) = (model.skin_size[0] as f32, model.skin_size[1] as f32);
            let (tw, th) = (cell_w - 28.0 * dpi, cell_h - 52.0 * dpi);
            draw.add_image(
                texture,
                [cx + 14.0 * dpi, cy + 12.0 * dpi],
                [cx + 14.0 * dpi + tw, cy + 12.0 * dpi + th],
            )
            .uv_min([8.0 / w, 8.0 / h])
            .uv_max([16.0 / w, 16.0 / h])
            .build();
        }
        {
            let _f = ui.push_font(fonts.small);
            let label = name.trim_end_matches(".png");
            let label = fit(ui, label, cell_w - 16.0 * dpi);
            let [tw, _] = ui.calc_text_size(label.as_str());
            draw.add_text(
                [cx + (cell_w - tw) / 2.0, cy + cell_h - 30.0 * dpi],
                rgba(if chosen { c.accent } else { c.ink_soft }, 1.0),
                label.as_str(),
            );
        }
        ui.set_cursor_screen_pos([cx, cy]);
        let id = format!("##skin{name}");
        if ui.invisible_button(id.as_str(), [cell_w, cell_h]) {
            sink.push(LauncherEvent::PickSkin(name.clone()));
        }
        cx += cell_w + 10.0 * dpi;
    }
    if model.skins.is_empty() {
        let draw = ui.get_window_draw_list();
        let _f = ui.push_font(fonts.small);
        let line = "No PNGs in import/skins yet. Drop a 64x64 skin there and reopen this page.";
        for (i, part) in wrap(ui, line, ww).iter().enumerate() {
            draw.add_text(
                [wx, cy + 8.0 * dpi + i as f32 * 18.0 * dpi],
                rgba(c.muted, 1.0),
                part.as_str(),
            );
        }
    }
}

/// The player, drawn as the real model: a head, a body, two arms and two legs, each
/// a box with the skin's own pixels on every face the model shows.
///
/// This is a 3D projection rather than a stack of rectangles. Each part is a cube in
/// model space, its eight corners are rotated by the page's own yaw and tilted by a
/// fixed pitch, and each visible face is a textured quad with the UVs Minecraft's
/// model uses for that face. A cube's faces are painted back to front, so the near
/// ones cover the far ones the way a real model does.
#[allow(clippy::too_many_arguments)]
fn skin_figure(
    ui: &Ui,
    draw: &DrawListMut<'_>,
    texture: TextureId,
    size: [u32; 2],
    cx: f32,
    top: f32,
    height: f32,
    yaw: f32,
) {
    let (w, h) = (size[0] as f32, size[1] as f32);
    if w < 64.0 || h < 32.0 {
        return;
    }
    let legacy = h < 64.0;
    // Model units: the player is 32 tall, and the figure's height on screen decides
    // how many pixels one unit is.
    let u = height / 34.0;
    let _ = ui;
    // A slight downward look, which is how a skin viewer shows a standing player.
    let pitch = 0.16f32;
    let (sy, cy_) = (yaw.sin(), yaw.cos());
    let (sp, cp) = (pitch.sin(), pitch.cos());
    // Model space: x right, y up, z towards the viewer. The model's origin is between
    // the feet; `top` is where the top of the head should land on screen.
    let project = |p: [f32; 3]| -> [f32; 2] {
        // Rotate about Y (the yaw), then about X (the fixed pitch).
        let x = p[0] * cy_ + p[2] * sy;
        let z = -p[0] * sy + p[2] * cy_;
        let y = p[1] * cp - z * sp;
        let depth = p[1] * sp + z * cp;
        // A weak perspective: enough to read as a solid, not enough to distort.
        let scale = 1.0 / (1.0 + depth * 0.006);
        [cx + x * u * scale, top + (32.0 - y) * u * scale]
    };
    // The parts, in Minecraft's own proportions: head 8x8x8, body 8x12x4, arms and
    // legs 4x12x4 and 4x12x4, and the limbs sit at the body's sides and below it.
    let parts: [([f32; 3], [f32; 3], [[f32; 4]; 6]); 6] = [
        // Head: front, right, back, left, top, bottom.
        (
            [0.0, 24.0, 0.0],
            [8.0, 8.0, 8.0],
            [
                [8.0, 8.0, 16.0, 16.0],
                [0.0, 8.0, 8.0, 16.0],
                [24.0, 8.0, 32.0, 16.0],
                [16.0, 8.0, 24.0, 16.0],
                [8.0, 0.0, 16.0, 8.0],
                [16.0, 0.0, 24.0, 8.0],
            ],
        ),
        // Body: front, right, back, left, top, bottom.
        (
            [0.0, 12.0, 0.0],
            [8.0, 12.0, 4.0],
            [
                [20.0, 20.0, 28.0, 32.0],
                [16.0, 20.0, 20.0, 32.0],
                [32.0, 20.0, 40.0, 32.0],
                [28.0, 20.0, 32.0, 32.0],
                [20.0, 16.0, 28.0, 20.0],
                [28.0, 16.0, 36.0, 20.0],
            ],
        ),
        // Right arm (the model's left as it faces us), 4 wide: it hangs at x -6.
        (
            [-6.0, 12.0, 0.0],
            [4.0, 12.0, 4.0],
            [
                [44.0, 20.0, 48.0, 32.0],
                [40.0, 20.0, 44.0, 32.0],
                [36.0, 20.0, 40.0, 32.0],
                [32.0, 20.0, 36.0, 32.0],
                [44.0, 16.0, 48.0, 20.0],
                [48.0, 16.0, 52.0, 20.0],
            ],
        ),
        // Left arm.
        (
            [6.0, 12.0, 0.0],
            [4.0, 12.0, 4.0],
            if legacy {
                [
                    [44.0, 20.0, 48.0, 32.0],
                    [40.0, 20.0, 44.0, 32.0],
                    [36.0, 20.0, 40.0, 32.0],
                    [32.0, 20.0, 36.0, 32.0],
                    [44.0, 16.0, 48.0, 20.0],
                    [48.0, 16.0, 52.0, 20.0],
                ]
            } else {
                [
                    [36.0, 52.0, 40.0, 64.0],
                    [32.0, 52.0, 36.0, 64.0],
                    [44.0, 52.0, 48.0, 64.0],
                    [40.0, 52.0, 44.0, 64.0],
                    [36.0, 48.0, 40.0, 52.0],
                    [40.0, 48.0, 44.0, 52.0],
                ]
            },
        ),
        // Right leg.
        (
            [-2.0, 0.0, 0.0],
            [4.0, 12.0, 4.0],
            [
                [4.0, 20.0, 8.0, 32.0],
                [0.0, 20.0, 4.0, 32.0],
                [12.0, 20.0, 16.0, 32.0],
                [8.0, 20.0, 12.0, 32.0],
                [4.0, 16.0, 8.0, 20.0],
                [8.0, 16.0, 12.0, 20.0],
            ],
        ),
        // Left leg.
        (
            [2.0, 0.0, 0.0],
            [4.0, 12.0, 4.0],
            if legacy {
                [
                    [4.0, 20.0, 8.0, 32.0],
                    [0.0, 20.0, 4.0, 32.0],
                    [12.0, 20.0, 16.0, 32.0],
                    [8.0, 20.0, 12.0, 32.0],
                    [4.0, 16.0, 8.0, 20.0],
                    [8.0, 16.0, 12.0, 20.0],
                ]
            } else {
                [
                    [20.0, 52.0, 24.0, 64.0],
                    [16.0, 52.0, 20.0, 64.0],
                    [28.0, 52.0, 32.0, 64.0],
                    [24.0, 52.0, 28.0, 64.0],
                    [20.0, 48.0, 24.0, 52.0],
                    [24.0, 48.0, 28.0, 52.0],
                ]
            },
        ),
    ];
    // Every quad, with the depth it should be painted at, so the near ones go last.
    let mut quads: Vec<(f32, [[f32; 2]; 4], [f32; 4])> = Vec::new();
    for (origin, size, faces) in parts {
        let [ox, oy, oz] = origin;
        let [sx, sy, sz] = size;
        let (x0, x1) = (ox - sx / 2.0, ox + sx / 2.0);
        let (y0, y1) = (oy, oy + sy);
        let (z0, z1) = (oz - sz / 2.0, oz + sz / 2.0);
        // The eight corners, in the order the faces below index them.
        let corners = [
            [x0, y1, z1],
            [x1, y1, z1],
            [x1, y0, z1],
            [x0, y0, z1],
            [x0, y1, z0],
            [x1, y1, z0],
            [x1, y0, z0],
            [x0, y0, z0],
        ];
        // The six faces, each as four corner indices going round it.
        let faces_at: [[usize; 4]; 6] = [
            [0, 1, 2, 3],
            [1, 5, 6, 2],
            [5, 4, 7, 6],
            [4, 0, 3, 7],
            [4, 5, 1, 0],
            [3, 2, 6, 7],
        ];
        for (face, uv) in faces_at.iter().zip(faces.iter()) {
            let points = [
                project(corners[face[0]]),
                project(corners[face[1]]),
                project(corners[face[2]]),
                project(corners[face[3]]),
            ];
            // The depth is the average of the four corners after the same rotation the
            // projection uses: a bigger value is nearer.
            let depth: f32 = face
                .iter()
                .map(|&i| {
                    let p = corners[i];
                    let x = p[0] * cy_ + p[2] * sy;
                    let z = -p[0] * sy + p[2] * cy_;
                    p[1] * sp + z * cp
                })
                .sum::<f32>()
                / 4.0;
            let uv = [uv[0] / w, uv[1] / h, uv[2] / w, uv[3] / h];
            quads.push((depth, points, uv));
        }
    }
    // Far first: the projection's depth is towards the viewer, so paint ascending.
    quads.sort_by(|a, b| a.0.total_cmp(&b.0));
    for (_, p, uv) in quads {
        draw.add_image_quad(texture, p[0], p[1], p[2], p[3])
            .uv([uv[0], uv[1]], [uv[2], uv[1]], [uv[2], uv[3]], [uv[0], uv[3]])
            .build();
    }
}

/// A player as blocks: head, body, arms, legs. The fallback when no skin is
/// decoded, so the card is never empty.
fn player_figure(draw: &DrawListMut<'_>, c: &Css, dpi: f32, cx: f32, cy: f32, s: f32) {
    let u = s / 8.0;
    let head = [cx - u * 1.4, cy - u * 4.0];
    fill(draw, head, [head[0] + u * 2.8, head[1] + u * 2.8], 0xc79a72, 1.0, u * 0.2);
    fill(draw, head, [head[0] + u * 2.8, head[1] + u * 0.8], 0x4a3423, 1.0, u * 0.2);
    fill(draw, [head[0] + u * 0.6, head[1] + u * 1.4], [head[0] + u * 1.0, head[1] + u * 1.8], 0x2a2a2e, 1.0, 0.0);
    fill(draw, [head[0] + u * 1.8, head[1] + u * 1.4], [head[0] + u * 2.2, head[1] + u * 1.8], 0x2a2a2e, 1.0, 0.0);
    let body_y = head[1] + u * 2.8;
    fill(draw, [cx - u * 1.4, body_y], [cx + u * 1.4, body_y + u * 3.0], 0x2e9c9c, 1.0, u * 0.15);
    fill(draw, [cx - u * 2.6, body_y], [cx - u * 1.4, body_y + u * 2.6], 0xc79a72, 1.0, u * 0.15);
    fill(draw, [cx + u * 1.4, body_y], [cx + u * 2.6, body_y + u * 2.6], 0xc79a72, 1.0, u * 0.15);
    let leg_y = body_y + u * 3.0;
    fill(draw, [cx - u * 1.4, leg_y], [cx - u * 0.1, leg_y + u * 2.6], 0x3b3b8f, 1.0, u * 0.15);
    fill(draw, [cx + u * 0.1, leg_y], [cx + u * 1.4, leg_y + u * 2.6], 0x3b3b8f, 1.0, u * 0.15);
    let _ = (c, dpi);
}

/// News and Patch Notes are the same page with different words: a heading, a line
/// saying what the page is, and a column of cards.
#[allow(clippy::too_many_arguments)]
fn stories_page(
    ui: &Ui,
    stories: &[LauncherStory],
    title: &str,
    blurb: &str,
    fonts: &Fonts,
    c: &Css,
    dpi: f32,
    g: &Grid,
    height: f32,
    rise: f32,
) {
    let mut y = heading(ui, fonts, c, dpi, g, title, rise);
    {
        let draw = ui.get_window_draw_list();
        draw.add_text([g.x + g.pad, y - 18.0 * dpi], rgba(c.muted, 1.0), blurb);
    }
    if stories.is_empty() {
        let draw = ui.get_window_draw_list();
        draw.add_text(
            [g.x + g.pad, y + 8.0 * dpi],
            rgba(c.muted, 1.0),
            "Nothing here yet.",
        );
        return;
    }
    let card_w = if g.w > 900.0 * dpi {
        (g.w - 2.0 * g.pad - g.pad) / 2.0
    } else {
        g.w - 2.0 * g.pad
    };
    let mut x = g.x + g.pad;
    for story in stories {
        let lines = wrap(ui, &story.body, card_w - 2.0 * 18.0 * dpi);
        let h = 26.0 * dpi + 22.0 * dpi + lines.len() as f32 * 19.0 * dpi + 18.0 * dpi;
        if y + h > height + rise {
            if x > g.x + g.pad {
                x = g.x + g.pad;
                y += 150.0 * dpi;
            }
            if y + h > height + rise {
                break;
            }
        }
        let draw = ui.get_window_draw_list();
        panel(&draw, c, [x, y], [x + card_w, y + h], c.panel, 4.0 * dpi);
        {
            let _f = ui.push_font(fonts.pixel);
            draw.add_text(
                [x + 18.0 * dpi, y + 16.0 * dpi],
                rgba(c.ink, 1.0),
                fit(ui, &story.title, card_w - 36.0 * dpi),
            );
        }
        if !story.tag.is_empty() {
            let _f = ui.push_font(fonts.small);
            draw.add_text(
                [x + 18.0 * dpi, y + 42.0 * dpi],
                rgba(c.accent, 1.0),
                fit(ui, &story.tag, card_w - 36.0 * dpi),
            );
        }
        for (i, line) in lines.iter().enumerate() {
            draw.add_text(
                [x + 18.0 * dpi, y + 66.0 * dpi + i as f32 * 19.0 * dpi],
                rgba(c.ink_soft, 1.0),
                line.as_str(),
            );
        }
        if x + card_w * 2.0 + g.pad <= g.x + g.w - g.pad {
            x += card_w + g.pad;
        } else {
            x = g.x + g.pad;
            y += h + g.pad;
        }
    }
}

fn mods_page(
    ui: &Ui,
    model: &LauncherModel,
    fonts: &Fonts,
    c: &Css,
    dpi: f32,
    g: &Grid,
    height: f32,
    rise: f32,
    sink: &mut Sink<'_>,
) {
    let mut y = heading(ui, fonts, c, dpi, g, "MODS", rise);
    // The registry's own site, and a way to fetch it again: the list is a snapshot of
    // what the registry said when the page opened, and a mod published since then is
    // only visible after a refresh. A failed fetch keeps the same button, which is the
    // retry.
    let bw = 150.0 * dpi;
    let bh = 36.0 * dpi;
    let mut bx = g.x + g.pad;
    // Refresh is also the retry: it drops the registry and the last notice, so a fetch
    // that failed is asked again and a list that went stale is rebuilt. It is not the
    // retry for an install — that is the row's own Install button, which is still there
    // because the file never landed.
    if raised_button(ui, fonts, c, dpi, bx, y - 10.0 * dpi, bw, bh, "Refresh", c.button, c.bevel, c.ink) {
        sink.push(LauncherEvent::RefreshMods);
    }
    bx += bw + 8.0 * dpi;
    if raised_button(ui, fonts, c, dpi, bx, y - 10.0 * dpi, bw + 40.0 * dpi, bh, "Browse the site", c.button, c.bevel, c.ink) {
        sink.push(LauncherEvent::OpenModsSite);
    }
    if !model.status.is_empty() {
        let draw = ui.get_window_draw_list();
        let _f = ui.push_font(fonts.small);
        draw.add_text(
            [bx + bw + 56.0 * dpi, y - 2.0 * dpi],
            // An install that failed is drawn in the same red as a fetch that did: both
            // are "this did not work", and only the wording differs.
            rgba(if model.mods_failed { c.danger } else { c.muted }, 1.0),
            fit(ui, &model.status, g.x + g.w - g.pad - bx - bw - 64.0 * dpi),
        );
    }
    y += 40.0 * dpi;

    // INSTALLED, above the registry: what is on the phone is the first thing a player
    // wants to see on a page called Mods, and the only rows that can be removed. The
    // registry below is what can be added.
    if !model.installed_mods.is_empty() {
        {
            let draw = ui.get_window_draw_list();
            let _f = ui.push_font(fonts.small);
            draw.add_text([g.x + g.pad, y], rgba(c.muted, 1.0), "INSTALLED");
        }
        y += 22.0 * dpi;
        let row_h = 46.0 * dpi;
        for name in &model.installed_mods {
            if y + row_h > height + rise {
                break;
            }
            let x = g.x + g.pad;
            let w = g.w - 2.0 * g.pad;
            // Scoped: `raised_button` asks for its own draw list at the end of the row.
            {
                let draw = ui.get_window_draw_list();
                panel(&draw, c, [x, y], [x + w, y + row_h], c.panel, 3.0 * dpi);
                fill(
                    &draw,
                    [x + 14.0 * dpi, y + 14.0 * dpi],
                    [x + 34.0 * dpi, y + row_h - 14.0 * dpi],
                    c.well,
                    1.0,
                    2.0 * dpi,
                );
                fill(
                    &draw,
                    [x + 19.0 * dpi, y + 20.0 * dpi],
                    [x + 29.0 * dpi, y + 26.0 * dpi],
                    c.accent,
                    1.0,
                    1.0,
                );
            }
            let btn_w = 104.0 * dpi;
            let text_x = x + 46.0 * dpi;
            let text_w = (x + w - 16.0 * dpi - btn_w - 12.0 * dpi - text_x).max(0.0);
            {
                let draw = ui.get_window_draw_list();
                let _f = ui.push_font(fonts.small);
                draw.add_text(
                    [text_x, y + (row_h - 16.0 * dpi) / 2.0],
                    rgba(c.ink, 1.0),
                    fit(ui, name, text_w),
                );
            }
            // Uninstall stays live during a download: it deletes a different file from
            // the one the worker is writing (the download goes to a `.part` first), so
            // there is no state to protect here.
            if raised_button(
                ui,
                fonts,
                c,
                dpi,
                x + w - 16.0 * dpi - btn_w,
                y + (row_h - 32.0 * dpi) / 2.0,
                btn_w,
                32.0 * dpi,
                "Uninstall",
                c.danger,
                c.bevel,
                c.on_accent,
            ) {
                sink.push(LauncherEvent::UninstallMod(name.clone()));
            }
            y += row_h + 6.0 * dpi;
        }
        y += 12.0 * dpi;
    }

    {
        let draw = ui.get_window_draw_list();
        let _f = ui.push_font(fonts.small);
        draw.add_text([g.x + g.pad, y], rgba(c.muted, 1.0), "REGISTRY");
    }
    y += 22.0 * dpi;

    if model.mods_loading {
        // The fetch runs on a worker; this frame has nothing to list yet. Saying so is
        // the whole difference between a page that is loading and one that froze.
        let draw = ui.get_window_draw_list();
        let _f = ui.push_font(fonts.small);
        draw.add_text(
            [g.x + g.pad, y],
            rgba(c.muted, 1.0),
            "Asking the registry...",
        );
        return;
    }
    if model.mods.is_empty() {
        // The reason, not a shrug: an empty list is either "the registry has nothing
        // for this build" or "the fetch failed", and only the status line can say
        // which.
        let draw = ui.get_window_draw_list();
        let line = if model.status.is_empty() {
            "No mods to show. Refresh to ask the registry again."
        } else {
            model.status.as_str()
        };
        let _f = ui.push_font(fonts.small);
        for (i, part) in wrap(ui, line, g.w - 2.0 * g.pad).iter().enumerate() {
            draw.add_text(
                [g.x + g.pad, y + i as f32 * 18.0 * dpi],
                rgba(if model.mods_failed { c.danger } else { c.muted }, 1.0),
                part.as_str(),
            );
        }
        return;
    }
    let row_h = 64.0 * dpi;
    for m in &model.mods {
        if y + row_h > height + rise {
            break;
        }
        let x = g.x + g.pad;
        let w = g.w - 2.0 * g.pad;
        let installed = model.installed_mods.iter().any(|name| name == &m.file);
        let installing = model.installing.as_deref() == Some(m.id.as_str());
        // Scoped: `raised_button` asks for its own draw list at the end of the row.
        {
            let draw = ui.get_window_draw_list();
            panel(&draw, c, [x, y], [x + w, y + row_h], c.panel, 3.0 * dpi);
            // A block, not a downloaded icon: it cannot be a broken image, and it reads
            // as the same family as the navigation's marks.
            fill(&draw, [x + 14.0 * dpi, y + 14.0 * dpi], [x + 50.0 * dpi, y + row_h - 14.0 * dpi], c.well, 1.0, 2.0 * dpi);
            fill(&draw, [x + 20.0 * dpi, y + 22.0 * dpi], [x + 44.0 * dpi, y + 30.0 * dpi], c.accent, 1.0, 1.0);
            fill(&draw, [x + 20.0 * dpi, y + 34.0 * dpi], [x + 38.0 * dpi, y + 40.0 * dpi], c.bevel, 1.0, 1.0);
        }
        let text_x = x + 62.0 * dpi;
        let btn_w = 104.0 * dpi;
        let text_w = (x + w - 16.0 * dpi - btn_w - 12.0 * dpi - text_x).max(0.0);
        {
            let draw = ui.get_window_draw_list();
            let _f = ui.push_font(fonts.pixel);
            draw.add_text([text_x, y + 12.0 * dpi], rgba(c.ink, 1.0), fit(ui, &m.name, text_w));
        }
        {
            let draw = ui.get_window_draw_list();
            let _f = ui.push_font(fonts.small);
            draw.add_text(
                [text_x, y + 34.0 * dpi],
                rgba(c.muted, 1.0),
                fit(ui, &format!("{} · {}", m.version, m.summary), text_w),
            );
        }
        // A download in flight says so on its own row, and the others stay pressable:
        // one at a time is enforced where the event is handled, and the label is what
        // tells the player why a second tap did nothing.
        let label = if installed {
            "Installed"
        } else if installing {
            "Installing..."
        } else {
            "Install"
        };
        let face = if installed || installing { c.button } else { c.accent };
        let ink = if installed || installing { c.muted } else { c.on_accent };
        if raised_button(
            ui,
            fonts,
            c,
            dpi,
            x + w - 16.0 * dpi - btn_w,
            y + (row_h - 34.0 * dpi) / 2.0,
            btn_w,
            34.0 * dpi,
            label,
            face,
            c.bevel,
            ink,
        ) && !installed
            && !installing
        {
            sink.push(LauncherEvent::InstallMod(m.id.clone()));
        }
        y += row_h + 8.0 * dpi;
    }
}

fn profile_page(
    ui: &Ui,
    model: &LauncherModel,
    fonts: &Fonts,
    c: &Css,
    dpi: f32,
    g: &Grid,
    height: f32,
    rise: f32,
    sink: &mut Sink<'_>,
) {
    let mut y = heading(ui, fonts, c, dpi, g, "PROFILE", rise);
    let face = 64.0 * dpi;
    {
        let draw = ui.get_window_draw_list();
        panel(&draw, c, [g.x + g.pad, y], [g.x + g.w - g.pad, y + face + 28.0 * dpi], c.panel, 4.0 * dpi);
        fill(&draw, [g.x + g.pad + 16.0 * dpi, y + 14.0 * dpi], [g.x + g.pad + 16.0 * dpi + face, y + 14.0 * dpi + face], c.accent, 1.0, 4.0 * dpi);
        {
            let _f = ui.push_font(fonts.title);
            let initial: String = model.account.chars().take(1).collect::<String>().to_uppercase();
            let initial = if initial.is_empty() { "P".to_string() } else { initial };
            let [iw, ih] = ui.calc_text_size(initial.as_str());
            draw.add_text(
                [g.x + g.pad + 16.0 * dpi + (face - iw) / 2.0, y + 14.0 * dpi + (face - ih) / 2.0],
                rgba(c.on_accent, 1.0),
                initial.as_str(),
            );
        }
        {
            let _f = ui.push_font(fonts.pixel);
            draw.add_text(
                [g.x + g.pad + face + 32.0 * dpi, y + 22.0 * dpi],
                rgba(c.ink, 1.0),
                fit(ui, &model.account, g.w - 2.0 * g.pad - face - 48.0 * dpi),
            );
        }
        {
            let _f = ui.push_font(fonts.small);
            draw.add_text(
                [g.x + g.pad + face + 32.0 * dpi, y + 46.0 * dpi],
                rgba(c.muted, 1.0),
                "Tap to rename",
            );
        }
    }
    ui.set_cursor_screen_pos([g.x + g.pad, y]);
    if ui.invisible_button("##rename", [g.w - 2.0 * g.pad, face + 28.0 * dpi]) {
        sink.push(LauncherEvent::RenameAccount);
    }
    y += face + 44.0 * dpi;

    // The line the platform has for this page: a sign-in waiting on the browser, a
    // failure, or the last thing that changed.
    if !model.status.is_empty() && y + 20.0 * dpi <= height + rise {
        let draw = ui.get_window_draw_list();
        let _f = ui.push_font(fonts.small);
        for (i, line) in wrap(ui, &model.status, g.w - 2.0 * g.pad).iter().enumerate() {
            draw.add_text(
                [g.x + g.pad, y + i as f32 * 17.0 * dpi],
                rgba(c.ink_soft, 1.0),
                line.as_str(),
            );
        }
        y += wrap(ui, &model.status, g.w - 2.0 * g.pad).len() as f32 * 17.0 * dpi + 14.0 * dpi;
    }

    // Accounts: the list this device knows, one of them selected, and the two ways
    // to add one. A Microsoft account is the real thing — the device-code flow in
    // `auth` — and an offline name is the other, which is what a phone could do
    // before. Both are rows, because they are the same decision.
    if y + 22.0 * dpi <= height + rise {
        let draw = ui.get_window_draw_list();
        let _f = ui.push_font(fonts.small);
        draw.add_text([g.x + g.pad, y], rgba(c.muted, 1.0), "ACCOUNTS");
        y += 24.0 * dpi;
    }
    let row_h = 52.0 * dpi;
    for (i, account) in model.accounts.iter().enumerate() {
        if y + row_h > height + rise {
            break;
        }
        let x = g.x + g.pad;
        let w = g.w - 2.0 * g.pad;
        {
            let draw = ui.get_window_draw_list();
            panel(
                &draw,
                c,
                [x, y],
                [x + w, y + row_h],
                if account.selected { c.raised } else { c.panel },
                3.0 * dpi,
            );
            if account.selected {
                stroke(&draw, [x, y], [x + w, y + row_h], c.accent, 1.0, 3.0 * dpi, 2.0);
            }
            {
                let _f = ui.push_font(fonts.pixel);
                draw.add_text(
                    [x + 16.0 * dpi, y + 8.0 * dpi],
                    rgba(c.ink, 1.0),
                    fit(ui, &account.name, w - 220.0 * dpi),
                );
            }
            {
                let _f = ui.push_font(fonts.small);
                draw.add_text(
                    [x + 16.0 * dpi, y + 30.0 * dpi],
                    rgba(if account.selected { c.accent } else { c.muted }, 1.0),
                    &account.kind,
                );
            }
        }
        let bw = 100.0 * dpi;
        if !account.selected
            && raised_button(
                ui,
                fonts,
                c,
                dpi,
                x + w - 16.0 * dpi - bw * 2.0 - 8.0 * dpi,
                y + (row_h - 32.0 * dpi) / 2.0,
                bw,
                32.0 * dpi,
                "Play as",
                c.accent,
                c.accent_lip,
                c.on_accent,
            )
        {
            sink.push(LauncherEvent::SwitchAccount(i as u32));
        }
        if model.accounts.len() > 1
            && raised_button(
                ui,
                fonts,
                c,
                dpi,
                x + w - 16.0 * dpi - bw,
                y + (row_h - 32.0 * dpi) / 2.0,
                bw,
                32.0 * dpi,
                "Remove",
                c.button,
                c.bevel,
                c.muted,
            )
        {
            sink.push(LauncherEvent::RemoveAccount(i as u32));
        }
        y += row_h + 8.0 * dpi;
    }

    let btn_h = 38.0 * dpi;
    if y + btn_h <= height + rise {
        let bw = 190.0 * dpi;
        if model.sign_in_waiting {
            // A sign-in is running: the useful control is the one that stops it, not a
            // second attempt at starting one.
            if raised_button(ui, fonts, c, dpi, g.x + g.pad, y, bw, btn_h, "CANCEL SIGN-IN", c.button, c.bevel, c.ink) {
                sink.push(LauncherEvent::CancelSignIn);
            }
        } else if raised_button(ui, fonts, c, dpi, g.x + g.pad, y, bw, btn_h, "+ MICROSOFT", c.accent, c.accent_lip, c.on_accent) {
            sink.push(LauncherEvent::SignInMicrosoft);
        }
        if raised_button(ui, fonts, c, dpi, g.x + g.pad + bw + 10.0 * dpi, y, bw, btn_h, "+ OFFLINE", c.button, c.bevel, c.ink) {
            sink.push(LauncherEvent::AddAccount);
        }
        y += btn_h + 22.0 * dpi;
    }

    // Touch controls: the two layouts a phone player already knows. This is a
    // launcher setting, so it stays here and never sends the player into the game's
    // own options to find it.
    if y + 22.0 * dpi <= height + rise {
        let draw = ui.get_window_draw_list();
        let _f = ui.push_font(fonts.small);
        draw.add_text([g.x + g.pad, y], rgba(c.muted, 1.0), "TOUCH CONTROLS");
        y += 24.0 * dpi;
    }
    if y + btn_h <= height + rise {
        let bw = 190.0 * dpi;
        let java = model.controls_preset == "java";
        let face = if java { c.accent } else { c.button };
        let ink = if java { c.on_accent } else { c.ink };
        if raised_button(ui, fonts, c, dpi, g.x + g.pad, y, bw, btn_h, "JAVA CLASSIC", face, c.bevel, ink) {
            sink.push(LauncherEvent::SetControls("java".into()));
        }
        let bedrock = model.controls_preset == "bedrock";
        let face = if bedrock { c.accent } else { c.button };
        let ink = if bedrock { c.on_accent } else { c.ink };
        if raised_button(ui, fonts, c, dpi, g.x + g.pad + bw + 10.0 * dpi, y, bw, btn_h, "BEDROCK PE", face, c.bevel, ink) {
            sink.push(LauncherEvent::SetControls("bedrock".into()));
        }
        y += btn_h + 22.0 * dpi;
    }

    // The facts, each a row, and the three actions under them.
    for (key, value) in &model.facts {
        if y + 40.0 * dpi > height + rise {
            break;
        }
        let draw = ui.get_window_draw_list();
        panel(&draw, c, [g.x + g.pad, y], [g.x + g.w - g.pad, y + 40.0 * dpi], c.panel, 3.0 * dpi);
        {
            let _f = ui.push_font(fonts.small);
            draw.add_text([g.x + g.pad + 16.0 * dpi, y + 12.0 * dpi], rgba(c.muted, 1.0), key.as_str());
        }
        let [vw, _] = ui.calc_text_size(value.as_str());
        draw.add_text(
            [g.x + g.w - g.pad - 16.0 * dpi - vw, y + 12.0 * dpi],
            rgba(c.ink, 1.0),
            value.as_str(),
        );
        y += 40.0 * dpi + 6.0 * dpi;
    }

    let btn_w = 170.0 * dpi;
    let btn_h = 42.0 * dpi;
    if y + btn_h <= height + rise {
        if raised_button(ui, fonts, c, dpi, g.x + g.pad, y, btn_w, btn_h, "SETTINGS", c.button, c.bevel, c.ink) {
            sink.push(LauncherEvent::Goto(LauncherPage::Settings));
        }
        if raised_button(ui, fonts, c, dpi, g.x + g.pad + btn_w + 10.0 * dpi, y, btn_w, btn_h, "EXPORT", c.button, c.bevel, c.ink) {
            // A folder the file manager can reach, not a path in a toast: the settings
            // archive goes to `export/settings` beside everything else.
            sink.push(LauncherEvent::ExportFiles("".into()));
        }
        if raised_button(ui, fonts, c, dpi, g.x + g.pad + (btn_w + 10.0 * dpi) * 2.0, y, btn_w, btn_h, "IMPORT", c.button, c.bevel, c.ink) {
            // The system picker, so a settings archive from another device can be
            // chosen rather than guessed at.
            sink.push(LauncherEvent::ImportFiles("settings".into()));
        }
    }
}

/// The smallest change a slider makes. A setting that does not name one gets a
/// hundredth of its range, which is what the old page sent and is still a usable
/// step for anything without a natural grid.
fn setting_step(setting: &LauncherSetting) -> f32 {
    if setting.step > 0.0 {
        setting.step
    } else {
        ((setting.max - setting.min) / 100.0).max(f32::EPSILON)
    }
}

/// How many decimals the number needs, read off the step it moves in and the scale
/// it is shown at: a 1% step is a whole percent, a 0.25 step is two decimals.
fn setting_decimals(step: f32) -> usize {
    if step >= 1.0 {
        0
    } else if step >= 0.25 {
        2
    } else if step >= 0.1 {
        1
    } else {
        2
    }
}

/// The number and its unit, as the row shows it: "12 chunks", "70°", "120 fps",
/// "50%". `percent` sliders store 0..1 and are read as 0..100, which is the one
/// place the shown number and the stored one differ.
fn setting_text(setting: &LauncherSetting, value: f32) -> String {
    let scale = if setting.percent { 100.0 } else { 1.0 };
    let decimals = setting_decimals(setting_step(setting) * scale);
    let mut number = format!("{:.*}", decimals, value * scale);
    // A trailing zero says nothing a player needs: "1.25" keeps both decimals
    // because the value has them, "1.00" becomes "1".
    if number.contains('.') {
        while number.ends_with('0') {
            number.pop();
        }
        if number.ends_with('.') {
            number.pop();
        }
    }
    if setting.unit.is_empty() {
        number
    } else if setting.unit == "%" || setting.unit == "°" || setting.unit == "x" {
        // A unit that is a symbol is set against the digits; a word gets a space.
        format!("{number}{}", setting.unit)
    } else {
        format!("{number} {}", setting.unit)
    }
}

/// `raw`, snapped to the slider's grid and kept inside its range. Without this a
/// 0..1 slider sends `0.30000001` and a frame limit sends 137, which the file then
/// rounds to 130 — a value that differs from the one the finger left behind.
fn snap_setting(setting: &LauncherSetting, raw: f32) -> f32 {
    let step = setting_step(setting);
    let snapped = setting.min + ((raw - setting.min) / step).round() * step;
    (snapped.clamp(setting.min, setting.max) * 1_000_000.0).round() / 1_000_000.0
}

/// The launcher's own settings: sliders and switches that write `options.txt`, so a
/// player never has to enter the game to change how it looks or how the controls
/// feel. The game reads the same file, so this is the same setting either way.
///
/// The page is one scrollable column: the heading and Back stay put, and the rows
/// move under them. A phone held in landscape has room for about eight of them, and
/// the page has three times that, so a page that simply stopped drawing at the
/// bottom edge hid two thirds of its own controls.
fn settings_page(
    ui: &Ui,
    model: &LauncherModel,
    fonts: &Fonts,
    c: &Css,
    dpi: f32,
    g: &Grid,
    height: f32,
    rise: f32,
    sink: &mut Sink<'_>,
) {
    let mut y = heading(ui, fonts, c, dpi, g, "SETTINGS", rise);
    if raised_button(ui, fonts, c, dpi, g.x + g.pad, y - 8.0 * dpi, 120.0 * dpi, 34.0 * dpi, "Back", c.button, c.bevel, c.ink) {
        sink.push(LauncherEvent::Goto(LauncherPage::Profile));
    }
    y += 30.0 * dpi;
    let top = y;
    let bottom = (height + rise - 14.0 * dpi).max(top);
    let view = bottom - top;
    let row_h = 60.0 * dpi;
    let gap = 8.0 * dpi;
    let group_h = 30.0 * dpi;
    let left = g.x + g.pad;
    let right = g.x + g.w - g.pad;
    let row_w = right - left;

    // The column's height, so the offset can be clamped to what there is to see. The
    // headings count: a page scrolled to its end must end on its last row, not on
    // empty glass that a heading's height would leave behind.
    let mut content = 0.0;
    let mut last_group = "";
    for setting in &model.settings {
        if !setting.group.is_empty() && setting.group.as_str() != last_group {
            content += group_h;
            last_group = setting.group.as_str();
        }
        content += row_h + gap;
    }
    let max_scroll = (content - view).max(0.0);

    // Input first, so this frame draws where the finger already is.
    let mouse = ui.io().mouse_pos;
    let over_page = mouse[0] >= g.x
        && mouse[0] <= g.x + g.w
        && mouse[1] >= top
        && mouse[1] <= bottom;
    let wheel = ui.io().mouse_wheel;
    let mut scroll = SETTINGS_SCROLL.get();
    if over_page && wheel != 0.0 {
        // A wheel is a mouse; a phone never sends one, and the drag below is what a
        // finger uses.
        scroll -= wheel * 48.0 * dpi;
    }
    if !ui.io().mouse_down[0] {
        SETTINGS_PRESS.set(None);
        SETTINGS_DRAG.set(None);
    } else if let Some(origin) = SETTINGS_PRESS.get() {
        // A widget that took the press owns the finger: a switch being held must not
        // drag the page out from under itself.
        if ui.is_any_item_active() {
            SETTINGS_DRAG.set(None);
        } else if (mouse[1] - origin[1]).abs() > 12.0 * dpi
            || SETTINGS_DRAG.get().is_some()
        {
            // A press that no widget took is a press on the page, and dragging it
            // moves the column. The threshold is what tells a drag from a tap that
            // landed between two rows.
            let last = SETTINGS_DRAG.get().unwrap_or(origin[1]);
            scroll -= mouse[1] - last;
            SETTINGS_DRAG.set(Some(mouse[1]));
        }
    } else if over_page && !ui.is_any_item_active() {
        SETTINGS_PRESS.set(Some(mouse));
    }
    let scroll = scroll.clamp(0.0, max_scroll);
    SETTINGS_SCROLL.set(scroll);

    // Everything the rows draw is clipped to the column, or a row scrolled past the
    // top would paint over the heading and the Back button.
    {
        let draw = ui.get_window_draw_list();
        draw.with_clip_rect_intersect([g.x, top], [g.x + g.w, bottom], || {
            let mut y = top - scroll;
            let mut last_group = "";
            for setting in &model.settings {
                if !setting.group.is_empty() && setting.group.as_str() != last_group {
                    if y + group_h > top && y < bottom {
                        {
                            let _f = ui.push_font(fonts.small);
                            draw.add_text([left, y + 6.0 * dpi], rgba(c.muted, 1.0), setting.group.as_str());
                        }
                        // A hairline under the heading: `stroke` needs a box with a
                        // height, so the rule is a filled one-pixel band.
                        fill(
                            &draw,
                            [left, y + group_h - 8.0 * dpi],
                            [right, y + group_h - 7.0 * dpi],
                            c.rule,
                            1.0,
                            0.0,
                        );
                    }
                    y += group_h;
                    last_group = setting.group.as_str();
                }
                // A row entirely outside the column is neither drawn nor given a hit
                // target: a button scrolled off the top must not take a tap meant for
                // the heading.
                if y + row_h <= top || y >= bottom {
                    y += row_h + gap;
                    continue;
                }
                // A row that starts above the column is drawn for the sliver that
                // shows, but takes no press: its hit target would sit under the
                // heading, where a tap belongs to Back.
                let takes_input = y >= top;
                panel(&draw, c, [left, y], [right, y + row_h], c.panel, 3.0 * dpi);
                {
                    // A switch has one line and centres it; a slider has the value on
                    // the top line and the track below it, so its label shares that top
                    // line rather than floating in the middle.
                    let label_y = if setting.toggle {
                        y + row_h / 2.0 - 8.0 * dpi
                    } else {
                        y + 9.0 * dpi
                    };
                    let _f = ui.push_font(fonts.body);
                    draw.add_text(
                        [left + 16.0 * dpi, label_y],
                        rgba(c.ink_soft, 1.0),
                        fit(ui, &setting.label, row_w * 0.62),
                    );
                }
                if setting.toggle {
                    let tw = 56.0 * dpi;
                    let th = 28.0 * dpi;
                    let tx = right - 16.0 * dpi - tw;
                    let ty = y + (row_h - th) / 2.0;
                    fill(
                        &draw,
                        [tx, ty],
                        [tx + tw, ty + th],
                        if setting.on { c.accent } else { c.well },
                        1.0,
                        th / 2.0,
                    );
                    let knob = th - 8.0 * dpi;
                    let kx = if setting.on { tx + tw - knob - 4.0 * dpi } else { tx + 4.0 * dpi };
                    fill(
                        &draw,
                        [kx, ty + 4.0 * dpi],
                        [kx + knob, ty + 4.0 * dpi + knob],
                        c.ink,
                        1.0,
                        knob / 2.0,
                    );
                    // The whole row is the switch. A 56x28 pill is a target a thumb
                    // misses; the label beside it says the same thing as the pill, so
                    // tapping either is the same intent.
                    if takes_input {
                        ui.set_cursor_screen_pos([left, y]);
                        let id = format!("##set{}", setting.key);
                        let hit = ui.invisible_button(id.as_str(), [row_w, row_h]);
                        // The row is drawn before the button, so its hover and press
                        // ring goes on after it, in the same list: a second
                        // `DrawListMut` for the same window cannot exist at once.
                        if hit || ui.is_item_hovered() {
                            stroke(
                                &draw,
                                [left, y],
                                [right, y + row_h],
                                if hit { c.accent } else { c.bevel },
                                1.0,
                                3.0 * dpi,
                                1.0,
                            );
                        }
                        if hit {
                            sink.push(LauncherEvent::ToggleOption(
                                setting.key.clone(),
                                !setting.on,
                            ));
                        }
                    }
                } else {
                    // The two ends of the range, measured first: they sit beside the
                    // track, not under it, so a handle at either end never covers the
                    // number that says where the end is.
                    let low = setting_text(setting, setting.min);
                    let high = setting_text(setting, setting.max);
                    let (low_w, high_w) = {
                        let _f = ui.push_font(fonts.small);
                        (ui.calc_text_size(low.as_str())[0], ui.calc_text_size(high.as_str())[0])
                    };
                    let track_x = left + 16.0 * dpi + low_w + 10.0 * dpi;
                    let track_w = (right - 16.0 * dpi - high_w - 10.0 * dpi - track_x).max(60.0 * dpi);
                    // The track is the well and the fill is the value; the handle is a
                    // block, because everything else on this screen is a block.
                    let ty = y + 34.0 * dpi;
                    let span = (setting.max - setting.min).max(f32::EPSILON);
                    let t = ((setting.value - setting.min) / span).clamp(0.0, 1.0);
                    fill(&draw, [track_x, ty], [track_x + track_w, ty + 6.0 * dpi], c.well, 1.0, 3.0 * dpi);
                    if t > 0.0 {
                        fill(
                            &draw,
                            [track_x, ty],
                            [track_x + track_w * t, ty + 6.0 * dpi],
                            c.accent,
                            1.0,
                            3.0 * dpi,
                        );
                    }
                    fill(
                        &draw,
                        [track_x + track_w * t - 5.0 * dpi, ty - 7.0 * dpi],
                        [track_x + track_w * t + 5.0 * dpi, ty + 13.0 * dpi],
                        c.ink,
                        1.0,
                        2.0 * dpi,
                    );
                    // The value with its unit, and the two ends of the track, so the
                    // number means something without a guess at what it ranges over.
                    {
                        let _f = ui.push_font(fonts.small);
                        let text = setting_text(setting, setting.value);
                        let [tw, _] = ui.calc_text_size(text.as_str());
                        draw.add_text(
                            [right - 16.0 * dpi - tw, y + 8.0 * dpi],
                            rgba(c.ink, 1.0),
                            text.as_str(),
                        );
                        draw.add_text(
                            [left + 16.0 * dpi, ty - 4.0 * dpi],
                            rgba(c.muted, 1.0),
                            low.as_str(),
                        );
                        draw.add_text(
                            [right - 16.0 * dpi - high_w, ty - 4.0 * dpi],
                            rgba(c.muted, 1.0),
                            high.as_str(),
                        );
                    }
                    // The hit target is the whole track's band, and the value follows
                    // the finger: a press sets it, and holding keeps sending as the
                    // finger slides, so a thumb that lands on 12 and drags to 30 sees
                    // the number move with it. Only a change of a step or more is
                    // sent, or a drag would write `options.txt` sixty times a second.
                    if takes_input {
                        ui.set_cursor_screen_pos([track_x, y]);
                        let id = format!("##set{}", setting.key);
                        let pressed = ui.invisible_button(id.as_str(), [track_w, row_h]);
                        if pressed || ui.is_item_active() {
                            let at = ui.io().mouse_pos[0];
                            let frac = ((at - track_x) / track_w).clamp(0.0, 1.0);
                            let value = snap_setting(setting, setting.min + span * frac);
                            let step = setting_step(setting);
                            if (value - setting.value).abs() > step * 0.5 {
                                sink.push(LauncherEvent::SetOption(setting.key.clone(), value));
                            }
                        }
                    }
                }
                y += row_h + gap;
            }
            if model.settings.is_empty() {
                draw.add_text(
                    [left, top + 8.0 * dpi],
                    rgba(c.muted, 1.0),
                    "No settings are available until the pack is installed.",
                );
            }
        });
    }

    // A thin thumb on the page's edge, when there is more than fits: the page has to
    // say that it scrolls, or the rows below the fold may as well not exist.
    if max_scroll > 0.0 && view > 1.0 {
        let draw = ui.get_window_draw_list();
        let x = g.x + g.w - 7.0 * dpi;
        fill(&draw, [x, top], [x + 3.0 * dpi, bottom], c.well, 1.0, 2.0 * dpi);
        let thumb = (view / content * view).max(28.0 * dpi);
        let at = top + (scroll / max_scroll) * (view - thumb);
        fill(&draw, [x, at], [x + 3.0 * dpi, at + thumb], c.rule, 1.0, 2.0 * dpi);
    }
}

/// The name field, on a pass of its own over the whole screen. The phone's keyboard
/// covers the bottom half, so the card sits high and the launcher behind it waits; a
/// tap anywhere outside is a way back out.
#[allow(clippy::too_many_arguments)]
fn rename_dialog(
    ui: &Ui,
    model: &mut LauncherModel,
    fonts: &Fonts,
    c: &Css,
    dpi: f32,
    width: f32,
    height: f32,
    sink: &mut Sink<'_>,
) {
    let w = (460.0 * dpi).min(width - 32.0 * dpi).max(160.0 * dpi);
    let h = 200.0 * dpi;
    let x = (width - w) / 2.0;
    let y = 60.0 * dpi;
    let inner = 22.0 * dpi;
    let keep = (w - 2.0 * inner).max(0.0);
    let button = 128.0 * dpi;
    let _ = ui
        .window("##rename")
        .flags(
            WindowFlags::NO_TITLE_BAR
                | WindowFlags::NO_RESIZE
                | WindowFlags::NO_MOVE
                | WindowFlags::NO_COLLAPSE
                | WindowFlags::NO_BACKGROUND
                | WindowFlags::NO_SCROLLBAR
                | WindowFlags::NO_SCROLL_WITH_MOUSE
                | WindowFlags::NO_FOCUS_ON_APPEARING,
        )
        .position([0.0, 0.0], Condition::Always)
        .size([width, height], Condition::Always)
        .build(|| {
            {
                let draw = ui.get_window_draw_list();
                fill(&draw, [0.0, 0.0], [width, height], c.page, 0.78, 0.0);
                fill(&draw, [x, y], [x + w, y + h], c.panel, 1.0, 6.0 * dpi);
                stroke(&draw, [x, y], [x + w, y + h], c.bevel, 1.0, 6.0 * dpi, 1.0);
                let _f = ui.push_font(fonts.pixel);
                draw.add_text([x + inner, y + 20.0 * dpi], rgba(c.ink, 1.0), "Player name");
                draw.add_text(
                    [x + w - inner - 148.0 * dpi, y + 24.0 * dpi],
                    rgba(c.muted, 1.0),
                    "A-Z a-z 0-9 _",
                );
                let _g = ui.push_font(fonts.body);
                draw.add_text(
                    [x + inner, y + 48.0 * dpi],
                    rgba(c.muted, 1.0),
                    format!(
                        "Signed in as {}",
                        if model.account.is_empty() {
                            "Player"
                        } else {
                            model.account.as_str()
                        }
                    ),
                );
            }
            {
                let draw = ui.get_window_draw_list();
                field(&draw, ui, fonts, c, dpi, x + inner, y + 68.0 * dpi, keep, &model.draft);
            }

            ui.set_cursor_screen_pos([x + inner, y + h - 54.0 * dpi]);
            {
                let _b = ui.push_style_color(StyleColor::Button, rgba(c.accent, 1.0));
                let _hover = ui.push_style_color(StyleColor::ButtonHovered, rgba(ACCENT_HOVER, 1.0));
                let _a = ui.push_style_color(StyleColor::ButtonActive, rgba(ACCENT_HOVER, 1.0));
                let _t = ui.push_style_color(StyleColor::Text, rgba(c.on_accent, 1.0));
                let _f = ui.push_font(fonts.pixel);
                if ui.button_with_size("Save", [button, 38.0 * dpi]) {
                    sink.push(LauncherEvent::SetAccount(model.draft.clone()));
                    model.editing = false;
                }
            }
            ui.set_cursor_screen_pos([x + w - inner - button, y + h - 54.0 * dpi]);
            {
                let _b = ui.push_style_color(StyleColor::Button, rgba(c.button, 1.0));
                let _t = ui.push_style_color(StyleColor::Text, rgba(c.muted, 1.0));
                let _f = ui.push_font(fonts.pixel);
                if ui.button_with_size("Cancel", [button, 38.0 * dpi]) {
                    model.editing = false;
                }
            }
            // Away from the card in any direction is a way out. The side bands matter
            // most: the card is a third of the screen wide at the phone's aspect, and
            // the gutters beside it used to belong to the full-screen window with no
            // widget under them, so a tap there did nothing at all.
            let bands = [
                (0.0, y, 0.0, width),
                (y, y + h, 0.0, x),
                (y, y + h, x + w, width),
                (y + h, height, 0.0, width),
            ];
            for (i, (top, bottom, left, right)) in bands.into_iter().enumerate() {
                ui.set_cursor_screen_pos([left, top]);
                let id = format!("##away{i}");
                if ui.invisible_button(id.as_str(), [right - left, bottom - top]) {
                    model.editing = false;
                }
            }
        });
}

/// The field's own face: what the platform has gathered from the IME, and a caret at
/// the end of it. imgui's text widget wants a keyboard of its own, and the phone's
/// delivers words to the activity rather than keys to a window, so the name is edited
/// the way every other field on this build is and only drawn here.
fn field(
    draw: &DrawListMut<'_>,
    ui: &Ui,
    fonts: &Fonts,
    c: &Css,
    dpi: f32,
    x: f32,
    y: f32,
    width: f32,
    draft: &str,
) {
    let _f = ui.push_font(fonts.body);
    // Measured in the body face, which is the one the text is drawn in: the caret has
    // to land at the end of the glyphs that are actually there.
    let caret = x + 14.0 * dpi + ui.calc_text_size(draft)[0];
    fill(draw, [x, y], [x + width, y + 50.0 * dpi], c.well, 1.0, 3.0 * dpi);
    fill(draw, [x, y + 49.0 * dpi], [x + width, y + 51.0 * dpi], c.accent, 1.0, 0.0);
    if draft.is_empty() {
        draw.add_text([x + 14.0 * dpi, y + 15.0 * dpi], rgba(c.muted, 1.0), "New name");
    } else {
        draw.add_text([x + 14.0 * dpi, y + 15.0 * dpi], rgba(c.ink, 1.0), draft);
    }
    let cx = caret.min(x + width - 3.0 * dpi);
    fill(
        draw,
        [cx, y + 13.0 * dpi],
        [cx + 2.0 * dpi, y + 37.0 * dpi],
        c.accent,
        1.0,
        1.0,
    );
}

impl Css {
    /// The account face's colour: the accent, so the one saturated thing in the
    /// column is the person using it.
    fn face(&self) -> u32 {
        self.accent
    }
}
