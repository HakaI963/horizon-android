//! The launcher's front end, drawn in imgui over the game's own surface.
//!
//! The desktop launcher is a GPUI app, and GPUI has no Android backend: a phone
//! runs one activity with one winit loop, so the launcher cannot be a second
//! program the way it is on a desktop. It is drawn here instead, in the imgui
//! layer that already paints the touch controls, so it shares the loop, the
//! device and the font atlas with the game.
//!
//! This is the desktop launcher's own design, reproduced: the "Blocks" system from
//! `crates/horizon-theme` and `launcher/src/blocks.rs`. A top bar carrying the mark
//! and the destinations, a sidebar of instances, and a page beside them. Panels are
//! stone-dark with a two-pixel outline, a light bevel on top and a darker lip below;
//! headings, navigation and buttons are in Pixelify Sans; body text is Geist.
//!
//! Every measurement below is the desktop's, in logical pixels, so the two
//! launchers are the same interface at any screen size rather than two designs that
//! resemble each other.
//!
//! This module knows how a launcher looks and nothing about what is installed: the
//! platform feeds it a [`LauncherModel`] of text and numbers it has gathered from
//! the engine, and it hands back [`LauncherEvent`]s for the platform to act on.

use std::cell::Cell;

use imgui::{
    Condition, DrawListMut, FontConfig, FontId, FontSource, StyleColor, TextureId, Ui, WindowFlags,
};

thread_local! {
    /// The destination the pointer was on last frame, if any.
    static NAV_HOVER: Cell<Option<LauncherPage>> = const { Cell::new(None) };

    /// The eased hover layer of each destination, 0 to 1, one per [`PAGES`] row.
    static NAV_GLOW: Cell<[f32; NAV_ROWS]> = const { Cell::new([0.0; NAV_ROWS]) };

    /// Whether the platform wants animations.
    static MOTION: Cell<bool> = const { Cell::new(true) };

    /// How far a scrolling page is scrolled, in pixels.
    static SETTINGS_SCROLL: Cell<f32> = const { Cell::new(0.0) };

    /// Where the finger was on the last frame of a drag that scrolls a page.
    static SETTINGS_DRAG: Cell<Option<f32>> = const { Cell::new(None) };

    /// Whether a finger was down on the last frame.
    static SETTINGS_HELD: Cell<bool> = const { Cell::new(false) };

    /// Where the finger landed on a scrolling page.
    static SETTINGS_PRESS: Cell<Option<[f32; 2]>> = const { Cell::new(None) };

    /// Whether the gesture in progress became a scroll.
    static SETTINGS_SWALLOW: Cell<bool> = const { Cell::new(false) };

    /// Whether a slider was the widget holding the finger on the last frame.
    static SETTINGS_ON_TRACK: Cell<bool> = const { Cell::new(false) };

    /// Where the finger was on the last frame of a drag that turns the Skins model.
    static SKIN_DRAG: Cell<Option<[f32; 2]>> = const { Cell::new(None) };

    /// Whether a finger was already down when the Skins page opened, so a drag that
    /// began elsewhere does not grab the model.
    static SKIN_HELD: Cell<bool> = const { Cell::new(false) };

    /// The model's angle and tilt, kept between frames because the platform rebuilds
    /// the model on every one.
    static SKIN_TURN: Cell<(f32, f32)> = const { Cell::new((0.6, 0.10)) };
}

/// The destinations in the top bar, in the order they are drawn.
///
/// The desktop's own set: Play, Versions, Mods, Servers, Patch notes. Settings is
/// reached from the account chip and the sliders button, not from the bar.
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
    Settings,
    Files,
    Multiplayer,
}

impl LauncherPage {
    pub fn label(self) -> &'static str {
        match self {
            LauncherPage::Play => "Play",
            LauncherPage::Installations => "Versions",
            LauncherPage::Skins => "Skins",
            LauncherPage::PatchNotes => "Patch notes",
            LauncherPage::News => "What's next",
            LauncherPage::Mods => "Mods",
            LauncherPage::Profile => "Accounts",
            LauncherPage::Settings => "Settings",
            LauncherPage::Files => "Files",
            LauncherPage::Multiplayer => "Servers",
        }
    }

    /// The line under a page's title, as the desktop writes it.
    fn subtitle(self) -> &'static str {
        match self {
            LauncherPage::Play => "",
            LauncherPage::Installations => "Builds from GitHub. Each one installs into its own folder.",
            LauncherPage::Skins => "The skin the game draws you in.",
            LauncherPage::PatchNotes => "What changed in each build.",
            LauncherPage::News => "What is being worked on.",
            LauncherPage::Mods => "Mods, resource packs and modpacks.",
            LauncherPage::Profile => "Who is playing.",
            LauncherPage::Settings => "How the launcher and the game behave.",
            LauncherPage::Files => "Where the game keeps its files.",
            LauncherPage::Multiplayer => "Start the game, or run a server for others to join.",
        }
    }

    /// The icon this destination wears, by name in the theme's pixel set.
    fn icon(self) -> &'static str {
        match self {
            LauncherPage::Play => "home",
            LauncherPage::Installations => "layers",
            LauncherPage::Skins => "user",
            LauncherPage::PatchNotes => "book",
            LauncherPage::News => "sparkles",
            LauncherPage::Mods => "puzzle",
            LauncherPage::Profile => "user",
            LauncherPage::Settings => "sliders",
            LauncherPage::Files => "folder",
            LauncherPage::Multiplayer => "server",
        }
    }
}

/// The bar's order, as the desktop launcher has it.
pub const PAGES: [LauncherPage; 5] = [
    LauncherPage::Play,
    LauncherPage::Installations,
    LauncherPage::Mods,
    LauncherPage::Multiplayer,
    LauncherPage::PatchNotes,
];

/// How many destinations the bar has, and so how many hover layers it keeps.
const NAV_ROWS: usize = PAGES.len();

/// One row of a list page.
#[derive(Clone, Debug, Default)]
pub struct LauncherRow {
    pub id: u32,
    pub title: String,
    pub subtitle: String,
    /// A short right-aligned tag: "Installed", "26.3", "not supported".
    pub badge: String,
    pub selected: bool,
    pub enabled: bool,
}

/// One slider or switch on the Settings page.
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
    /// What the number is measured in: "chunks", "fps", "°", "%", "x".
    pub unit: String,
    /// The heading this row sits under.
    pub group: String,
    /// The smallest change the slider can make.
    pub step: f32,
    /// The stored value is 0..1 and the player reads it as a percentage.
    pub percent: bool,
    /// The key is the launcher's own rather than an `options.txt` name.
    pub launcher: bool,
}

/// A card on Patch notes or What's next: a heading, a tag, and a paragraph.
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
    /// What the front button says, when it is not Play or Install.
    pub front: Option<String>,
    pub account: String,
    /// Extra one-line facts for the Accounts page.
    pub facts: Vec<(String, String)>,
    /// The name field is open.
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
    /// The `.hmod` files already installed, by name.
    pub installed_mods: Vec<String>,
    /// The mod id being downloaded right now, if one is.
    pub installing: Option<String>,
    /// Where the mod registry's website is.
    pub mods_site: String,
    /// The last registry fetch failed.
    pub mods_failed: bool,
    /// The registry is being fetched on a worker right now.
    pub mods_loading: bool,
    /// The accounts this device knows, and which one Play uses.
    pub accounts: Vec<LauncherAccount>,
    /// The chosen skin's pixel size.
    pub skin_size: [u32; 2],
    /// Which built-in touch layout is in force: `"java"` or `"bedrock"`.
    /// While a world is hosted: the address friends join on.
    pub hosting: Option<String>,
    /// A Microsoft sign-in is waiting on the browser.
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
    /// The file name the registry will install as.
    pub file: String,
}

/// What the loading surface says while the game is starting or a world is opening.
#[derive(Clone, Debug, Default)]
pub struct LoadingScreen {
    /// The line under the title: "Starting the game", "Opening world".
    pub title: String,
    /// What is being worked on right now, if the platform can say.
    pub detail: String,
    /// 0..1 when the platform knows how far along it is.
    pub progress: Option<f32>,
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
    Restart,
    RenameAccount,
    SetAccount(String),
    OpenSettings,
    OpenSkins,
    PickSkin(String),
    ExportSettings,
    ImportSettings,
    /// Pick a file of this kind: "skin", "mod", "resourcepack", "settings".
    ImportFiles(String),
    /// Copy a folder of the game's own files out where a file manager can reach it.
    ExportFiles(String),
    Quit,
    /// A slider moved: `options.txt` key and its new value.
    SetOption(String, f32),
    /// A switch flipped.
    ToggleOption(String, bool),
    InstallMod(String),
    UninstallMod(String),
    RefreshMods,
    OpenModsSite,
    RevealFiles,
    HostWorld(u32),
    StopHosting,
    AddAccount,
    SwitchAccount(u32),
    RemoveAccount(u32),
    SignInMicrosoft,
    CancelSignIn,
    /// Open the pad's arrange mode: the buttons become draggable.
    OpenControls,
}

/// The "Blocks" palette, straight from `horizon_theme::BLOCKS_DARK`: the same
/// violet accent, the same stone plates, the same two-pixel outline.
pub struct Css {
    /// The window behind everything, under its stone texture.
    pub page: u32,
    /// The top bar and the sidebar.
    pub side: u32,
    /// Panels: rows, cards, dialogs.
    pub panel: u32,
    /// A selected or hovered panel or row, the open tab.
    pub raised: u32,
    /// Buttons and the chosen segment.
    pub button: u32,
    /// A button's top bevel.
    pub button_bevel: u32,
    /// Sunk wells: fields, tracks, switch boxes.
    pub well: u32,
    /// The light top edge of a panel.
    pub bevel: u32,
    /// The darker bottom edge of a panel or button.
    pub lip: u32,
    /// The 2 px outline around panels, buttons and wells.
    pub outline: u32,
    /// Rules between rows inside a panel.
    pub rule: u32,
    pub ink: u32,
    pub ink_soft: u32,
    pub muted: u32,
    /// Links, focus, progress, the selection marker.
    pub accent: u32,
    /// The main action's face (Play, Install), its top bevel and bottom lip.
    pub face: u32,
    pub face_bevel: u32,
    pub face_lip: u32,
    pub on_face: u32,
    pub danger: u32,
    pub success: u32,
    pub warn: u32,
}

pub const THEME: Css = Css {
    page: 0x0f0d17,
    side: 0x0b0a11,
    panel: 0x1a1727,
    raised: 0x221e34,
    button: 0x2b2641,
    button_bevel: 0x3d3560,
    well: 0x0c0b12,
    bevel: 0x342d52,
    lip: 0x07060b,
    outline: 0x05040a,
    rule: 0x120f1c,
    ink: 0xecedf3,
    ink_soft: 0xc5c7d1,
    muted: 0xa29cb8,
    accent: 0x9a86ff,
    face: 0x5b3fd6,
    face_bevel: 0x8a74ff,
    face_lip: 0x3a2694,
    on_face: 0xffffff,
    danger: 0xf07a68,
    success: 0x45c27a,
    warn: 0xf0b84f,
};

/// The desktop's measurements, in logical pixels.
pub const TOPBAR: f32 = 56.0;
pub const SIDEBAR: f32 = 248.0;
pub const PAGE_GUTTER: f32 = 32.0;
pub const CONTROL: f32 = 34.0;
pub const SEGMENT: f32 = 26.0;
/// The light top edge and the darker lip on a raised block.
pub const BEVEL: f32 = 2.0;
pub const LIP: f32 = 3.0;
/// The outline around a panel, a button or a well.
pub const OUTLINE: f32 = 2.0;

/// How far a page's content slides in when the page changes, and how heavy the veil
/// over it starts.
const PAGE_SLIDE: f32 = 8.0;
const PAGE_VEIL: f32 = 0.3;

/// How far a button's face drops under a finger.
const PRESS_DROP: f32 = 1.0;

/// The hover layer of a bar destination: its weight at full strength, and the time
/// constant of its approach.
const NAV_GLOW_ALPHA: f32 = 1.0;
const NAV_GLOW_TAU: f32 = 0.09;

/// The one geometry every element is placed from.
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
    /// The top bar's height.
    top: f32,
}

impl Grid {
    fn new(width: f32, dpi: f32) -> Self {
        // A phone in landscape has less width than a desktop window, so the sidebar
        // keeps its desktop width but gives way on a narrow screen rather than
        // leaving the page unusable.
        let side = (SIDEBAR * dpi).min(width * 0.34);
        Self {
            x: side,
            w: width - side,
            pad: PAGE_GUTTER * dpi,
            side,
            top: TOPBAR * dpi,
        }
    }

    /// Where a right-aligned control of width `w` starts.
    fn right(&self, w: f32) -> f32 {
        self.x + self.w - self.pad - w
    }
}

fn rgba(c: u32, a: f32) -> [f32; 4] {
    [
        ((c >> 16) & 0xff) as f32 / 255.0,
        ((c >> 8) & 0xff) as f32 / 255.0,
        (c & 0xff) as f32 / 255.0,
        a,
    ]
}

/// One frame of a hover layer: `current` moving towards `target` at a rate set by
/// the frame's own time, so the same fade takes the same time at any refresh rate.
fn ease_glow(current: f32, target: f32, dt: f32) -> f32 {
    if dt <= 0.0 {
        return current;
    }
    let k = 1.0 - (-dt / NAV_GLOW_TAU).exp();
    (current + (target - current) * k).clamp(0.0, 1.0)
}

/// The faces the launcher draws with, exactly the desktop's: Pixelify Sans for
/// headings, navigation, buttons and badges; Geist for body text; Geist Mono for
/// paths and versions.
pub struct Fonts {
    /// Body text.
    pub body: FontId,
    /// Headings, navigation, buttons.
    pub title: FontId,
    /// The page title's display size.
    pub display: FontId,
    /// Captions, units, dense rows.
    pub small: FontId,
    /// Buttons and badges, one step heavier than body.
    pub label: FontId,
    /// Paths and versions.
    pub mono: FontId,
}

/// Load the fonts and set the base style. Called once, where the fonts are added.
pub fn theme(imgui: &mut imgui::Context, dpi: f32) -> Fonts {
    let body = imgui.fonts().add_font(&[FontSource::TtfData {
        data: horizon_theme::fonts::GEIST_REGULAR,
        size_pixels: 14.0 * dpi,
        config: Some(FontConfig {
            oversample_h: 2,
            oversample_v: 1,
            ..Default::default()
        }),
    }]);
    let title = imgui.fonts().add_font(&[FontSource::TtfData {
        data: horizon_theme::fonts::PIXEL_SEMIBOLD,
        size_pixels: 16.0 * dpi,
        config: Some(FontConfig::default()),
    }]);
    let display = imgui.fonts().add_font(&[FontSource::TtfData {
        data: horizon_theme::fonts::PIXEL_BOLD,
        size_pixels: 38.0 * dpi,
        config: Some(FontConfig::default()),
    }]);
    let small = imgui.fonts().add_font(&[FontSource::TtfData {
        data: horizon_theme::fonts::GEIST_REGULAR,
        size_pixels: 12.0 * dpi,
        config: Some(FontConfig::default()),
    }]);
    let label = imgui.fonts().add_font(&[FontSource::TtfData {
        data: horizon_theme::fonts::PIXEL_MEDIUM,
        size_pixels: 15.0 * dpi,
        config: Some(FontConfig::default()),
    }]);
    let mono = imgui.fonts().add_font(&[FontSource::TtfData {
        data: horizon_theme::fonts::GEIST_MONO,
        size_pixels: 12.5 * dpi,
        config: Some(FontConfig::default()),
    }]);

    let c = &THEME;
    let style = imgui.style_mut();
    style.window_rounding = 0.0;
    style.frame_rounding = 0.0;
    style.scrollbar_rounding = 0.0;
    style.grab_rounding = 0.0;
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
    colors[StyleColor::ScrollbarGrab as usize] = rgba(c.outline, 1.0);
    colors[StyleColor::ScrollbarGrabHovered as usize] = rgba(c.muted, 1.0);
    colors[StyleColor::ScrollbarGrabActive as usize] = rgba(c.ink, 1.0);
    Fonts {
        body,
        title,
        display,
        small,
        label,
        mono,
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

/// The blocky look's raised edge: a light band along the top and a darker lip along
/// the bottom, inside the outline. This is the one thing that makes a panel or a
/// button read as a block rather than as a rectangle.
fn edges(draw: &DrawListMut<'_>, min: [f32; 2], max: [f32; 2], top: u32, bottom: u32, dpi: f32) {
    let inset = OUTLINE * dpi;
    fill(
        draw,
        [min[0] + inset, min[1] + inset],
        [max[0] - inset, min[1] + inset + BEVEL * dpi],
        top,
        1.0,
        0.0,
    );
    fill(
        draw,
        [min[0] + inset, max[1] - inset - LIP * dpi],
        [max[0] - inset, max[1] - inset],
        bottom,
        1.0,
        0.0,
    );
}

/// A panel: a face, a two-pixel outline, and the raised edges inside it.
fn panel(draw: &DrawListMut<'_>, c: &Css, min: [f32; 2], max: [f32; 2], face: u32, dpi: f32) {
    let o = OUTLINE * dpi;
    stroke(draw, min, max, c.outline, 1.0, 0.0, o);
    fill(
        draw,
        [min[0] + o, min[1] + o],
        [max[0] - o, max[1] - o],
        face,
        1.0,
        0.0,
    );
    edges(draw, min, max, c.bevel, c.lip, dpi);
}

/// A sunk well: a field, a track, the box a switch sits in. The inverse of a panel:
/// its outline is inside, so it reads as cut into the page.
fn well(draw: &DrawListMut<'_>, c: &Css, min: [f32; 2], max: [f32; 2], dpi: f32) {
    well_alpha(draw, c, min, max, dpi, 1.0);
}

/// The same, at `alpha`: the loading surface's bar fades with the surface.
fn well_alpha(
    draw: &DrawListMut<'_>,
    c: &Css,
    min: [f32; 2],
    max: [f32; 2],
    dpi: f32,
    alpha: f32,
) {
    let o = OUTLINE * dpi;
    fill(draw, min, max, c.well, alpha, 0.0);
    // A dark edge on top and a light one below is what a hole in the page looks like.
    fill(
        draw,
        [min[0], min[1]],
        [max[0], min[1] + o],
        c.outline,
        alpha,
        0.0,
    );
    fill(
        draw,
        [min[0], max[1] - o],
        [max[0], max[1]],
        c.bevel,
        0.35 * alpha,
        0.0,
    );
}

/// A colour at 88% of its own brightness: the face of a button under a finger.
fn darker(c: u32) -> u32 {
    let scale = |shift: u32| (((c >> shift) & 0xff) as f32 * 0.88) as u32;
    (scale(16) << 16) | (scale(8) << 8) | scale(0)
}

/// The width of `text` in a given face, without leaving that face pushed.
fn text_width(ui: &Ui, font: FontId, text: &str) -> f32 {
    let _f = ui.push_font(font);
    ui.calc_text_size(text)[0]
}

/// The text cut to what fits, with an ellipsis.
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
/// draw list has no wrapping of its own.
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

/// The events a page may raise this frame.
struct Sink<'a> {
    events: &'a mut Vec<LauncherEvent>,
}

impl<'a> Sink<'a> {
    fn push(&mut self, event: LauncherEvent) {
        self.events.push(event);
    }
}
/// The whole launcher, edge to edge.
#[allow(clippy::too_many_arguments)]
pub fn draw(
    ui: &Ui,
    model: &mut LauncherModel,
    fonts: &Fonts,
    dpi: f32,
    width: f32,
    height: f32,
    opening: Option<f32>,
    fade: f32,
    reduce_motion: bool,
    background: Option<(TextureId, [u32; 2])>,
    stone: Option<(TextureId, [u32; 2])>,
    skin: Option<TextureId>,
    out: &mut Vec<LauncherEvent>,
) {
    let c = &THEME;
    MOTION.with(|m| m.set(!reduce_motion));
    if reduce_motion {
        NAV_GLOW.with(|g| g.set([0.0; NAV_ROWS]));
        NAV_HOVER.with(|h| h.set(None));
    }
    // A tap is always a tap. The opening is a reveal, not a modal: the buttons are on
    // the glass from the first frame, and a player who taps one before the animation
    // has finished means it. Gating on the animation is what made the first launch
    // look broken — the Play button and every tab did nothing until a restart, because
    // the model recorded the tap and the event was thrown away.
    let mut sink = Sink { events: out };
    let g = Grid::new(width, dpi);
    let rise = match opening {
        Some(t) => horizon_theme::RISE * (1.0 - horizon_theme::ease_out_cubic(t)) * dpi,
        None => 0.0,
    };
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
            let page = model.page;
            if page != LauncherPage::Settings {
                SETTINGS_PRESS.set(None);
                SETTINGS_DRAG.set(None);
                SETTINGS_SWALLOW.set(false);
                SETTINGS_SCROLL.set(0.0);
                SETTINGS_HELD.set(ui.io().mouse_down[0]);
            }
            // The stone page behind everything: the desktop's own texture, tiled.
            {
                let draw = ui.get_window_draw_list();
                fill(&draw, [0.0, 0.0], [width, height], c.page, 1.0, 0.0);
                if let Some((texture, size)) = stone {
                    // The tile is one texture drawn repeatedly at its own size, which is
                    // what `blocks::stone` does on the desktop.
                    let tile = size[0] as f32;
                    if tile > 0.0 {
                        let cols = (width / tile).ceil() as usize;
                        let rows = (height / tile).ceil() as usize;
                        for row in 0..rows {
                            for col in 0..cols {
                                let x = col as f32 * tile;
                                let y = row as f32 * tile;
                                draw.add_image(texture, [x, y], [x + tile, y + tile])
                                    .build();
                            }
                        }
                    }
                }
            }
            // The top bar spans the window; the sidebar starts under it.
            let bar = top_bar(ui, model, fonts, c, dpi, width, rise, &mut sink);
            sidebar(ui, model, fonts, c, dpi, &g, height, rise, &mut sink);
            let body_top = bar;
            match page {
                LauncherPage::Play => play_page(
                    ui,
                    model,
                    fonts,
                    c,
                    dpi,
                    &page_g,
                    height,
                    body_top,
                    background,
                    &mut sink,
                ),
                LauncherPage::Installations => installations_page(
                    ui, model, fonts, c, dpi, &page_g, height, body_top, &mut sink,
                ),
                LauncherPage::Skins => skins_page(
                    ui, model, fonts, c, dpi, &page_g, height, body_top, skin, &mut sink,
                ),
                LauncherPage::PatchNotes => stories_page(
                    ui, model, fonts, c, dpi, &page_g, height, body_top, true,
                ),
                LauncherPage::News => stories_page(
                    ui, model, fonts, c, dpi, &page_g, height, body_top, false,
                ),
                LauncherPage::Mods => mods_page(
                    ui, model, fonts, c, dpi, &page_g, height, body_top, &mut sink,
                ),
                LauncherPage::Profile => profile_page(
                    ui, model, fonts, c, dpi, &page_g, height, body_top, &mut sink,
                ),
                LauncherPage::Settings => settings_page(
                    ui, model, fonts, c, dpi, &page_g, height, body_top, &mut sink,
                ),
                LauncherPage::Files => files_page(
                    ui, model, fonts, c, dpi, &page_g, height, body_top, &mut sink,
                ),
                LauncherPage::Multiplayer => multiplayer_page(
                    ui, model, fonts, c, dpi, &page_g, height, body_top, &mut sink,
                ),
            }
            // The page change: the content fades up out of the page.
            if page_ease < 1.0 {
                let draw = ui.get_window_draw_list();
                fill(
                    &draw,
                    [g.x, body_top],
                    [width, height],
                    c.page,
                    (1.0 - page_ease) * PAGE_VEIL,
                    0.0,
                );
            }
            if let Some(t) = opening {
                let ease = horizon_theme::ease_out_cubic(t);
                if ease < 1.0 {
                    let draw = ui.get_window_draw_list();
                    fill(&draw, [0.0, 0.0], [width, height], c.page, 1.0 - ease, 0.0);
                }
            }
        });
    if model.editing {
        rename_dialog(ui, model, fonts, c, dpi, width, height, &mut sink);
    }
}

/// The top bar: the mark and the name, the destinations, the account chip and the
/// settings button. Returns the y the page starts at.
#[allow(clippy::too_many_arguments)]
fn top_bar(
    ui: &Ui,
    model: &mut LauncherModel,
    fonts: &Fonts,
    c: &Css,
    dpi: f32,
    width: f32,
    rise: f32,
    sink: &mut Sink<'_>,
) -> f32 {
    let h = TOPBAR * dpi;
    let o = OUTLINE * dpi;
    {
        let draw = ui.get_window_draw_list();
        fill(&draw, [0.0, 0.0], [width, h], c.side, 1.0, 0.0);
        // The rule under the bar, and its dark outline: the bar is a block too.
        fill(&draw, [0.0, h - o], [width, h], c.outline, 1.0, 0.0);
    }
    let dt = ui.io().delta_time;
    let before = NAV_HOVER.with(|x| x.get());
    let mut hovered: Option<LauncherPage> = None;

    // The mark and the wordmark, on the sidebar's own width.
    let side = (SIDEBAR * dpi).min(width * 0.34);
    {
        let draw = ui.get_window_draw_list();
        let m = 22.0 * dpi;
        let mx = 18.0 * dpi;
        let my = (h - m) / 2.0 + rise;
        // The mark: a violet block with a horizon cut through it.
        fill(&draw, [mx, my], [mx + m, my + m], c.face, 1.0, 0.0);
        stroke(&draw, [mx, my], [mx + m, my + m], c.face_bevel, 1.0, 0.0, 2.0);
        fill(
            &draw,
            [mx + 4.0 * dpi, my + m * 0.52],
            [mx + m - 4.0 * dpi, my + m * 0.52 + 2.0 * dpi],
            c.on_face,
            0.9,
            0.0,
        );
        fill(
            &draw,
            [mx + 4.0 * dpi, my + m * 0.64],
            [mx + m - 4.0 * dpi, my + m * 0.64 + 2.0 * dpi],
            c.on_face,
            0.5,
            0.0,
        );
        let _f = ui.push_font(fonts.title);
        draw.add_text(
            [mx + m + 10.0 * dpi, my + 2.0 * dpi],
            rgba(c.ink, 1.0),
            "HORIZON",
        );
    }

    // The destinations.
    let mut x = side + 6.0 * dpi;
    let nav_h = h - 2.0 * o;
    for (i, page) in PAGES.iter().enumerate() {
        let page = *page;
        let selected = page == model.page
            || (page == LauncherPage::Settings && model.page == LauncherPage::Settings);
        let label = page.label();
        let tw = text_width(ui, fonts.label, label);
        let w = tw + 18.0 * dpi + 18.0 * dpi + 8.0 * dpi;
        let glow = NAV_GLOW.with(|g| g.get()[i]);
        {
            let draw = ui.get_window_draw_list();
            if selected {
                fill(&draw, [x, 0.0], [x + w, h - o], c.raised, 1.0, 0.0);
                // The accent underline: the desktop's marker for the open page.
                fill(
                    &draw,
                    [x + 8.0 * dpi, h - o - 4.0 * dpi],
                    [x + w - 8.0 * dpi, h - o],
                    c.accent,
                    1.0,
                    0.0,
                );
            } else if glow > 0.004 {
                fill(
                    &draw,
                    [x, 0.0],
                    [x + w, h - o],
                    c.raised,
                    glow * NAV_GLOW_ALPHA,
                    0.0,
                );
            }
            icon(
                &draw,
                page.icon(),
                x + 9.0 * dpi,
                (h - 18.0 * dpi) / 2.0 + rise,
                18.0 * dpi,
                if selected { c.accent } else { c.muted },
            );
            let _f = ui.push_font(fonts.label);
            let [_, th] = ui.calc_text_size(label);
            draw.add_text(
                [x + 18.0 * dpi + 9.0 * dpi, (h - th) / 2.0 + rise],
                rgba(if selected { c.ink } else { c.muted }, 1.0),
                label,
            );
        }
        ui.set_cursor_screen_pos([x, 0.0]);
        let id = format!("##nav{i}");
        let clicked = ui.invisible_button(id.as_str(), [w, h - o]);
        if MOTION.with(|m| m.get()) && (ui.is_item_hovered() || ui.is_item_active()) {
            hovered = Some(page);
        }
        if clicked {
            // The model is written here as well as the event pushed, so the page the
            // glass shows changes on this frame rather than the next model report.
            model.page = page;
            sink.push(LauncherEvent::Goto(page));
        }
        x += w;
    }

    // The account chip and the settings button, right-aligned.
    let chip_h = 40.0 * dpi;
    let chip_y = (h - chip_h) / 2.0;
    let settings_w = chip_h;
    let sx = width - 12.0 * dpi - settings_w;
    {
        let draw = ui.get_window_draw_list();
        let settings_open = model.page == LauncherPage::Settings;
        fill(
            &draw,
            [sx, chip_y],
            [sx + settings_w, chip_y + chip_h],
            if settings_open { c.raised } else { c.panel },
            1.0,
            0.0,
        );
        stroke(
            &draw,
            [sx, chip_y],
            [sx + settings_w, chip_y + chip_h],
            c.outline,
            1.0,
            0.0,
            o,
        );
        edges(&draw, [sx, chip_y], [sx + settings_w, chip_y + chip_h], c.bevel, c.lip, dpi);
        icon(
            &draw,
            "sliders",
            sx + (settings_w - 20.0 * dpi) / 2.0,
            chip_y + (chip_h - 20.0 * dpi) / 2.0,
            20.0 * dpi,
            if settings_open { c.accent } else { c.ink_soft },
        );
        if settings_open {
            fill(
                &draw,
                [sx, chip_y + chip_h - 4.0 * dpi],
                [sx + settings_w, chip_y + chip_h],
                c.accent,
                1.0,
                0.0,
            );
        }
    }
    ui.set_cursor_screen_pos([sx, chip_y]);
    if ui.invisible_button("##settingsbtn", [settings_w, chip_h]) {
        sink.push(LauncherEvent::Goto(LauncherPage::Settings));
    }

    let name = fit(ui, &model.account, 120.0 * dpi);
    let name_w = text_width(ui, fonts.label, &name);
    let chip_w = 4.0 * dpi + 30.0 * dpi + 10.0 * dpi + name_w + 12.0 * dpi;
    let cx = sx - 8.0 * dpi - chip_w;
    {
        let draw = ui.get_window_draw_list();
        fill(&draw, [cx, chip_y], [cx + chip_w, chip_y + chip_h], c.panel, 1.0, 0.0);
        stroke(
            &draw,
            [cx, chip_y],
            [cx + chip_w, chip_y + chip_h],
            c.outline,
            1.0,
            0.0,
            o,
        );
        edges(&draw, [cx, chip_y], [cx + chip_w, chip_y + chip_h], c.bevel, c.lip, dpi);
        // The avatar: the player's initial on the accent, because the account is the
        // one place the launcher says who is playing.
        let face = 30.0 * dpi;
        let fx = cx + 4.0 * dpi;
        let fy = chip_y + (chip_h - face) / 2.0;
        fill(&draw, [fx, fy], [fx + face, fy + face], c.well, 1.0, 0.0);
        stroke(&draw, [fx, fy], [fx + face, fy + face], c.outline, 1.0, 0.0, o);
        icon(
            &draw,
            "user",
            fx + (face - 18.0 * dpi) / 2.0,
            fy + (face - 18.0 * dpi) / 2.0,
            18.0 * dpi,
            c.muted,
        );
        let _f = ui.push_font(fonts.label);
        let [_, th] = ui.calc_text_size(name.as_str());
        draw.add_text(
            [fx + face + 10.0 * dpi, chip_y + (chip_h - th) / 2.0],
            rgba(c.ink, 1.0),
            name.as_str(),
        );
    }
    ui.set_cursor_screen_pos([cx, chip_y]);
    if ui.invisible_button("##accountchip", [chip_w, chip_h]) {
        sink.push(LauncherEvent::Goto(LauncherPage::Profile));
    }

    NAV_GLOW.with(|g| {
        let mut all = g.get();
        for (i, page) in PAGES.iter().enumerate() {
            let on = hovered == Some(*page);
            let target = if on { 1.0 } else { 0.0 };
            let from = if on && before != Some(*page) {
                0.0
            } else {
                all[i]
            };
            all[i] = ease_glow(from, target, dt);
        }
        g.set(all);
    });
    NAV_HOVER.with(|x| x.set(hovered));
    h
}

/// The sidebar: the instance, the player's own builds, then the launcher's version.
#[allow(clippy::too_many_arguments)]
fn sidebar(
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
    let w = g.side;
    let top = g.top;
    let o = OUTLINE * dpi;
    {
        let draw = ui.get_window_draw_list();
        fill(&draw, [0.0, top], [w, height], c.side, 1.0, 0.0);
        fill(&draw, [w - o, top], [w, height], c.outline, 1.0, 0.0);
    }
    let pad = 18.0 * dpi;
    let mut y = top + 14.0 * dpi;
    // The section heading, with the add button beside it.
    {
        let draw = ui.get_window_draw_list();
        let _f = ui.push_font(fonts.label);
        draw.add_text(
            [pad + 6.0 * dpi, y + 6.0 * dpi],
            rgba(c.muted, 1.0),
            "INSTANCES",
        );
    }
    let add = 24.0 * dpi;
    let ax = w - pad - add;
    {
        let draw = ui.get_window_draw_list();
        fill(&draw, [ax, y], [ax + add, y + add], c.panel, 1.0, 0.0);
        stroke(&draw, [ax, y], [ax + add, y + add], c.outline, 1.0, 0.0, o);
        edges(&draw, [ax, y], [ax + add, y + add], c.bevel, c.lip, dpi);
        icon(
            &draw,
            "plus",
            ax + (add - 14.0 * dpi) / 2.0,
            y + (add - 14.0 * dpi) / 2.0,
            14.0 * dpi,
            c.ink_soft,
        );
    }
    ui.set_cursor_screen_pos([ax, y]);
    if ui.invisible_button("##addinstance", [add, add]) {
        sink.push(LauncherEvent::NewInstance);
    }
    y += add + 8.0 * dpi;

    // The instance row: a grass-block slot, the name, and a dot for whether it is
    // ready. It is the one row that is always there.
    let row_h = 56.0 * dpi;
    let rw = w - pad * 2.0;
    let selected = model.page == LauncherPage::Play;
    {
        let draw = ui.get_window_draw_list();
        if selected {
            fill(&draw, [pad, y], [pad + rw, y + row_h], c.raised, 1.0, 0.0);
        }
        // The slot.
        let slot = 44.0 * dpi;
        let sx = pad + 6.0 * dpi;
        let sy = y + (row_h - slot) / 2.0;
        fill(&draw, [sx, sy], [sx + slot, sy + slot], c.well, 1.0, 0.0);
        stroke(&draw, [sx, sy], [sx + slot, sy + slot], c.outline, 1.0, 0.0, o);
        // A grass block, which is what the desktop draws from the player's own files
        // when it has them and this when it does not.
        let gx = sx + 4.0 * dpi;
        let gy = sy + 4.0 * dpi;
        let gs = slot - 8.0 * dpi;
        fill(&draw, [gx, gy], [gx + gs, gy + gs * 0.34], 0x5bbf4a, 1.0, 0.0);
        fill(&draw, [gx, gy + gs * 0.30], [gx + gs, gy + gs * 0.42], 0x74d95f, 1.0, 0.0);
        fill(&draw, [gx, gy + gs * 0.42], [gx + gs, gy + gs], 0x7a5433, 1.0, 0.0);
        fill(
            &draw,
            [gx + gs * 0.15, gy + gs * 0.60],
            [gx + gs * 0.34, gy + gs * 0.72],
            0x63421f,
            1.0,
            0.0,
        );
        fill(
            &draw,
            [gx + gs * 0.60, gy + gs * 0.76],
            [gx + gs * 0.84, gy + gs * 0.88],
            0x63421f,
            1.0,
            0.0,
        );
        // The dot: green when the build is ready, amber while it still needs preparing.
        let dot = 10.0 * dpi;
        let dx = sx + slot - dot * 0.8;
        let dy = sy + slot - dot * 0.8;
        fill(&draw, [dx, dy], [dx + dot, dy + dot], if model.playable { c.success } else { c.warn }, 1.0, 0.0);
        stroke(&draw, [dx, dy], [dx + dot, dy + dot], c.side, 1.0, 0.0, o);
        // The name and its one line.
        let tx = sx + slot + 10.0 * dpi;
        {
            let _f = ui.push_font(fonts.label);
            draw.add_text(
                [tx, y + 10.0 * dpi],
                rgba(c.ink, 1.0),
                fit(ui, "Horizon", w - tx - pad),
            );
        }
        {
            let _f = ui.push_font(fonts.small);
            let sub = if model.instance.is_empty() {
                "No version installed"
            } else if model.playable {
                "Vanilla · ready"
            } else {
                "Needs preparing"
            };
            draw.add_text(
                [tx, y + 30.0 * dpi],
                rgba(c.muted, 1.0),
                fit(ui, sub, w - tx - pad),
            );
        }
    }
    ui.set_cursor_screen_pos([pad, y]);
    if ui.invisible_button("##instance", [rw, row_h]) {
        sink.push(LauncherEvent::Goto(LauncherPage::Play));
    }
    y += row_h + 10.0 * dpi;

    // The destinations that are not in the bar: the skins and the files.
    for (i, page) in [LauncherPage::Skins, LauncherPage::Files].iter().enumerate() {
        let page = *page;
        let selected = model.page == page;
        let h = 36.0 * dpi;
        {
            let draw = ui.get_window_draw_list();
            if selected {
                fill(&draw, [pad, y], [pad + rw, y + h], c.raised, 1.0, 0.0);
            }
            icon(
                &draw,
                page.icon(),
                pad + 8.0 * dpi,
                y + (h - 16.0 * dpi) / 2.0,
                16.0 * dpi,
                if selected { c.accent } else { c.muted },
            );
            let _f = ui.push_font(fonts.label);
            let [_, th] = ui.calc_text_size(page.label());
            draw.add_text(
                [pad + 32.0 * dpi, y + (h - th) / 2.0],
                rgba(if selected { c.ink } else { c.muted }, 1.0),
                page.label(),
            );
        }
        ui.set_cursor_screen_pos([pad, y]);
        let id = format!("##side{i}");
        if ui.invisible_button(id.as_str(), [rw, h]) {
            sink.push(LauncherEvent::Goto(page));
        }
        y += h + 2.0 * dpi;
    }

    // The launcher's own version, at the very bottom.
    {
        let draw = ui.get_window_draw_list();
        let _f = ui.push_font(fonts.small);
        draw.add_text(
            [pad, height + rise - 24.0 * dpi],
            rgba(c.muted, 0.8),
            "Launcher 0.1.0",
        );
    }
}

/// The Play page: the banner with its landscape, the hero, and the row of actions.
#[allow(clippy::too_many_arguments)]
fn play_page(
    ui: &Ui,
    model: &LauncherModel,
    fonts: &Fonts,
    c: &Css,
    dpi: f32,
    g: &Grid,
    height: f32,
    top: f32,
    background: Option<(TextureId, [u32; 2])>,
    sink: &mut Sink<'_>,
) {
    let playable = model.playable && !model.instance.is_empty();
    let o = OUTLINE * dpi;
    let banner_h = (340.0 * dpi).min(height - top - 8.0 * dpi).max(220.0 * dpi);
    let x = g.x + g.pad;
    let w = g.w - g.pad * 2.0;
    // The banner: the landscape, a veil of the page colour, then the hero and the
    // actions over it. The desktop reaches it to the page's edges and the top bar.
    {
        let draw = ui.get_window_draw_list();
        fill(&draw, [g.x, top], [g.x + g.w, top + banner_h], c.page, 1.0, 0.0);
        if let Some((texture, size)) = background {
            artwork(&draw, texture, size, [g.x, top], [g.x + g.w, top + banner_h]);
        }
        // The veils: one from the top so the hero reads, one from the bottom.
        draw.add_rect_filled_multicolor(
            [g.x, top],
            [g.x + g.w, top + banner_h * 0.55],
            rgba(c.page, 0.55),
            rgba(c.page, 0.55),
            rgba(c.page, 0.0),
            rgba(c.page, 0.0),
        );
        draw.add_rect_filled_multicolor(
            [g.x, top + banner_h * 0.35],
            [g.x + g.w, top + banner_h],
            rgba(c.page, 0.0),
            rgba(c.page, 0.0),
            rgba(c.page, 0.82),
            rgba(c.page, 0.82),
        );
        // The outline under the banner, which is what makes it a block.
        fill(
            &draw,
            [g.x, top + banner_h - o],
            [g.x + g.w, top + banner_h],
            c.outline,
            1.0,
            0.0,
        );
    }

    // The hero: the status overline, the wordmark, and the version line.
    let hero_x = x;
    let mut hy = top + 24.0 * dpi;
    {
        let draw = ui.get_window_draw_list();
        let _f = ui.push_font(fonts.body);
        let overline = if model.instance.is_empty() {
            "No version installed"
        } else if model.playable {
            "Minecraft 26.3"
        } else {
            "Minecraft 26.3 · Needs preparing"
        };
        draw.add_text([hero_x, hy], rgba(c.ink_soft, 0.9), overline);
    }
    hy += 22.0 * dpi;
    {
        let draw = ui.get_window_draw_list();
        let _f = ui.push_font(fonts.display);
        draw.add_text([hero_x, hy], rgba(c.ink, 1.0), "Horizon");
    }
    hy += 46.0 * dpi;
    {
        let draw = ui.get_window_draw_list();
        let _f = ui.push_font(fonts.body);
        let line = format!(
            "{} · Minecraft: Java Edition 26.3, written in Rust.",
            model.version
        );
        draw.add_text(
            [hero_x, hy],
            rgba(c.ink_soft, 1.0),
            fit(ui, &line, w * 0.62),
        );
    }

    // The actions row, pinned to the bottom of the banner: the client/server segments
    // and the version picker on the left, Play on the right.
    let row_h = 60.0 * dpi;
    let row_y = top + banner_h - row_h - 24.0 * dpi;
    let seg_w = 132.0 * dpi;
    {
        let draw = ui.get_window_draw_list();
        let seg_h = 30.0 * dpi;
        let seg_y = row_y + (row_h - seg_h) / 2.0;
        well(&draw, c, [hero_x, seg_y], [hero_x + seg_w, seg_y + seg_h], dpi);
        let half = (seg_w - 4.0 * dpi) / 2.0;
        // Client is the chosen segment: a raised block on the sunk track.
        fill(
            &draw,
            [hero_x + 2.0 * dpi, seg_y + 2.0 * dpi],
            [hero_x + 2.0 * dpi + half, seg_y + seg_h - 2.0 * dpi],
            c.button,
            1.0,
            0.0,
        );
        edges(
            &draw,
            [hero_x + 2.0 * dpi, seg_y + 2.0 * dpi],
            [hero_x + 2.0 * dpi + half, seg_y + seg_h - 2.0 * dpi],
            c.button_bevel,
            c.lip,
            dpi,
        );
        {
            let _f = ui.push_font(fonts.label);
            let [cw, ch] = ui.calc_text_size("Client");
            draw.add_text(
                [hero_x + 2.0 * dpi + (half - cw) / 2.0, seg_y + (seg_h - ch) / 2.0],
                rgba(c.ink, 1.0),
                "Client",
            );
            let [sw, sh] = ui.calc_text_size("Server");
            draw.add_text(
                [
                    hero_x + 2.0 * dpi + half + (half - sw) / 2.0,
                    seg_y + (seg_h - sh) / 2.0,
                ],
                rgba(c.muted, 1.0),
                "Server",
            );
        }
    }

    // The version picker: a raised block with the version and a chevron.
    let pick_w = 116.0 * dpi;
    let pick_x = hero_x + seg_w + 12.0 * dpi;
    {
        let draw = ui.get_window_draw_list();
        let h = 34.0 * dpi;
        let py = row_y + (row_h - h) / 2.0;
        fill(&draw, [pick_x, py], [pick_x + pick_w, py + h], c.panel, 1.0, 0.0);
        stroke(&draw, [pick_x, py], [pick_x + pick_w, py + h], c.outline, 1.0, 0.0, o);
        edges(&draw, [pick_x, py], [pick_x + pick_w, py + h], c.bevel, c.lip, dpi);
        {
            let _f = ui.push_font(fonts.mono);
            let [_, th] = ui.calc_text_size(&model.version);
            draw.add_text(
                [pick_x + 10.0 * dpi, py + (h - th) / 2.0],
                rgba(c.ink_soft, 1.0),
                fit(ui, &model.version, pick_w - 40.0 * dpi),
            );
        }
        icon(
            &draw,
            "chevron-down",
            pick_x + pick_w - 24.0 * dpi,
            py + (h - 14.0 * dpi) / 2.0,
            14.0 * dpi,
            c.muted,
        );
    }
    ui.set_cursor_screen_pos([pick_x, row_y]);
    if ui.invisible_button("##versionpick", [pick_w, row_h]) {
        sink.push(LauncherEvent::Goto(LauncherPage::Installations));
    }

    // Play: the primary action, the biggest block on the page.
    let play_w = 200.0 * dpi;
    let play_x = g.right(play_w);
    let label = match &model.front {
        Some(text) => text.clone(),
        None if playable => "Play".to_string(),
        None => "Install".to_string(),
    };
    if primary_button(
        ui, fonts, c, dpi, play_x, row_y, play_w, row_h, &label, playable, sink,
    ) {
        sink.push(match model.front.as_deref() {
            Some(_) => LauncherEvent::Restart,
            None if playable => LauncherEvent::Play,
            None => LauncherEvent::Install,
        });
    }

    // Under the banner: the readiness index, one row per thing that has to be true.
    let mut y = top + banner_h + 22.0 * dpi;
    {
        let draw = ui.get_window_draw_list();
        icon(&draw, "check", x, y, 18.0 * dpi, c.accent);
        let _f = ui.push_font(fonts.title);
        draw.add_text([x + 26.0 * dpi, y], rgba(c.ink, 1.0), "Ready to play");
    }
    y += 34.0 * dpi;
    let status = if model.status.is_empty() {
        "Prepared".to_string()
    } else {
        model.status.clone()
    };
    let rows = [
        ("Resources", status, if model.playable { "Prepared" } else { "Prepare" }),
        ("Minecraft folder", "The game's own files.".to_string(), "Choose..."),
    ];
    let row_h2 = 52.0 * dpi;
    for (i, (label, value, action)) in rows.iter().enumerate() {
        if y + row_h2 > height {
            break;
        }
        {
            let draw = ui.get_window_draw_list();
            // The index number, in the pixel face.
            {
                let _f = ui.push_font(fonts.label);
                draw.add_text(
                    [x + 4.0 * dpi, y + row_h2 / 2.0 - 9.0 * dpi],
                    rgba(c.muted, 1.0),
                    format!("{}", i + 1),
                );
            }
            {
                let _f = ui.push_font(fonts.label);
                draw.add_text([x + 26.0 * dpi, y + 12.0 * dpi], rgba(c.ink, 1.0), *label);
            }
            {
                let _f = ui.push_font(fonts.small);
                draw.add_text(
                    [x + 26.0 * dpi, y + 30.0 * dpi],
                    rgba(c.muted, 1.0),
                    fit(ui, value, w * 0.5),
                );
            }
            // The rule between rows, as the desktop draws it.
            if i + 1 < rows.len() {
                fill(
                    &draw,
                    [x, y + row_h2 - 1.0 * dpi],
                    [x + w, y + row_h2],
                    c.rule,
                    1.0,
                    0.0,
                );
            }
        }
        // The row's action button, on the right.
        let bw = 96.0 * dpi;
        let bh = 30.0 * dpi;
        let bx = x + w - bw;
        let by = y + (row_h2 - bh) / 2.0;
        if secondary_button(ui, fonts, c, dpi, bx, by, bw, bh, action) {
            if i == 0 {
                sink.push(LauncherEvent::Install);
            } else {
                sink.push(LauncherEvent::RevealFiles);
            }
        }
        y += row_h2;
    }

    // The download's progress, when there is one.
    if let Some(p) = model.progress {
        let p = p.clamp(0.0, 1.0);
        let draw = ui.get_window_draw_list();
        let bar_y = y + 10.0 * dpi;
        if bar_y + 8.0 * dpi <= height {
            well(&draw, c, [x, bar_y], [x + w * 0.5, bar_y + 8.0 * dpi], dpi);
            fill(
                &draw,
                [x + 2.0 * dpi, bar_y + 2.0 * dpi],
                [x + 2.0 * dpi + (w * 0.5 - 4.0 * dpi) * p, bar_y + 6.0 * dpi],
                c.accent,
                1.0,
                0.0,
            );
        }
    }
}

/// The landscape behind the hero, drawn texel for texel.
///
/// The desktop bakes this at the window's scale so each texel is a whole block of
/// device pixels, and draws it at exactly that size. The same guarantee here comes
/// from the sampler: the texture is uploaded with nearest-neighbour filtering, and
/// it is drawn at a whole multiple of its texel size, so no texel is ever half a
/// pixel and the edges stay sharp.
fn artwork(
    draw: &DrawListMut<'_>,
    texture: TextureId,
    size: [u32; 2],
    min: [f32; 2],
    max: [f32; 2],
) {
    let (tw, th) = (size[0] as f32, size[1] as f32);
    if tw <= 0.0 || th <= 0.0 {
        return;
    }
    // The landscape is as tall as the banner and as wide as it needs to be; the
    // banner clips what runs past its right edge, which is what the desktop does.
    let (bw, bh) = (max[0] - min[0], max[1] - min[1]);
    let scale = (bh / th).max(1.0);
    let dw = tw * scale;
    let dh = th * scale;
    let _ = bw;
    draw.add_image(texture, [min[0], max[1] - dh], [min[0] + dw, max[1]])
        .build();
}
/// A block button: a face, a two-pixel outline, a light top edge and a darker lip.
/// `Primary` is the main action's violet; `Secondary` is the stone button; a press
/// drops the label one pixel and dims the face.
///
/// This is the desktop's `button()`: the three kinds it has, and no others.
#[allow(clippy::too_many_arguments)]
fn block_button(
    ui: &Ui,
    fonts: &Fonts,
    c: &Css,
    dpi: f32,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    label: &str,
    kind: Kind,
) -> bool {
    let id = format!("##btn{label}{x:.0}{y:.0}");
    ui.set_cursor_screen_pos([x, y]);
    let clicked = ui.invisible_button(id.as_str(), [w, h]);
    let down = MOTION.with(|m| m.get()) && ui.is_item_active();
    let hovered = ui.is_item_hovered();
    let o = OUTLINE * dpi;
    {
        let draw = ui.get_window_draw_list();
        let (face, top, lip, ink) = match kind {
            Kind::Primary => (c.face, c.face_bevel, c.face_lip, c.on_face),
            Kind::Secondary => (c.button, c.button_bevel, c.lip, c.ink),
            Kind::Ghost => (c.page, c.page, c.page, c.muted),
        };
        if kind != Kind::Ghost {
            let face = if down { darker(face) } else { face };
            stroke(&draw, [x, y], [x + w, y + h], c.outline, 1.0, 0.0, o);
            fill(
                &draw,
                [x + o, y + o],
                [x + w - o, y + h - o],
                face,
                1.0,
                0.0,
            );
            edges(&draw, [x, y], [x + w, y + h], top, lip, dpi);
            // The hover layer, inside the outline: the desktop raises the face.
            if hovered && !down {
                fill(
                    &draw,
                    [x + o, y + o],
                    [x + w - o, y + h - o],
                    if kind == Kind::Primary { c.on_face } else { c.bevel },
                    if kind == Kind::Primary { 0.10 } else { 0.18 },
                    0.0,
                );
            }
        } else if hovered {
            fill(&draw, [x, y], [x + w, y + h], c.raised, 0.55, 0.0);
        }
        let _f = ui.push_font(fonts.label);
        let [tw, th] = ui.calc_text_size(label);
        // The label sits on the face, above the lip; a press pushes it down.
        let drop = if down { PRESS_DROP * dpi } else { 0.0 };
        draw.add_text(
            [x + (w - tw) / 2.0, y + (h - th) / 2.0 - 1.0 * dpi + drop],
            rgba(ink, 1.0),
            label,
        );
    }
    clicked
}

/// The desktop's three button kinds.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Primary,
    Secondary,
    Ghost,
}

fn primary_button(
    ui: &Ui,
    fonts: &Fonts,
    c: &Css,
    dpi: f32,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    label: &str,
    enabled: bool,
    _sink: &mut Sink<'_>,
) -> bool {
    if !enabled {
        // A disabled Play is still a block: the same shape, dimmed, with no face.
        let o = OUTLINE * dpi;
        let draw = ui.get_window_draw_list();
        stroke(&draw, [x, y], [x + w, y + h], c.outline, 1.0, 0.0, o);
        fill(&draw, [x + o, y + o], [x + w - o, y + h - o], c.button, 0.55, 0.0);
        let _f = ui.push_font(fonts.label);
        let [tw, th] = ui.calc_text_size(label);
        draw.add_text(
            [x + (w - tw) / 2.0, y + (h - th) / 2.0],
            rgba(c.muted, 0.7),
            label,
        );
        return false;
    }
    block_button(ui, fonts, c, dpi, x, y, w, h, label, Kind::Primary)
}

fn secondary_button(
    ui: &Ui,
    fonts: &Fonts,
    c: &Css,
    dpi: f32,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    label: &str,
) -> bool {
    block_button(ui, fonts, c, dpi, x, y, w, h, label, Kind::Secondary)
}

/// A page's title in the pixel face and one line on what it is for.
fn page_header(ui: &Ui, model: &LauncherModel, fonts: &Fonts, c: &Css, dpi: f32, g: &Grid, top: f32) -> f32 {
    let x = g.x + g.pad;
    let y = top + 28.0 * dpi;
    {
        let draw = ui.get_window_draw_list();
        let _f = ui.push_font(fonts.display);
        draw.add_text([x, y], rgba(c.ink, 1.0), model.page.label());
    }
    let mut bottom = y + 46.0 * dpi;
    if !model.page.subtitle().is_empty() {
        let draw = ui.get_window_draw_list();
        let _f = ui.push_font(fonts.body);
        draw.add_text(
            [x, bottom],
            rgba(c.ink_soft, 1.0),
            fit(ui, model.page.subtitle(), g.w - g.pad * 2.0),
        );
        bottom += 24.0 * dpi;
    }
    bottom + 8.0 * dpi
}

/// A row of a list: a panel with a title, a line under it, a badge, and actions.
#[allow(clippy::too_many_arguments)]
fn list_row(
    ui: &Ui,
    fonts: &Fonts,
    c: &Css,
    dpi: f32,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    title: &str,
    subtitle: &str,
    badge: &str,
    selected: bool,
) -> bool {
    let id = format!("##row{x:.0}_{y:.0}");
    ui.set_cursor_screen_pos([x, y]);
    let clicked = ui.invisible_button(id.as_str(), [w, h]);
    let hovered = ui.is_item_hovered();
    {
        let draw = ui.get_window_draw_list();
        // A row is a plate, not a card: a raised block when it is chosen, a flat one
        // otherwise. The desktop separates rows with a rule rather than with gaps.
        let face = if selected { c.raised } else { c.panel };
        fill(&draw, [x, y], [x + w, y + h], face, if hovered { 1.0 } else { 0.0 }, 0.0);
        if selected {
            fill(&draw, [x, y], [x + 4.0 * dpi, y + h], c.accent, 1.0, 0.0);
        }
        {
            let _f = ui.push_font(fonts.label);
            draw.add_text(
                [x + 14.0 * dpi, y + 12.0 * dpi],
                rgba(c.ink, 1.0),
                fit(ui, title, w - 28.0 * dpi - 120.0 * dpi),
            );
        }
        if !subtitle.is_empty() {
            let _f = ui.push_font(fonts.small);
            draw.add_text(
                [x + 14.0 * dpi, y + 32.0 * dpi],
                rgba(c.muted, 1.0),
                fit(ui, subtitle, w - 28.0 * dpi - 120.0 * dpi),
            );
        }
        if !badge.is_empty() {
            let _f = ui.push_font(fonts.label);
            let [bw, bh] = ui.calc_text_size(badge);
            // The badge is a small block, the way the desktop draws "Installed".
            let pad = 8.0 * dpi;
            let bx = x + 14.0 * dpi + text_width(ui, fonts.label, title) + 10.0 * dpi;
            if bx + bw + pad * 2.0 < x + w - 120.0 * dpi {
                fill(
                    &draw,
                    [bx, y + 12.0 * dpi],
                    [bx + bw + pad * 2.0, y + 12.0 * dpi + bh + 6.0 * dpi],
                    c.button,
                    1.0,
                    0.0,
                );
                stroke(
                    &draw,
                    [bx, y + 12.0 * dpi],
                    [bx + bw + pad * 2.0, y + 12.0 * dpi + bh + 6.0 * dpi],
                    c.outline,
                    1.0,
                    0.0,
                    OUTLINE * dpi,
                );
                draw.add_text(
                    [bx + pad, y + 15.0 * dpi],
                    rgba(c.ink_soft, 1.0),
                    badge,
                );
            }
        }
    }
    clicked
}

/// Versions: the builds that can be installed, newest first.
#[allow(clippy::too_many_arguments)]
fn installations_page(
    ui: &Ui,
    model: &LauncherModel,
    fonts: &Fonts,
    c: &Css,
    dpi: f32,
    g: &Grid,
    height: f32,
    top: f32,
    sink: &mut Sink<'_>,
) {
    let mut y = page_header(ui, model, fonts, c, dpi, g, top);
    let x = g.x + g.pad;
    let w = g.w - g.pad * 2.0;
    let row_h = 58.0 * dpi;
    for row in &model.rows {
        if y + row_h > height {
            break;
        }
        if list_row(
            ui,
            fonts,
            c,
            dpi,
            x,
            y,
            w,
            row_h,
            &row.title,
            &row.subtitle,
            &row.badge,
            row.selected,
        ) {
            sink.push(LauncherEvent::SelectRow(row.id));
        }
        // The row's action, right-aligned: Install, or In use.
        let bw = 90.0 * dpi;
        let bh = 30.0 * dpi;
        let bx = x + w - bw - 12.0 * dpi;
        let by = y + (row_h - bh) / 2.0;
        if row.selected {
            text_note(ui, fonts, c, bx, by, bw, bh, "In use");
        } else if secondary_button(ui, fonts, c, dpi, bx, by, bw, bh, "Install") {
            sink.push(LauncherEvent::SelectRow(row.id));
        }
        // The rule under the row.
        {
            let draw = ui.get_window_draw_list();
            fill(
                &draw,
                [x, y + row_h],
                [x + w, y + row_h + 1.0 * dpi],
                c.rule,
                1.0,
                0.0,
            );
        }
        y += row_h + 1.0 * dpi;
    }
    if model.rows.is_empty() {
        let draw = ui.get_window_draw_list();
        let _f = ui.push_font(fonts.body);
        draw.add_text([x, y], rgba(c.muted, 1.0), "No version installed yet.");
        y += 30.0 * dpi;
    }
    y += 14.0 * dpi;
    if y + 34.0 * dpi <= height {
        if secondary_button(ui, fonts, c, dpi, x, y, 130.0 * dpi, 34.0 * dpi, "New instance") {
            sink.push(LauncherEvent::NewInstance);
        }
    }
}

/// A quiet label where a button would be.
#[allow(clippy::too_many_arguments)]
fn text_note(
    ui: &Ui,
    fonts: &Fonts,
    c: &Css,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    label: &str,
) {
    let draw = ui.get_window_draw_list();
    let _f = ui.push_font(fonts.label);
    let [tw, th] = ui.calc_text_size(label);
    draw.add_text(
        [x + (w - tw) / 2.0, y + (h - th) / 2.0],
        rgba(c.muted, 1.0),
        label,
    );
}

/// Patch notes and What's next: a column of entries with a tag each.
#[allow(clippy::too_many_arguments)]
fn stories_page(
    ui: &Ui,
    model: &LauncherModel,
    fonts: &Fonts,
    c: &Css,
    dpi: f32,
    g: &Grid,
    height: f32,
    top: f32,
    patches: bool,
) {
    let mut y = page_header(ui, model, fonts, c, dpi, g, top);
    let x = g.x + g.pad;
    let w = (g.w - g.pad * 2.0).min(720.0 * dpi);
    let stories = if patches { &model.patches } else { &model.news };
    for story in stories {
        let body = wrap(ui, &story.body, w);
        let h = 58.0 * dpi + body.len() as f32 * 19.0 * dpi;
        if y + h > height {
            break;
        }
        {
            let draw = ui.get_window_draw_list();
            {
                let _f = ui.push_font(fonts.title);
                draw.add_text(
                    [x, y],
                    rgba(c.ink, 1.0),
                    fit(ui, &story.title, w - 100.0 * dpi),
                );
            }
            if !story.tag.is_empty() {
                let _f = ui.push_font(fonts.label);
                let [tw, th] = ui.calc_text_size(story.tag.as_str());
                let bx = x + w - tw - 12.0 * dpi;
                fill(
                    &draw,
                    [bx, y - 2.0 * dpi],
                    [bx + tw + 12.0 * dpi, y + th + 4.0 * dpi],
                    c.button,
                    1.0,
                    0.0,
                );
                stroke(
                    &draw,
                    [bx, y - 2.0 * dpi],
                    [bx + tw + 12.0 * dpi, y + th + 4.0 * dpi],
                    c.outline,
                    1.0,
                    0.0,
                    OUTLINE * dpi,
                );
                draw.add_text(
                    [bx + 6.0 * dpi, y + 1.0 * dpi],
                    rgba(c.accent, 1.0),
                    story.tag.as_str(),
                );
            }
            let _f = ui.push_font(fonts.body);
            let mut by = y + 30.0 * dpi;
            for line in &body {
                draw.add_text([x, by], rgba(c.ink_soft, 1.0), line.as_str());
                by += 19.0 * dpi;
            }
            fill(
                &draw,
                [x, y + h - 10.0 * dpi],
                [x + w, y + h - 9.0 * dpi],
                c.rule,
                1.0,
                0.0,
            );
        }
        y += h;
    }
}

/// Skins: the player on the left, the library on the right.
#[allow(clippy::too_many_arguments)]
fn skins_page(
    ui: &Ui,
    model: &LauncherModel,
    fonts: &Fonts,
    c: &Css,
    dpi: f32,
    g: &Grid,
    height: f32,
    top: f32,
    skin: Option<TextureId>,
    sink: &mut Sink<'_>,
) {
    let mut y = page_header(ui, model, fonts, c, dpi, g, top);
    // What the last import did, under the page's title: the picker is a second
    // activity, so the tap that chose a file lands back here with no other sign that
    // anything happened. Without this line an import that failed and one that worked
    // looked exactly alike — a library that had not changed yet.
    if !model.status.is_empty() {
        let draw = ui.get_window_draw_list();
        let _f = ui.push_font(fonts.small);
        draw.add_text(
            [g.x + g.pad, y - 4.0 * dpi],
            rgba(c.muted, 1.0),
            fit(ui, &model.status, g.w - g.pad * 2.0),
        );
        y += 22.0 * dpi;
    }
    let x = g.x + g.pad;
    let avail_h = (height - y - g.pad).max(140.0 * dpi);
    let side_by_side = g.w > 760.0 * dpi;
    let model_w = if side_by_side {
        (avail_h * 0.62).clamp(200.0 * dpi, 320.0 * dpi)
    } else {
        g.w - g.pad * 2.0
    };
    {
        let draw = ui.get_window_draw_list();
        panel(&draw, c, [x, y], [x + model_w, y + avail_h], c.panel, dpi);
    }
    // The figure is a preview, not the page. A player model drawn to fill the card is
    // a wall of pixels with no room around it to see the silhouette, so it takes a
    // little over half the card and sits in the middle of the space above its name.
    let label_h = 44.0 * dpi;
    let figure_h = (avail_h - label_h - 24.0 * dpi).clamp(88.0 * dpi, 180.0 * dpi);
    let figure_top = y + (avail_h - label_h - figure_h).max(0.0) * 0.5;
    // Dragging the figure turns it. The drag is read before the figure is drawn, so
    // this frame already shows the angle the finger asked for.
    let (yaw, pitch) = drag_skin(ui, [x, y], [x + model_w, y + avail_h], dpi);
    match skin {
        Some(texture) => skin_figure(
            ui,
            &ui.get_window_draw_list(),
            texture,
            model.skin_size,
            x + model_w / 2.0,
            figure_top,
            figure_h,
            yaw,
            pitch,
        ),
        None => {
            let draw = ui.get_window_draw_list();
            let _f = ui.push_font(fonts.small);
            let text = "Loading the player...";
            let [tw, _] = ui.calc_text_size(text);
            draw.add_text(
                [x + (model_w - tw) / 2.0, y + avail_h / 2.0],
                rgba(c.muted, 1.0),
                text,
            );
        }
    }
    {
        let draw = ui.get_window_draw_list();
        let _f = ui.push_font(fonts.label);
        let name = model.chosen_skin.as_deref().unwrap_or("Default skin");
        let [nw, _] = ui.calc_text_size(name);
        draw.add_text(
            [x + (model_w - nw) / 2.0, y + avail_h - 34.0 * dpi],
            rgba(c.ink_soft, 1.0),
            fit(ui, name, model_w - 24.0 * dpi),
        );
    }

    let rx = if side_by_side {
        x + model_w + 20.0 * dpi
    } else {
        x
    };
    let rw = if side_by_side {
        g.x + g.w - g.pad - rx
    } else {
        g.w - g.pad * 2.0
    };
    let mut ry = if side_by_side {
        y
    } else {
        y + avail_h + 20.0 * dpi
    };
    if ry + 34.0 * dpi <= height {
        if secondary_button(ui, fonts, c, dpi, rx, ry, 180.0 * dpi, 34.0 * dpi, "Choose a skin") {
            sink.push(LauncherEvent::OpenSkins);
        }
        ry += 48.0 * dpi;
    }
    {
        let draw = ui.get_window_draw_list();
        let _f = ui.push_font(fonts.label);
        draw.add_text([rx, ry], rgba(c.muted, 1.0), "YOUR SKINS");
        ry += 24.0 * dpi;
    }
    if model.skins.is_empty() {
        // Scoped: the list below asks for its own draw list, and imgui allows one
        // live borrow per window.
        {
            let draw = ui.get_window_draw_list();
            let _f = ui.push_font(fonts.small);
            for (i, line) in wrap(
                ui,
                "No skins yet. A 64x64 PNG dropped in the import folder appears here.",
                rw,
            )
            .iter()
            .enumerate()
            {
                draw.add_text(
                    [rx, ry + i as f32 * 18.0 * dpi],
                    rgba(c.muted, 1.0),
                    line.as_str(),
                );
            }
        }
    } else {
        let row_h = 44.0 * dpi;
        for name in &model.skins {
            if ry + row_h > height {
                break;
            }
            let chosen = model.chosen_skin.as_deref() == Some(name.as_str());
            if list_row(
                ui,
                fonts,
                c,
                dpi,
                rx,
                ry,
                rw,
                row_h,
                name,
                "",
                if chosen { "Worn" } else { "" },
                chosen,
            ) {
                sink.push(LauncherEvent::PickSkin(name.clone()));
            }
            ry += row_h + 1.0 * dpi;
        }
    }
}

/// Mods: what the registry offers, and what is installed.
#[allow(clippy::too_many_arguments)]
fn mods_page(
    ui: &Ui,
    model: &LauncherModel,
    fonts: &Fonts,
    c: &Css,
    dpi: f32,
    g: &Grid,
    height: f32,
    top: f32,
    sink: &mut Sink<'_>,
) {
    let mut y = page_header(ui, model, fonts, c, dpi, g, top);
    let x = g.x + g.pad;
    let w = g.w - g.pad * 2.0;
    let two = g.w > 820.0 * dpi;
    let col_w = if two { (w - 20.0 * dpi) / 2.0 } else { w };

    {
        let draw = ui.get_window_draw_list();
        let _f = ui.push_font(fonts.label);
        let line = if model.mods_loading {
            "Loading the registry..."
        } else if model.mods_failed {
            "The registry could not be reached."
        } else {
            "AVAILABLE"
        };
        draw.add_text([x, y], rgba(c.muted, 1.0), line);
    }
    y += 24.0 * dpi;
    let row_h = 66.0 * dpi;
    for m in &model.mods {
        if y + row_h > height {
            break;
        }
        let installed = model.installed_mods.iter().any(|f| f.starts_with(&m.id));
        let busy = model.installing.as_deref() == Some(m.id.as_str());
        {
            let draw = ui.get_window_draw_list();
            fill(&draw, [x, y], [x + col_w, y + row_h], c.panel, 1.0, 0.0);
            fill(
                &draw,
                [x, y + row_h],
                [x + col_w, y + row_h + 1.0 * dpi],
                c.rule,
                1.0,
                0.0,
            );
            {
                let _f = ui.push_font(fonts.label);
                draw.add_text(
                    [x + 14.0 * dpi, y + 10.0 * dpi],
                    rgba(c.ink, 1.0),
                    fit(ui, &m.name, col_w - 130.0 * dpi),
                );
            }
            {
                let _f = ui.push_font(fonts.small);
                draw.add_text(
                    [x + 14.0 * dpi, y + 30.0 * dpi],
                    rgba(c.muted, 1.0),
                    fit(ui, &m.summary, col_w - 130.0 * dpi),
                );
                let _f = ui.push_font(fonts.mono);
                draw.add_text(
                    [x + 14.0 * dpi, y + 46.0 * dpi],
                    rgba(c.muted, 0.8),
                    fit(ui, &format!("{} · {}", m.id, m.version), col_w - 130.0 * dpi),
                );
            }
        }
        let bw = 92.0 * dpi;
        let bh = 30.0 * dpi;
        let bx = x + col_w - bw - 12.0 * dpi;
        let by = y + (row_h - bh) / 2.0;
        if installed {
            text_note(ui, fonts, c, bx, by, bw, bh, "Installed");
        } else if busy {
            text_note(ui, fonts, c, bx, by, bw, bh, "Installing");
        } else if secondary_button(ui, fonts, c, dpi, bx, by, bw, bh, "Install") {
            sink.push(LauncherEvent::InstallMod(m.id.clone()));
        }
        y += row_h + 1.0 * dpi;
    }
    if model.mods.is_empty() && !model.mods_loading {
        let draw = ui.get_window_draw_list();
        let _f = ui.push_font(fonts.small);
        draw.add_text(
            [x, y],
            rgba(c.muted, 1.0),
            "No mods are listed. Refresh, or drop an .hmod in import/mods.",
        );
        y += 24.0 * dpi;
    }
    y += 12.0 * dpi;
    if y + 34.0 * dpi <= height {
        if secondary_button(ui, fonts, c, dpi, x, y, 110.0 * dpi, 34.0 * dpi, "Refresh") {
            sink.push(LauncherEvent::RefreshMods);
        }
    }

    if two {
        let ix = x + col_w + 20.0 * dpi;
        let mut iy = top + 28.0 * dpi + 46.0 * dpi;
        {
            let draw = ui.get_window_draw_list();
            let _f = ui.push_font(fonts.label);
            draw.add_text([ix, iy], rgba(c.muted, 1.0), "INSTALLED");
        }
        iy += 24.0 * dpi;
        if model.installed_mods.is_empty() {
            let draw = ui.get_window_draw_list();
            let _f = ui.push_font(fonts.small);
            draw.add_text([ix, iy], rgba(c.muted, 1.0), "Nothing installed yet.");
        }
        for name in &model.installed_mods {
            if iy + 40.0 * dpi > height {
                break;
            }
            {
                let draw = ui.get_window_draw_list();
                fill(&draw, [ix, iy], [ix + col_w, iy + 38.0 * dpi], c.panel, 1.0, 0.0);
                let _f = ui.push_font(fonts.small);
                draw.add_text(
                    [ix + 14.0 * dpi, iy + 11.0 * dpi],
                    rgba(c.ink_soft, 1.0),
                    fit(ui, name, col_w - 110.0 * dpi),
                );
            }
            if block_button(
                ui,
                fonts,
                c,
                dpi,
                ix + col_w - 84.0 * dpi,
                iy + 4.0 * dpi,
                74.0 * dpi,
                30.0 * dpi,
                "Remove",
                Kind::Ghost,
            ) {
                sink.push(LauncherEvent::UninstallMod(name.clone()));
            }
            iy += 40.0 * dpi;
        }
    }
}

/// Files: the imports and the exports, as the desktop's index rows.
#[allow(clippy::too_many_arguments)]
fn files_page(
    ui: &Ui,
    model: &LauncherModel,
    fonts: &Fonts,
    c: &Css,
    dpi: f32,
    g: &Grid,
    height: f32,
    top: f32,
    sink: &mut Sink<'_>,
) {
    let mut y = page_header(ui, model, fonts, c, dpi, g, top);
    let x = g.x + g.pad;
    let w = g.w - g.pad * 2.0;
    {
        let draw = ui.get_window_draw_list();
        let _f = ui.push_font(fonts.small);
        let path = if model.instance.is_empty() {
            "import/".to_string()
        } else {
            format!("{}/import/", model.instance)
        };
        draw.add_text([x, y], rgba(c.muted, 1.0), fit(ui, &path, w));
    }
    y += 26.0 * dpi;

    let items: [(&str, &str, LauncherEvent, bool); 7] = [
        ("Import skin", "A 64x64 PNG", LauncherEvent::ImportFiles("skin".into()), true),
        ("Import mod", "An .hmod file", LauncherEvent::ImportFiles("mod".into()), true),
        ("Import pack", "A .zip resource pack", LauncherEvent::ImportFiles("resourcepack".into()), true),
        ("Import settings", "A saved set", LauncherEvent::ImportFiles("settings".into()), true),
        ("Export skins", "To the Downloads folder", LauncherEvent::ExportFiles("skins".into()), false),
        ("Export mods", "To the Downloads folder", LauncherEvent::ExportFiles("mods".into()), false),
        ("Export settings", "To the Downloads folder", LauncherEvent::ExportSettings, false),
    ];
    let row_h = 52.0 * dpi;
    let item_count = items.len();
    for (i, (label, sub, event, _is_import)) in items.into_iter().enumerate() {
        if y + row_h > height {
            break;
        }
        {
            let draw = ui.get_window_draw_list();
            {
                let _f = ui.push_font(fonts.label);
                draw.add_text(
                    [x + 4.0 * dpi, y + row_h / 2.0 - 9.0 * dpi],
                    rgba(c.muted, 1.0),
                    format!("{}", i + 1),
                );
            }
            {
                let _f = ui.push_font(fonts.label);
                draw.add_text([x + 26.0 * dpi, y + 10.0 * dpi], rgba(c.ink, 1.0), label);
            }
            {
                let _f = ui.push_font(fonts.small);
                draw.add_text(
                    [x + 26.0 * dpi, y + 29.0 * dpi],
                    rgba(c.muted, 1.0),
                    fit(ui, sub, w * 0.5),
                );
            }
            if i + 1 < item_count {
                fill(
                    &draw,
                    [x, y + row_h - 1.0 * dpi],
                    [x + w, y + row_h],
                    c.rule,
                    1.0,
                    0.0,
                );
            }
        }
        let bw = 96.0 * dpi;
        let bh = 30.0 * dpi;
        let bx = x + w - bw;
        let by = y + (row_h - bh) / 2.0;
        if secondary_button(ui, fonts, c, dpi, bx, by, bw, bh, "Choose...") {
            sink.push(event);
        }
        y += row_h;
    }
    y += 14.0 * dpi;
    if y + 34.0 * dpi <= height {
        if secondary_button(ui, fonts, c, dpi, x, y, 160.0 * dpi, 34.0 * dpi, "Open the folder") {
            sink.push(LauncherEvent::RevealFiles);
        }
    }
}

/// Servers: hosting this world, and the worlds that can be hosted.
#[allow(clippy::too_many_arguments)]
fn multiplayer_page(
    ui: &Ui,
    model: &LauncherModel,
    fonts: &Fonts,
    c: &Css,
    dpi: f32,
    g: &Grid,
    height: f32,
    top: f32,
    sink: &mut Sink<'_>,
) {
    let mut y = page_header(ui, model, fonts, c, dpi, g, top);
    let x = g.x + g.pad;
    let w = g.w - g.pad * 2.0;
    let card_h = 118.0 * dpi;
    {
        let draw = ui.get_window_draw_list();
        panel(&draw, c, [x, y], [x + w, y + card_h], c.panel, dpi);
        {
            let _f = ui.push_font(fonts.label);
            draw.add_text([x + 16.0 * dpi, y + 14.0 * dpi], rgba(c.ink, 1.0), "This device");
        }
        {
            let _f = ui.push_font(fonts.display);
            let text = model.hosting.as_deref().unwrap_or("Not hosting");
            draw.add_text(
                [x + 16.0 * dpi, y + 38.0 * dpi],
                rgba(if model.hosting.is_some() { c.accent } else { c.muted }, 1.0),
                fit(ui, text, w - 32.0 * dpi),
            );
        }
        {
            let _f = ui.push_font(fonts.small);
            draw.add_text(
                [x + 16.0 * dpi, y + 90.0 * dpi],
                rgba(c.muted, 1.0),
                fit(
                    ui,
                    if model.hosting.is_some() {
                        "Friends on this network can join at that address."
                    } else {
                        "Open a world first, then host it. It stays on this device."
                    },
                    w - 32.0 * dpi,
                ),
            );
        }
    }
    y += card_h + 16.0 * dpi;
    if model.hosting.is_some() {
        if secondary_button(ui, fonts, c, dpi, x, y, 150.0 * dpi, 34.0 * dpi, "Stop hosting") {
            sink.push(LauncherEvent::StopHosting);
        }
    } else if primary_button(
        ui, fonts, c, dpi, x, y, 150.0 * dpi, 34.0 * dpi, "Host", true, sink,
    ) {
        sink.push(LauncherEvent::HostWorld(0));
    }
    y += 50.0 * dpi;
    {
        let draw = ui.get_window_draw_list();
        let _f = ui.push_font(fonts.label);
        draw.add_text([x, y], rgba(c.muted, 1.0), "WORLDS ON THIS DEVICE");
    }
    y += 24.0 * dpi;
    let row_h = 54.0 * dpi;
    let mut shown = 0;
    for row in &model.rows {
        if y + row_h > height {
            break;
        }
        if list_row(
            ui, fonts, c, dpi, x, y, w, row_h, &row.title, &row.subtitle, &row.badge, false,
        ) {
            sink.push(LauncherEvent::HostWorld(row.id));
        }
        fill(
            &ui.get_window_draw_list(),
            [x, y + row_h],
            [x + w, y + row_h + 1.0 * dpi],
            c.rule,
            1.0,
            0.0,
        );
        y += row_h + 1.0 * dpi;
        shown += 1;
    }
    if shown == 0 {
        let draw = ui.get_window_draw_list();
        let _f = ui.push_font(fonts.small);
        draw.add_text([x, y], rgba(c.muted, 1.0), "No worlds yet. Play once to make one.");
    }
}

/// Accounts: who is playing, how to sign in, and the touch controls.
#[allow(clippy::too_many_arguments)]
fn profile_page(
    ui: &Ui,
    model: &LauncherModel,
    fonts: &Fonts,
    c: &Css,
    dpi: f32,
    g: &Grid,
    height: f32,
    top: f32,
    sink: &mut Sink<'_>,
) {
    let mut y = page_header(ui, model, fonts, c, dpi, g, top);
    let x = g.x + g.pad;
    let w = g.w - g.pad * 2.0;
    {
        let draw = ui.get_window_draw_list();
        let _f = ui.push_font(fonts.label);
        draw.add_text([x, y], rgba(c.muted, 1.0), "ACCOUNTS");
    }
    y += 24.0 * dpi;
    let row_h = 54.0 * dpi;
    for (i, account) in model.accounts.iter().enumerate() {
        if y + row_h > height {
            break;
        }
        let badge = if account.selected { "Playing" } else { "" };
        if list_row(
            ui,
            fonts,
            c,
            dpi,
            x,
            y,
            w - 96.0 * dpi,
            row_h,
            &account.name,
            &account.kind,
            badge,
            account.selected,
        ) {
            sink.push(LauncherEvent::SwitchAccount(i as u32));
        }
        if block_button(
            ui,
            fonts,
            c,
            dpi,
            x + w - 88.0 * dpi,
            y + (row_h - 30.0 * dpi) / 2.0,
            88.0 * dpi,
            30.0 * dpi,
            "Remove",
            Kind::Ghost,
        ) {
            sink.push(LauncherEvent::RemoveAccount(i as u32));
        }
        fill(
            &ui.get_window_draw_list(),
            [x, y + row_h],
            [x + w, y + row_h + 1.0 * dpi],
            c.rule,
            1.0,
            0.0,
        );
        y += row_h + 1.0 * dpi;
    }
    y += 12.0 * dpi;
    if y + 34.0 * dpi <= height {
        let half = (w - 12.0 * dpi) / 2.0;
        if model.sign_in_waiting {
            if secondary_button(
                ui, fonts, c, dpi, x, y, half, 34.0 * dpi, "Cancel sign-in",
            ) {
                sink.push(LauncherEvent::CancelSignIn);
            }
        } else if primary_button(
            ui,
            fonts,
            c,
            dpi,
            x,
            y,
            half,
            34.0 * dpi,
            "Sign in with Microsoft",
            true,
            sink,
        ) {
            sink.push(LauncherEvent::SignInMicrosoft);
        }
        if secondary_button(
            ui,
            fonts,
            c,
            dpi,
            x + half + 12.0 * dpi,
            y,
            half,
            34.0 * dpi,
            "Add offline account",
        ) {
            sink.push(LauncherEvent::AddAccount);
        }
        y += 46.0 * dpi;
    }
    if !model.status.is_empty() {
        let draw = ui.get_window_draw_list();
        let _f = ui.push_font(fonts.small);
        for (i, line) in wrap(ui, &model.status, w).iter().take(2).enumerate() {
            draw.add_text(
                [x, y + i as f32 * 18.0 * dpi],
                rgba(c.muted, 1.0),
                line.as_str(),
            );
        }
        y += 40.0 * dpi;
    }

    if y + 24.0 * dpi <= height {
        {
            let draw = ui.get_window_draw_list();
            let _f = ui.push_font(fonts.label);
            draw.add_text([x, y], rgba(c.muted, 1.0), "TOUCH CONTROLS");
        }
        y += 26.0 * dpi;
        // One pad, and one way into it. The pad is the layout file, so there is no
        // preset to choose between: what a player can change is where its buttons
        // are, and that is the arrange mode the in-game panel opens.
        if secondary_button(
            ui,
            fonts,
            c,
            dpi,
            x,
            y,
            200.0 * dpi,
            32.0 * dpi,
            "Arrange the buttons",
        ) {
            sink.push(LauncherEvent::OpenControls);
        }
        y += 44.0 * dpi;
    }

    if y + 20.0 * dpi <= height {
        {
            let draw = ui.get_window_draw_list();
            let _f = ui.push_font(fonts.label);
            draw.add_text([x, y], rgba(c.muted, 1.0), "ABOUT");
        }
        y += 24.0 * dpi;
        for (key, value) in &model.facts {
            if y + 20.0 * dpi > height {
                break;
            }
            let draw = ui.get_window_draw_list();
            {
                let _f = ui.push_font(fonts.small);
                draw.add_text([x, y], rgba(c.muted, 1.0), key.as_str());
            }
            {
                let _f = ui.push_font(fonts.mono);
                let [vw, _] = ui.calc_text_size(value.as_str());
                draw.add_text(
                    [x + w - vw, y],
                    rgba(c.ink_soft, 1.0),
                    fit(ui, value, w * 0.6),
                );
            }
            y += 20.0 * dpi;
        }
    }
}

/// Settings: the launcher's own switches first, then the game's options.
#[allow(clippy::too_many_arguments)]
fn settings_page(
    ui: &Ui,
    model: &LauncherModel,
    fonts: &Fonts,
    c: &Css,
    dpi: f32,
    g: &Grid,
    height: f32,
    top: f32,
    sink: &mut Sink<'_>,
) {
    let mut head = page_header(ui, model, fonts, c, dpi, g, top);
    // A way back: the desktop's Settings is a page like any other, and the bar stays.
    if secondary_button(ui, fonts, c, dpi, g.right(90.0 * dpi), top + 28.0 * dpi, 90.0 * dpi, 30.0 * dpi, "Back") {
        sink.push(LauncherEvent::Goto(LauncherPage::Play));
    }
    if !model.status.is_empty() {
        let draw = ui.get_window_draw_list();
        let _f = ui.push_font(fonts.small);
        draw.add_text(
            [g.x + g.pad, head - 6.0 * dpi],
            rgba(c.muted, 1.0),
            fit(ui, &model.status, g.w * 0.6),
        );
    }
    head += 8.0 * dpi;
    let x = g.x + g.pad;
    let w = g.w - g.pad * 2.0;
    let row_h = 54.0 * dpi;
    let gap = 2.0 * dpi;
    let group_h = 30.0 * dpi;

    let mut content = 0.0;
    let mut last_group = "";
    for setting in &model.settings {
        if !setting.group.is_empty() && setting.group.as_str() != last_group {
            content += group_h;
            last_group = setting.group.as_str();
        }
        content += row_h + gap;
    }
    let bottom = (height - 12.0 * dpi).max(head);
    let scroll = scroll_begin(ui, g, head, bottom, content, dpi);
    let swallowed = scroll.swallowed;

    {
        let draw = ui.get_window_draw_list();
        draw.with_clip_rect_intersect([g.x, head], [g.x + g.w, bottom], || {
            let mut y = head - scroll.offset;
            let mut last_group = "";
            for setting in &model.settings {
                if !setting.group.is_empty() && setting.group.as_str() != last_group {
                    if y + group_h > head && y < bottom {
                        let _f = ui.push_font(fonts.label);
                        draw.add_text(
                            [x, y + 6.0 * dpi],
                            rgba(c.muted, 1.0),
                            setting.group.as_str(),
                        );
                        fill(
                            &draw,
                            [x, y + group_h - 8.0 * dpi],
                            [x + w, y + group_h - 7.0 * dpi],
                            c.rule,
                            1.0,
                            0.0,
                        );
                    }
                    y += group_h;
                    last_group = setting.group.as_str();
                }
                if y + row_h <= head || y >= bottom {
                    y += row_h + gap;
                    continue;
                }
                let hit_y = y.max(head);
                let hit_h = ((y + row_h).min(bottom) - hit_y).max(0.0);
                // A row is a plate with a rule under it, like every other list here.
                fill(&draw, [x, y], [x + w, y + row_h], c.panel, 1.0, 0.0);
                fill(
                    &draw,
                    [x, y + row_h],
                    [x + w, y + row_h + 1.0 * dpi],
                    c.rule,
                    1.0,
                    0.0,
                );
                {
                    let _f = ui.push_font(fonts.body);
                    draw.add_text(
                        [x + 14.0 * dpi, y + 10.0 * dpi],
                        rgba(c.ink_soft, 1.0),
                        fit(ui, &setting.label, w * 0.60),
                    );
                }
                if setting.toggle {
                    // A square knob in a sunk track, which is the desktop's switch.
                    let tw = 44.0 * dpi;
                    let th = 24.0 * dpi;
                    let tx = x + w - 14.0 * dpi - tw;
                    let ty = y + (row_h - th) / 2.0;
                    well(&draw, c, [tx, ty], [tx + tw, ty + th], dpi);
                    let knob = th - 8.0 * dpi;
                    let kx = if setting.on {
                        tx + tw - knob - 4.0 * dpi
                    } else {
                        tx + 4.0 * dpi
                    };
                    if setting.on {
                        fill(&draw, [tx, ty], [tx + tw, ty + th], c.face, 1.0, 0.0);
                    }
                    fill(
                        &draw,
                        [kx, ty + 4.0 * dpi],
                        [kx + knob, ty + 4.0 * dpi + knob],
                        if setting.on { c.on_face } else { c.muted },
                        1.0,
                        0.0,
                    );
                    if hit_h > 0.0 {
                        ui.set_cursor_screen_pos([x, hit_y]);
                        let id = format!("##set{}", setting.key);
                        let hit = ui.invisible_button(id.as_str(), [w, hit_h]);
                        if hit && !swallowed {
                            sink.push(LauncherEvent::ToggleOption(
                                setting.key.clone(),
                                !setting.on,
                            ));
                        }
                    }
                } else {
                    let low = setting_text(setting, setting.min);
                    let high = setting_text(setting, setting.max);
                    let (low_w, high_w) = {
                        let _f = ui.push_font(fonts.small);
                        (
                            ui.calc_text_size(low.as_str())[0],
                            ui.calc_text_size(high.as_str())[0],
                        )
                    };
                    let track_x = x + 14.0 * dpi + low_w + 10.0 * dpi;
                    let track_w =
                        (x + w - 14.0 * dpi - high_w - 10.0 * dpi - track_x).max(60.0 * dpi);
                    let ty = y + 34.0 * dpi;
                    let span = (setting.max - setting.min).max(f32::EPSILON);
                    let t = ((setting.value - setting.min) / span).clamp(0.0, 1.0);
                    well(&draw, c, [track_x, ty], [track_x + track_w, ty + 8.0 * dpi], dpi);
                    if t > 0.0 {
                        fill(
                            &draw,
                            [track_x + 2.0 * dpi, ty + 2.0 * dpi],
                            [track_x + 2.0 * dpi + (track_w - 4.0 * dpi) * t, ty + 6.0 * dpi],
                            c.face,
                            1.0,
                            0.0,
                        );
                    }
                    // The handle is a block, because everything else on this screen is.
                    fill(
                        &draw,
                        [track_x + (track_w - 8.0 * dpi) * t, ty - 2.0 * dpi],
                        [track_x + (track_w - 8.0 * dpi) * t + 8.0 * dpi, ty + 10.0 * dpi],
                        c.ink,
                        1.0,
                        0.0,
                    );
                    {
                        let _f = ui.push_font(fonts.small);
                        let text = setting_text(setting, setting.value);
                        let [tw, _] = ui.calc_text_size(text.as_str());
                        draw.add_text(
                            [x + w - 14.0 * dpi - tw, y + 10.0 * dpi],
                            rgba(c.ink, 1.0),
                            text.as_str(),
                        );
                        draw.add_text(
                            [x + 14.0 * dpi, ty - 6.0 * dpi],
                            rgba(c.muted, 0.9),
                            low.as_str(),
                        );
                        draw.add_text(
                            [x + w - 14.0 * dpi - high_w, ty - 6.0 * dpi],
                            rgba(c.muted, 0.9),
                            high.as_str(),
                        );
                    }
                    if hit_h > 0.0 {
                        ui.set_cursor_screen_pos([track_x, hit_y]);
                        let id = format!("##set{}", setting.key);
                        let pressed = ui.invisible_button(id.as_str(), [track_w, hit_h]);
                        SETTINGS_ON_TRACK.set(pressed || ui.is_item_active());
                        let across = SETTINGS_PRESS
                            .get()
                            .is_none_or(|origin| (ui.io().mouse_pos[0] - origin[0]).abs() > 4.0 * dpi);
                        if (pressed || (ui.is_item_active() && across))
                            && !swallowed
                            && SETTINGS_DRAG.get().is_none()
                        {
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
                    [x, head + 8.0 * dpi],
                    rgba(c.muted, 1.0),
                    "The game's options appear once the pack is installed.",
                );
            }
        });
    }
    scroll_end(ui, g, c, dpi, &scroll, content);
}
/// A page's vertical scroll, for a finger that drags it.
struct Scroll {
    /// How far the content is scrolled, clamped to what there is to see.
    offset: f32,
    /// The greatest offset the content allows.
    max: f32,
    /// The column's own top and bottom, in screen space.
    top: f32,
    bottom: f32,
    /// The gesture became a scroll, so the row it began on must not also fire.
    swallowed: bool,
}

/// Read the gesture and clamp the offset. Call once, before drawing the content.
fn scroll_begin(ui: &Ui, g: &Grid, top: f32, bottom: f32, content: f32, dpi: f32) -> Scroll {
    let view = (bottom - top).max(1.0);
    let max = (content - view).max(0.0);
    let mouse = ui.io().mouse_pos;
    let down = ui.io().mouse_down[0];
    let over = mouse[0] >= g.x && mouse[0] <= g.x + g.w && mouse[1] >= top && mouse[1] <= bottom;
    let mut offset = SETTINGS_SCROLL.get();
    let wheel = ui.io().mouse_wheel;
    if over && wheel != 0.0 {
        offset -= wheel * 48.0 * dpi;
    }
    let held = SETTINGS_HELD.get();
    SETTINGS_HELD.set(down);
    let mut swallowed = SETTINGS_SWALLOW.get();
    if !down {
        swallowed |= SETTINGS_DRAG.get().is_some();
        SETTINGS_PRESS.set(None);
        SETTINGS_DRAG.set(None);
        SETTINGS_SWALLOW.set(false);
    } else if let Some(origin) = SETTINGS_PRESS.get() {
        let dy = mouse[1] - origin[1];
        if let Some(last) = SETTINGS_DRAG.get() {
            offset -= mouse[1] - last;
            SETTINGS_DRAG.set(Some(mouse[1]));
        } else if dy.abs() > 12.0 * dpi && dy.abs() > (mouse[0] - origin[0]).abs() {
            // A mostly-vertical drag is the page's, even when it began on a row.
            offset -= dy - 12.0 * dpi * dy.signum();
            SETTINGS_DRAG.set(Some(mouse[1]));
            SETTINGS_SWALLOW.set(true);
            swallowed = true;
        }
    } else if over && !held && !SETTINGS_ON_TRACK.get() {
        SETTINGS_PRESS.set(Some(mouse));
    }
    Scroll {
        offset: offset.clamp(0.0, max),
        max,
        top,
        bottom,
        swallowed,
    }
}

/// Write the clamped offset back, and draw the thumb that says the page scrolls.
fn scroll_end(ui: &Ui, g: &Grid, c: &Css, dpi: f32, scroll: &Scroll, content: f32) {
    SETTINGS_SCROLL.set(scroll.offset);
    if scroll.max <= 0.0 || scroll.bottom - scroll.top <= 1.0 {
        return;
    }
    let draw = ui.get_window_draw_list();
    let x = g.x + g.w - 8.0 * dpi;
    let view = scroll.bottom - scroll.top;
    well(
        &draw,
        c,
        [x, scroll.top],
        [x + 4.0 * dpi, scroll.bottom],
        dpi,
    );
    let thumb = (view / content * view).max(28.0 * dpi).min(view);
    let at = scroll.top + (scroll.offset / scroll.max) * (view - thumb).max(0.0);
    fill(
        &draw,
        [x + 1.0 * dpi, at],
        [x + 3.0 * dpi, at + thumb],
        c.muted,
        1.0,
        0.0,
    );
}

/// How far one step of a setting moves it.
fn setting_step(setting: &LauncherSetting) -> f32 {
    if setting.step > 0.0 {
        return setting.step;
    }
    let span = setting.max - setting.min;
    if setting.percent || span <= 1.5 {
        0.01
    } else {
        1.0
    }
}

/// How many decimals a step needs.
fn setting_decimals(step: f32) -> usize {
    if step >= 1.0 {
        0
    } else if step >= 0.1 {
        1
    } else {
        2
    }
}

/// The number, with its unit: "12 chunks", "70°", "50%".
fn setting_text(setting: &LauncherSetting, value: f32) -> String {
    let step = setting_step(setting);
    let decimals = setting_decimals(step);
    let shown = if setting.percent { value * 100.0 } else { value };
    let mut number = format!("{shown:.decimals$}");
    if number.contains('.') {
        while number.ends_with('0') {
            number.pop();
        }
        if number.ends_with('.') {
            number.pop();
        }
    }
    // The frame limit's top end is "no cap", not a number: 260 is what the file stores
    // for unlimited, and a player reading "260 fps" would think it was a ceiling.
    if setting.unit == "fps" && value >= 260.0 {
        return "Unlimited".to_string();
    }
    match setting.unit.as_str() {
        "" => number,
        "%" => format!("{number}%"),
        "°" => format!("{number}°"),
        "x" => format!("{number}x"),
        word => format!("{number} {word}"),
    }
}

/// Put a value on the option's real grid.
fn snap_setting(setting: &LauncherSetting, raw: f32) -> f32 {
    let step = setting_step(setting);
    let snapped = (raw / step).round() * step;
    snapped.clamp(setting.min, setting.max)
}

/// A pixel icon from the theme's set, drawn as blocks.
///
/// The desktop draws these as SVG. imgui has no vector path, and loading an SVG
/// parser for thirty-two icons is not worth it: every icon in the set is a 24x24
/// grid of one-unit blocks, so each one is written here as the handful of rects it
/// is made of. That keeps them crisp at any size, the way the pixel art is meant to
/// be, and costs no texture.
fn icon(draw: &DrawListMut<'_>, name: &str, x: f32, y: f32, s: f32, col: u32) {
    let u = s / 24.0;
    let block = |gx: f32, gy: f32, gw: f32, gh: f32| {
        fill(
            draw,
            [x + gx * u, y + gy * u],
            [x + (gx + gw) * u, y + (gy + gh) * u],
            col,
            1.0,
            0.0,
        );
    };
    // A run of one-unit cells, written as a row string for each line of the glyph:
    // '#' is ink. This is the same information the SVG's path carries, in the shape
    // a person can read.
    let glyph: &[&str] = match name {
        // home: a roof over a door.
        "home" => &[
            "....####....",
            "..########..",
            ".##########.",
            "####....####",
            "..##....##..",
            "..##.##.##..",
            "..##.##.##..",
            "..##.##.##..",
            "..##....##..",
            "............",
        ],
        // play: a triangle.
        "play" => &[
            "............",
            "..##........",
            "..####......",
            "..######....",
            "..########..",
            "..########..",
            "..######....",
            "..####......",
            "..##........",
            "............",
        ],
        // layers: three stacked plates.
        "layers" => &[
            "............",
            "...######...",
            "..########..",
            "..########..",
            "............",
            "..########..",
            "..########..",
            "............",
            "..########..",
            "..########..",
        ],
        // user: a head over shoulders.
        "user" => &[
            "....####....",
            "...######...",
            "...######...",
            "....####....",
            "............",
            "..########..",
            ".##########.",
            ".##########.",
            "############",
            "############",
        ],
        // book: an open book.
        "book" => &[
            "............",
            ".####..####.",
            ".##......##.",
            ".##..##..##.",
            ".##..##..##.",
            ".##..##..##.",
            ".##......##.",
            ".####..####.",
            "............",
            "............",
        ],
        // sparkles: a four-pointed star and a small one.
        "sparkles" => &[
            ".....##.....",
            "....####....",
            "....####....",
            "..########..",
            "..########..",
            "....####....",
            "....####....",
            ".....##.....",
            "..........##",
            "..........##",
        ],
        // puzzle: a mod's piece.
        "puzzle" => &[
            "............",
            "..########..",
            "..##....##..",
            "..##....##..",
            "######..####",
            "######..####",
            "..##....##..",
            "..##....##..",
            "..########..",
            "............",
        ],
        // sliders: three tracks with a knob each.
        "sliders" => &[
            "............",
            "..##########",
            ".....####...",
            "..##########",
            "............",
            "..##########",
            ".....####...",
            "..##########",
            "............",
            "............",
        ],
        // folder.
        "folder" => &[
            "............",
            "..####......",
            "..####......",
            ".##########.",
            ".##########.",
            ".##########.",
            ".##########.",
            ".##########.",
            "............",
            "............",
        ],
        // server: two racks.
        "server" => &[
            "............",
            ".##########.",
            ".#........#.",
            ".#........#.",
            ".##########.",
            "............",
            ".##########.",
            ".#........#.",
            ".#........#.",
            ".##########.",
        ],
        // check.
        "check" => &[
            "............",
            "..........##",
            ".........##.",
            "........##..",
            "##.....##...",
            ".##...##....",
            "..##.##.....",
            "...###......",
            "....#.......",
            "............",
        ],
        // close.
        "close" => &[
            "............",
            ".##......##.",
            "..##....##..",
            "...##..##...",
            "....####....",
            "....####....",
            "...##..##...",
            "..##....##..",
            ".##......##.",
            "............",
        ],
        // download: an arrow into a tray.
        "download" => &[
            ".....##.....",
            ".....##.....",
            ".....##.....",
            "..########..",
            "...######...",
            "....####....",
            ".....##.....",
            "............",
            "##........##",
            "############",
        ],
        // reload: a turning arrow.
        "reload" => &[
            "....####....",
            "..##....##..",
            ".##......##.",
            "##........##",
            "##..........",
            "##..........",
            ".##......##.",
            "..##....##..",
            "....####....",
            "............",
        ],
        // plus.
        "plus" => &[
            "............",
            ".....##.....",
            ".....##.....",
            ".....##.....",
            "..########..",
            "..########..",
            ".....##.....",
            ".....##.....",
            ".....##.....",
            "............",
        ],
        // trash.
        "trash" => &[
            "...######...",
            "............",
            ".##########.",
            ".#.#.##.#.#.",
            ".#.#.##.#.#.",
            ".#.#.##.#.#.",
            ".#.#.##.#.#.",
            ".#.#.##.#.#.",
            ".##########.",
            "............",
        ],
        // box: a package.
        "box" => &[
            "............",
            "..########..",
            ".##......##.",
            "##..####..##",
            "##..####..##",
            "##..####..##",
            ".##......##.",
            "..########..",
            "............",
            "............",
        ],
        // image.
        "image" => &[
            "............",
            ".##########.",
            ".#...##...#.",
            ".#..####..#.",
            ".#.######.#.",
            ".#..####..#.",
            ".#...##...#.",
            ".##########.",
            "............",
            "............",
        ],
        // globe.
        "globe" => &[
            "....####....",
            "..##.##.##..",
            ".##..##..##.",
            "##...##...##",
            "##########..",
            "##...##...##",
            ".##..##..##.",
            "..##.##.##..",
            "....####....",
            "............",
        ],
        // search.
        "search" => &[
            "...####.....",
            "..##..##....",
            ".##....##...",
            ".##....##...",
            "..##..##....",
            "...####.....",
            "......###...",
            ".......###..",
            "........###.",
            "............",
        ],
        // clock.
        "clock" => &[
            "....####....",
            "..##....##..",
            ".##..##..##.",
            "##...##...##",
            "##...##...##",
            "##...###..##",
            ".##......##.",
            "..##....##..",
            "....####....",
            "............",
        ],
        // monitor.
        "monitor" => &[
            "............",
            "############",
            "#..........#",
            "#..........#",
            "#..........#",
            "#..........#",
            "############",
            ".....##.....",
            "...######...",
            "............",
        ],
        // gamepad.
        "gamepad" => &[
            "............",
            "..########..",
            ".##..##..##.",
            "##..####..##",
            "##..####..##",
            ".##..##..##.",
            "..########..",
            "............",
            "............",
            "............",
        ],
        // zap.
        "zap" => &[
            "......##....",
            ".....##.....",
            "....##......",
            "...######...",
            "......##....",
            ".....##.....",
            "....##......",
            "...##.......",
            "..##........",
            "............",
        ],
        // message.
        "message" => &[
            "............",
            ".##########.",
            ".#........#.",
            ".#........#.",
            ".#........#.",
            ".##########.",
            "..##........",
            ".##.........",
            "............",
            "............",
        ],
        // archive.
        "archive" => &[
            "############",
            "............",
            ".##########.",
            ".#..####..#.",
            ".#..####..#.",
            ".#..####..#.",
            ".#........#.",
            ".##########.",
            "............",
            "............",
        ],
        // notes.
        "notes" => &[
            "............",
            ".####...##..",
            ".#..#...##..",
            ".#..#...##..",
            ".#..#...##..",
            ".#..#...##..",
            ".#..#..####.",
            ".#..#..####.",
            ".####...##..",
            "............",
        ],
        // package.
        "package" => &[
            "............",
            "....####....",
            "..##.##.##..",
            ".##########.",
            ".##..##..##.",
            ".##..##..##.",
            ".##########.",
            "............",
            "............",
            "............",
        ],
        // terminal.
        "terminal" => &[
            "............",
            "############",
            "#..........#",
            "#.##.......#",
            "#..##......#",
            "#.##.......#",
            "#..........#",
            "############",
            "............",
            "............",
        ],
        // info-box.
        "info-box" => &[
            "....####....",
            "..##....##..",
            ".##..##..##.",
            "##...##...##",
            "##...##...##",
            "##...##...##",
            ".##..##..##.",
            "..##....##..",
            "....####....",
            "............",
        ],
        // external-link.
        "external-link" => &[
            "......######",
            "......##....",
            "......##....",
            ".######.....",
            ".##.........",
            ".##.........",
            ".##.........",
            ".##.........",
            ".##.........",
            "............",
        ],
        // arrow-left.
        "arrow-left" => &[
            "............",
            "....##......",
            "...##.......",
            "..##........",
            ".##########.",
            "..##........",
            "...##.......",
            "....##......",
            "............",
            "............",
        ],
        // chevron-right.
        "chevron-right" => &[
            "............",
            "....##......",
            ".....##.....",
            "......##....",
            ".......##...",
            ".......##...",
            "......##....",
            ".....##.....",
            "....##......",
            "............",
        ],
        // chevron-down.
        "chevron-down" => &[
            "............",
            "............",
            "##........##",
            ".##......##.",
            "..##....##..",
            "...##..##...",
            "....####....",
            ".....##.....",
            "............",
            "............",
        ],
        // bug.
        "bug" => &[
            "....####....",
            "..##.##.##..",
            "..########..",
            "##..####..##",
            "##..####..##",
            "##..####..##",
            "..########..",
            "..##.##.##..",
            "....####....",
            "............",
        ],
        // heart.
        "heart" => &[
            "............",
            "..##....##..",
            ".####..####.",
            "############",
            "############",
            ".##########.",
            "..########..",
            "...######...",
            "....####....",
            ".....##.....",
        ],
        _ => return,
    };
    let rows = glyph.len() as f32;
    let cols = glyph.iter().map(|r| r.len()).max().unwrap_or(0) as f32;
    if rows == 0.0 || cols == 0.0 {
        return;
    }
    // Each '#' is one cell, and a horizontal run of them is one rect: an icon is a
    // handful of draws rather than sixty.
    let cell_w = s / cols;
    let cell_h = s / rows;
    for (row, line) in glyph.iter().enumerate() {
        let mut run_start: Option<usize> = None;
        for (column, ch) in line.chars().chain(std::iter::once('.')).enumerate() {
            if ch == '#' {
                if run_start.is_none() {
                    run_start = Some(column);
                }
            } else if let Some(first) = run_start.take() {
                fill(
                    draw,
                    [x + first as f32 * cell_w, y + row as f32 * cell_h],
                    [x + column as f32 * cell_w, y + (row + 1) as f32 * cell_h],
                    col,
                    1.0,
                    0.0,
                );
            }
        }
    }
}

/// Read a drag on the figure and answer the angle to draw it at.
///
/// This cell is the only place the angle lives. It used to be copied into the platform's
/// model every frame and back again, which cost a frame of lag on every move and — on the
/// frame the finger lifted — replaced the angle the drag had just reached with the one
/// from before it, so the figure sprang back as soon as it was let go.
fn drag_skin(
    ui: &Ui,
    min: [f32; 2],
    max: [f32; 2],
    dpi: f32,
) -> (f32, f32) {
    let mouse = ui.io().mouse_pos;
    let down = ui.io().mouse_down[0];
    let over = mouse[0] >= min[0] && mouse[0] <= max[0] && mouse[1] >= min[1] && mouse[1] <= max[1];
    if !down {
        // The lift ends the drag, and clears the flag that stopped a finger which was
        // already down elsewhere from grabbing the model. Leaving that flag set is what
        // let the figure be turned exactly once and never again.
        SKIN_DRAG.with(|d| d.set(None));
        SKIN_HELD.with(|h| h.set(false));
        return SKIN_TURN.with(|t| t.get());
    }
    let mut turning = SKIN_DRAG.with(|d| d.get());
    if turning.is_none() && over && !SKIN_HELD.with(|h| h.get()) {
        turning = Some(mouse);
        SKIN_DRAG.with(|d| d.set(turning));
    }
    SKIN_HELD.with(|h| h.set(true));
    let Some(last) = turning else {
        return SKIN_TURN.with(|t| t.get());
    };
    let (yaw, pitch) = SKIN_TURN.with(|t| t.get());
    let dx = mouse[0] - last[0];
    let dy = mouse[1] - last[1];
    // One finger width turns the figure a half turn, which is the range a hand has.
    let yaw = yaw + dx / (110.0 * dpi);
    // The tilt stops at straight up and straight down: past that the model is upside
    // down, which no skin was drawn for.
    let pitch = (pitch + dy / (220.0 * dpi)).clamp(-1.35, 1.35);
    SKIN_DRAG.with(|d| d.set(Some(mouse)));
    SKIN_TURN.with(|t| t.set((yaw, pitch)));
    (yaw, pitch)
}

/// A player, built the way the game builds one: six boxes with the skin's own UVs,
/// back faces culled, and the overlay layer inflated over the base.
///
/// The parts and their regions are Minecraft's 64x64 layout, so a skin drawn for the
/// game is drawn here exactly as it is in it. The second layer is the hat and the
/// jacket: it is inflated half a pixel and drawn over the part it belongs to, and its
/// own alpha decides what shows.
fn skin_figure(
    ui: &Ui,
    draw: &DrawListMut<'_>,
    texture: TextureId,
    size: [u32; 2],
    cx: f32,
    top: f32,
    height: f32,
    yaw: f32,
    pitch: f32,
) {
    let (w, h) = (size[0] as f32, size[1] as f32);
    if w < 64.0 || h < 32.0 {
        return;
    }
    let legacy = h < 64.0;
    let _ = ui;
    // One model unit is a sixteenth of a block, and the player is 32 units tall.
    let u = height / 32.0;
    let (sy, cy_) = (yaw.sin(), yaw.cos());
    let (sp, cp) = (pitch.sin(), pitch.cos());
    // The figure turns about the middle of the body, which is what a hand holding a
    // figure does. Turning it about the feet (model y = 0) made the whole model swing
    // away from the finger on a tilt, so the head travelled and the shoes stayed put.
    let mid = top + 16.0 * u;
    let project = |p: [f32; 3]| -> ([f32; 2], f32) {
        let x1 = p[0] * cy_ + p[2] * sy;
        let z1 = -p[0] * sy + p[2] * cy_;
        let y0 = p[1] - 16.0;
        let y2 = y0 * cp - z1 * sp;
        let z2 = y0 * sp + z1 * cp;
        let scale = 1.0 / (1.0 - z2 * 0.006);
        ([cx + x1 * u * scale, mid - y2 * u * scale], z2)
    };
    let facing = |n: [f32; 3]| -> f32 {
        let z1 = -n[0] * sy + n[2] * cy_;
        n[1] * sp + z1 * cp
    };

    struct Part {
        origin: [f32; 3],
        size: [f32; 3],
        base: [[f32; 4]; 6],
        overlay: Option<[[f32; 4]; 6]>,
    }
    let parts: [Part; 6] = [
        Part {
            origin: [0.0, 24.0, 0.0],
            size: [8.0, 8.0, 8.0],
            base: [
                [8.0, 8.0, 16.0, 16.0],
                [0.0, 8.0, 8.0, 16.0],
                [24.0, 8.0, 32.0, 16.0],
                [16.0, 8.0, 24.0, 16.0],
                [8.0, 0.0, 16.0, 8.0],
                [16.0, 0.0, 24.0, 8.0],
            ],
            overlay: (!legacy).then_some([
                [40.0, 8.0, 48.0, 16.0],
                [32.0, 8.0, 40.0, 16.0],
                [56.0, 8.0, 64.0, 16.0],
                [48.0, 8.0, 56.0, 16.0],
                [40.0, 0.0, 48.0, 8.0],
                [48.0, 0.0, 56.0, 8.0],
            ]),
        },
        Part {
            origin: [0.0, 12.0, 0.0],
            size: [8.0, 12.0, 4.0],
            base: [
                [20.0, 20.0, 28.0, 32.0],
                [16.0, 20.0, 20.0, 32.0],
                [32.0, 20.0, 40.0, 32.0],
                [28.0, 20.0, 32.0, 32.0],
                [20.0, 16.0, 28.0, 20.0],
                [28.0, 16.0, 36.0, 20.0],
            ],
            overlay: (!legacy).then_some([
                [20.0, 36.0, 28.0, 48.0],
                [16.0, 36.0, 20.0, 48.0],
                [32.0, 36.0, 40.0, 48.0],
                [28.0, 36.0, 32.0, 48.0],
                [20.0, 32.0, 28.0, 36.0],
                [28.0, 32.0, 36.0, 36.0],
            ]),
        },
        Part {
            origin: [-6.0, 12.0, 0.0],
            size: [4.0, 12.0, 4.0],
            base: [
                [44.0, 20.0, 48.0, 32.0],
                [40.0, 20.0, 44.0, 32.0],
                [52.0, 20.0, 56.0, 32.0],
                [48.0, 20.0, 52.0, 32.0],
                [44.0, 16.0, 48.0, 20.0],
                [48.0, 16.0, 52.0, 20.0],
            ],
            overlay: (!legacy).then_some([
                [44.0, 36.0, 48.0, 48.0],
                [40.0, 36.0, 44.0, 48.0],
                [52.0, 36.0, 56.0, 48.0],
                [48.0, 36.0, 52.0, 48.0],
                [44.0, 32.0, 48.0, 36.0],
                [48.0, 32.0, 52.0, 36.0],
            ]),
        },
        Part {
            origin: [6.0, 12.0, 0.0],
            size: [4.0, 12.0, 4.0],
            base: if legacy {
                [
                    [44.0, 20.0, 48.0, 32.0],
                    [40.0, 20.0, 44.0, 32.0],
                    [52.0, 20.0, 56.0, 32.0],
                    [48.0, 20.0, 52.0, 32.0],
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
            overlay: (!legacy).then_some([
                [52.0, 52.0, 56.0, 64.0],
                [48.0, 52.0, 52.0, 64.0],
                [60.0, 52.0, 64.0, 64.0],
                [56.0, 52.0, 60.0, 64.0],
                [52.0, 48.0, 56.0, 52.0],
                [56.0, 48.0, 60.0, 52.0],
            ]),
        },
        Part {
            origin: [-2.0, 0.0, 0.0],
            size: [4.0, 12.0, 4.0],
            base: [
                [4.0, 20.0, 8.0, 32.0],
                [0.0, 20.0, 4.0, 32.0],
                [12.0, 20.0, 16.0, 32.0],
                [8.0, 20.0, 12.0, 32.0],
                [4.0, 16.0, 8.0, 20.0],
                [8.0, 16.0, 12.0, 20.0],
            ],
            overlay: (!legacy).then_some([
                [4.0, 36.0, 8.0, 48.0],
                [0.0, 36.0, 4.0, 48.0],
                [12.0, 36.0, 16.0, 48.0],
                [8.0, 36.0, 12.0, 48.0],
                [4.0, 32.0, 8.0, 36.0],
                [8.0, 32.0, 12.0, 36.0],
            ]),
        },
        Part {
            origin: [2.0, 0.0, 0.0],
            size: [4.0, 12.0, 4.0],
            base: if legacy {
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
            overlay: (!legacy).then_some([
                [4.0, 52.0, 8.0, 64.0],
                [0.0, 52.0, 4.0, 64.0],
                [12.0, 52.0, 16.0, 64.0],
                [8.0, 52.0, 12.0, 64.0],
                [4.0, 48.0, 8.0, 52.0],
                [8.0, 48.0, 12.0, 52.0],
            ]),
        },
    ];
    struct Face {
        depth: f32,
        layer: u8,
        points: [[f32; 2]; 4],
        uv: [f32; 4],
        shade: f32,
    }
    let mut faces: Vec<Face> = Vec::new();
    for part in parts {
        let [ox, oy, oz] = part.origin;
        let [sx, syz, sz] = part.size;
        for layer in 0..2u8 {
            let rects = match (layer, &part.overlay) {
                (0, _) => &part.base,
                (1, Some(overlay)) => overlay,
                _ => continue,
            };
            let inflate = if layer == 1 { 0.5 } else { 0.0 };
            let x0 = ox - sx / 2.0 - inflate;
            let x1 = ox + sx / 2.0 + inflate;
            let y0 = oy - inflate;
            let y1 = oy + syz + inflate;
            let z0 = oz - sz / 2.0 - inflate;
            let z1 = oz + sz / 2.0 + inflate;
            let quads: [[[f32; 3]; 4]; 6] = [
                [[x0, y1, z1], [x1, y1, z1], [x1, y0, z1], [x0, y0, z1]],
                [[x0, y1, z0], [x0, y1, z1], [x0, y0, z1], [x0, y0, z0]],
                [[x1, y1, z0], [x0, y1, z0], [x0, y0, z0], [x1, y0, z0]],
                [[x1, y1, z1], [x1, y1, z0], [x1, y0, z0], [x1, y0, z1]],
                [[x0, y1, z0], [x1, y1, z0], [x1, y1, z1], [x0, y1, z1]],
                [[x0, y0, z1], [x1, y0, z1], [x1, y0, z0], [x0, y0, z0]],
            ];
            let normals = [
                [0.0, 0.0, 1.0],
                [-1.0, 0.0, 0.0],
                [0.0, 0.0, -1.0],
                [1.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
                [0.0, -1.0, 0.0],
            ];
            for (face_index, (quad, normal)) in quads.iter().zip(normals).enumerate() {
                if facing(normal) <= 0.0 {
                    continue;
                }
                let mut points = [[0.0f32; 2]; 4];
                let mut depth = 0.0f32;
                for (k, corner) in quad.iter().enumerate() {
                    let (p, z) = project(*corner);
                    points[k] = p;
                    depth += z;
                }
                depth /= 4.0;
                // Light from above and slightly to the front: the top is brightest.
                let shade = if normal[1] > 0.5 {
                    1.0
                } else if normal[1] < -0.5 {
                    0.62
                } else if normal[0].abs() > 0.5 {
                    0.84
                } else {
                    0.96
                };
                faces.push(Face {
                    depth,
                    layer,
                    points,
                    uv: rects[face_index],
                    shade,
                });
            }
        }
    }
    faces.sort_by(|a, b| {
        a.depth
            .total_cmp(&b.depth)
            .then_with(|| a.layer.cmp(&b.layer))
    });
    for face in faces {
        let [u0, v0, u1, v1] = face.uv;
        draw.add_image_quad(
            texture,
            face.points[0],
            face.points[1],
            face.points[2],
            face.points[3],
        )
        .uv(
            [u0 / w, v0 / h],
            [u1 / w, v0 / h],
            [u1 / w, v1 / h],
            [u0 / w, v1 / h],
        )
        // A face turned away from the light is darker, not see-through: the shade
        // used to be written into the alpha, which made three of the six faces of
        // every part translucent and let the card show through the player.
        .col([face.shade, face.shade, face.shade, 1.0])
        .build();
    }
}

/// The hotbar strip, drawn while the pad is being arranged.
///
/// It is invisible while playing: the game draws its own bar there, and a second row
/// of nine squares over it was the bar drawn twice. But a player arranging their pad
/// has to know where the strip is, because it is the one part of the layout that does
/// not stay where they put it — it follows the game's own bar, and therefore the GUI
/// scale.
pub fn draw_hotbar_hint(ui: &Ui, strip: Option<crate::Rect>, dpi: f32) {
    let Some(strip) = strip else {
        return;
    };
    let draw = ui.get_window_draw_list();
    draw.add_rect(
        [strip.x, strip.y],
        [strip.x + strip.width, strip.y + strip.height],
        rgba(0x22d3ee, 0.85),
    )
    .thickness(1.5 * dpi)
    .build();
    // One tick per slot boundary, so the nine slots are countable.
    for slot in 1..9 {
        let x = strip.x + strip.width * (slot as f32 / 9.0);
        draw.add_line([x, strip.y], [x, strip.y + strip.height], rgba(0x22d3ee, 0.45))
            .thickness(1.0 * dpi)
            .build();
    }
}

/// The virtual mouse's cursor: a small arrow, drawn where the finger last put it.
///
/// A phone has no pointer, and every Minecraft screen that is not the world hit-tests
/// a cursor position. The arrow is drawn by us rather than by the game so that it is
/// the same shape over the world, over a menu and over a chest, and so that it is
/// visibly *ours*: a player who cannot see a cursor has no way to know the mouse is
/// on.
pub fn draw_cursor(ui: &Ui, at: [f32; 2], scale: f32) {
    let draw = ui.get_window_draw_list();
    // A white arrow with a black outline, drawn as shapes rather than shipped as an
    // image: crisp at every size, no texture, no atlas, and the same shape on every
    // screen. The tip is the cursor's own point, which is what makes a click land
    // where the player aimed rather than where the middle of the arrow is.
    let s = 14.0 * scale.max(0.3);
    let [x, y] = at;
    let points = [
        [x, y],
        [x, y + s * 2.0],
        [x + s * 0.52, y + s * 1.42],
        [x + s * 0.86, y + s * 1.96],
        [x + s * 1.16, y + s * 1.80],
        [x + s * 0.82, y + s * 1.26],
        [x + s * 1.44, y + s * 1.26],
    ];
    // The outline first, a little wider than the fill, so the arrow reads on snow and
    // on the Nether alike.
    for i in 0..points.len() {
        let a = points[i];
        let b = points[(i + 1) % points.len()];
        draw.add_line([a[0], a[1]], [b[0], b[1]], rgba(0x000000, 0.95))
            .thickness(3.2 * scale.max(0.3))
            .build();
    }
    for i in 1..points.len() - 1 {
        draw.add_triangle(points[0], points[i], points[i + 1], rgba(0xffffff, 1.0))
            .filled(true)
            .build();
    }
}

/// The name field, on a pass of its own over the whole screen.
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
    {
        let draw = ui.get_window_draw_list();
        fill(&draw, [0.0, 0.0], [width, height], 0x000000, 0.62, 0.0);
    }
    let w = 400.0 * dpi;
    let h = 200.0 * dpi;
    let x = (width - w) / 2.0;
    let y = (height - h) / 2.0;
    {
        let draw = ui.get_window_draw_list();
        panel(&draw, c, [x, y], [x + w, y + h], c.panel, dpi);
        {
            let _f = ui.push_font(fonts.title);
            draw.add_text(
                [x + 20.0 * dpi, y + 18.0 * dpi],
                rgba(c.ink, 1.0),
                "Player name",
            );
        }
        {
            let _f = ui.push_font(fonts.small);
            draw.add_text(
                [x + 20.0 * dpi, y + 46.0 * dpi],
                rgba(c.muted, 1.0),
                "The name the game shows.",
            );
        }
    }
    let field_y = y + 78.0 * dpi;
    let field_w = w - 40.0 * dpi;
    field(ui, fonts, c, dpi, x + 20.0 * dpi, field_y, field_w, 42.0 * dpi, &model.draft);
    let by = y + h - 56.0 * dpi;
    if block_button(
        ui,
        fonts,
        c,
        dpi,
        x + w - 232.0 * dpi,
        by,
        96.0 * dpi,
        34.0 * dpi,
        "Cancel",
        Kind::Ghost,
    ) {
        model.editing = false;
        model.draft.clear();
    }
    if block_button(
        ui,
        fonts,
        c,
        dpi,
        x + w - 128.0 * dpi,
        by,
        108.0 * dpi,
        34.0 * dpi,
        "Save",
        Kind::Primary,
    ) {
        let name = model.draft.trim().to_string();
        if !name.is_empty() {
            sink.push(LauncherEvent::SetAccount(name));
        }
        model.editing = false;
    }
}

/// A text field: a sunk well with the draft in it.
#[allow(clippy::too_many_arguments)]
fn field(
    ui: &Ui,
    fonts: &Fonts,
    c: &Css,
    dpi: f32,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    text: &str,
) -> bool {
    {
        let draw = ui.get_window_draw_list();
        well(&draw, c, [x, y], [x + w, y + h], dpi);
        let _f = ui.push_font(fonts.body);
        let shown = fit(ui, text, w - 24.0 * dpi);
        let [_, th] = ui.calc_text_size(shown.as_str());
        draw.add_text(
            [x + 12.0 * dpi, y + (h - th) / 2.0],
            rgba(if text.is_empty() { c.muted } else { c.ink }, 1.0),
            if text.is_empty() { "Player" } else { shown.as_str() },
        );
    }
    ui.set_cursor_screen_pos([x, y]);
    ui.invisible_button("##field", [w, h])
}

/// The loading surface: the launcher's page colour, the mark, a spinner, and the one
/// line that says what is happening.
///
/// It is drawn over the launcher, so the page behind it cannot be tapped while the
/// game is coming up. The spinner is a ring with an arc that turns: nothing that
/// implies a percentage it does not have.
#[allow(clippy::too_many_arguments)]
pub fn draw_loading(
    ui: &Ui,
    screen: &LoadingScreen,
    fonts: &Fonts,
    dpi: f32,
    width: f32,
    height: f32,
    elapsed: f32,
    alpha: f32,
) {
    let c = &THEME;
    let _ = ui
        .window("##loading")
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
            {
                let draw = ui.get_window_draw_list();
                fill(&draw, [0.0, 0.0], [width, height], c.page, alpha, 0.0);
            }
            let cx = width / 2.0;
            let cy = height / 2.0;
            let radius = 24.0 * dpi;
            let ring_y = cy - 40.0 * dpi;
            {
                let draw = ui.get_window_draw_list();
                draw.add_circle([cx, ring_y], radius, rgba(c.outline, alpha))
                    .thickness(4.0 * dpi)
                    .num_segments(48)
                    .build();
                // An arc drawn as a short chain of points: imgui has no arc, and a
                // circle with a gap is what an indeterminate spinner is.
                let start = elapsed * std::f32::consts::TAU;
                let steps = 22;
                let span = 1.5f32;
                let mut last: Option<[f32; 2]> = None;
                for i in 0..=steps {
                    let a = start + span * (i as f32 / steps as f32);
                    let p = [cx + a.cos() * radius, ring_y + a.sin() * radius];
                    if let Some(from) = last {
                        draw.add_line(from, p, rgba(c.accent, alpha))
                            .thickness(4.0 * dpi)
                            .build();
                    }
                    last = Some(p);
                }
            }
            {
                let draw = ui.get_window_draw_list();
                let _f = ui.push_font(fonts.title);
                let title = if screen.title.is_empty() {
                    "Starting"
                } else {
                    screen.title.as_str()
                };
                let [tw, th] = ui.calc_text_size(title);
                draw.add_text([cx - tw / 2.0, cy + 4.0 * dpi], rgba(c.ink, alpha), title);
                let _f = ui.push_font(fonts.small);
                let detail = if screen.detail.is_empty() {
                    "This takes a moment on a phone."
                } else {
                    screen.detail.as_str()
                };
                let [dw, _] = ui.calc_text_size(detail);
                draw.add_text(
                    [cx - dw / 2.0, cy + 4.0 * dpi + th + 10.0 * dpi],
                    rgba(c.muted, alpha),
                    detail,
                );
            }
            if let Some(p) = screen.progress {
                let p = p.clamp(0.0, 1.0);
                let bw = 220.0 * dpi;
                let bx = cx - bw / 2.0;
                let by = cy + 68.0 * dpi;
                let draw = ui.get_window_draw_list();
                well_alpha(&draw, c, [bx, by], [bx + bw, by + 8.0 * dpi], dpi, alpha);
                if p > 0.0 {
                    fill(
                        &draw,
                        [bx + 2.0 * dpi, by + 2.0 * dpi],
                        [bx + 2.0 * dpi + (bw - 4.0 * dpi) * p, by + 6.0 * dpi],
                        c.accent,
                        alpha,
                        0.0,
                    );
                }
            }
        });
}
