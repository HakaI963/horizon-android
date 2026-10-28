//! The launcher's front end, drawn in imgui over the game's own surface.
//!
//! The desktop launcher is a GPUI app, and GPUI has no Android backend: a phone
//! runs one activity with one winit loop, so the launcher cannot be a second
//! program the way it is on a desktop. It is drawn here instead, in the imgui
//! layer that already paints the touch controls, so it shares the loop, the
//! device and the font atlas with the game.
//!
//! The design is Material 3, and the rules it follows are Material 3's own:
//! tonal surfaces rather than outlines, one accent for the action that matters,
//! pill-shaped controls, and a navigation rail instead of a list of tabs. There
//! is no bevel, no lip and no pixel lettering on a button: a button is a filled
//! shape whose state is its tone.
//!
//! The front page is the pack's own animated artwork, full bleed, with one scrim
//! and one Play button over it. Every other page is a surface with cards on it.
//!
//! This module knows how a launcher looks and nothing about what is installed:
//! the platform feeds it a [`LauncherModel`] of text and numbers it has gathered
//! from the engine, and it hands back [`LauncherEvent`]s for the platform to act
//! on. That keeps the crate free of the engine, the way the rest of the overlay
//! is.

use std::cell::Cell;

use imgui::{
    Condition, DrawListMut, FontConfig, FontId, FontSource, StyleColor, TextureId, Ui, WindowFlags,
};

/// How many destinations the rail has, and so how many state layers it keeps.
const NAV_ROWS: usize = PAGES.len();

thread_local! {
    /// The destination the pointer was on last frame, if any.
    static NAV_HOVER: Cell<Option<LauncherPage>> = const { Cell::new(None) };

    /// The eased state layer of each destination, 0 to 1, one per [`PAGES`] row.
    static NAV_GLOW: Cell<[f32; NAV_ROWS]> = const { Cell::new([0.0; NAV_ROWS]) };

    /// Whether the platform wants animations.
    static MOTION: Cell<bool> = const { Cell::new(true) };

    /// How far the Settings page is scrolled, in pixels.
    static SETTINGS_SCROLL: Cell<f32> = const { Cell::new(0.0) };

    /// Where the finger was on the last frame of a drag that scrolls a page.
    static SETTINGS_DRAG: Cell<Option<f32>> = const { Cell::new(None) };

    /// Whether a finger was down on the last frame, so a press is only anchored on
    /// the frame it begins.
    static SETTINGS_HELD: Cell<bool> = const { Cell::new(false) };

    /// Where the finger landed on a scrolling page.
    static SETTINGS_PRESS: Cell<Option<[f32; 2]>> = const { Cell::new(None) };

    /// Whether the gesture in progress became a scroll.
    static SETTINGS_SWALLOW: Cell<bool> = const { Cell::new(false) };

    /// Whether a slider was the widget holding the finger on the last frame.
    static SETTINGS_ON_TRACK: Cell<bool> = const { Cell::new(false) };
}

/// The destinations in the rail, in the order they are drawn.
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
    /// Reachable from the account chip, not from the rail.
    Settings,
    /// The launcher's own files.
    Files,
    /// Horizon's own multiplayer: hosting a world.
    Multiplayer,
}

impl LauncherPage {
    pub fn label(self) -> &'static str {
        match self {
            LauncherPage::Play => "Play",
            LauncherPage::Installations => "Versions",
            LauncherPage::Skins => "Skins",
            LauncherPage::PatchNotes => "Patch Notes",
            LauncherPage::News => "News",
            LauncherPage::Mods => "Mods",
            LauncherPage::Profile => "Accounts",
            LauncherPage::Settings => "Settings",
            LauncherPage::Files => "Files",
            LauncherPage::Multiplayer => "Host",
        }
    }

    /// The one line a page says about itself, under its title.
    fn subtitle(self) -> &'static str {
        match self {
            LauncherPage::Play => "",
            LauncherPage::Installations => "The Minecraft build this launcher runs.",
            LauncherPage::Skins => "Wear a skin of your own.",
            LauncherPage::PatchNotes => "What changed in each Horizon build.",
            LauncherPage::News => "Horizon news and updates.",
            LauncherPage::Mods => "Ember mods: add what the game does not have.",
            LauncherPage::Profile => "Who is playing.",
            LauncherPage::Settings => "How the launcher and the game behave.",
            LauncherPage::Files => "Where the game reads its files.",
            LauncherPage::Multiplayer => "Host this world for the room.",
        }
    }
}

/// The rail's order. `Settings` is reached from the account chip, and the pages
/// that are reached from elsewhere are not in it: five destinations is what fits a
/// phone in landscape without the rail becoming a second screen.
pub const PAGES: [LauncherPage; 5] = [
    LauncherPage::Play,
    LauncherPage::Skins,
    LauncherPage::Mods,
    LauncherPage::Files,
    LauncherPage::Multiplayer,
];

/// One row of a list page.
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
    /// The key is the launcher's own rather than an `options.txt` name. The platform
    /// reads these itself, and `key` is the name it knows them by.
    pub launcher: bool,
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
    /// Where the model is turned to, in radians.
    pub skin_yaw: f32,
    /// Which built-in touch layout is in force: `"java"` or `"bedrock"`.
    pub controls_preset: String,
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
///
/// It exists because the gap between a tap and the first frame of the world is real:
/// the pack is read, the world is meshed, and the GPU is warmed, and none of that can
/// happen on the thread that draws. Drawing this instead is what makes that gap an
/// animation rather than a freeze.
#[derive(Clone, Debug, Default)]
pub struct LoadingScreen {
    /// The line under the title: "Starting the game", "Opening world".
    pub title: String,
    /// What is being worked on right now, if the platform can say.
    pub detail: String,
    /// 0..1 when the platform knows how far along it is. `None` is an indeterminate
    /// bar, which is the honest answer for work that has no step count.
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
    /// The card's install landed but the loop it would need to play is already spent.
    Restart,
    /// The account card was tapped.
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
    /// Pick a file of this kind: "skin", "mod", "resourcepack", "settings".
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
    /// Delete this installed `.hmod`.
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

/// The palette. Material 3's roles, on the dark surface the artwork asks for:
/// a near-black base, tonal containers that step up in lightness, and one warm
/// accent taken from the artwork's own orange.
pub struct Css {
    /// The window's base.
    pub page: u32,
    /// The rail: one step up from the base.
    pub side: u32,
    /// A card, a row, a field: `surface-container`.
    pub panel: u32,
    /// `surface-container-high`: a card under a finger, a selected row.
    pub raised: u32,
    /// `surface-container-highest`: a field, a well.
    pub button: u32,
    /// A sunken well: a slider track.
    pub well: u32,
    pub rule: u32,
    pub bevel: u32,
    pub ink: u32,
    pub ink_soft: u32,
    pub muted: u32,
    /// The accent: the one action that matters.
    pub accent: u32,
    /// The accent's container, for a selected chip.
    pub accent_soft: u32,
    pub accent_lip: u32,
    pub on_accent: u32,
    pub danger: u32,
}

pub const THEME: Css = Css {
    page: 0x0e1014,
    side: 0x14171d,
    panel: 0x1a1e25,
    raised: 0x232830,
    button: 0x2b313a,
    well: 0x0a0c0f,
    rule: 0x2a2f38,
    bevel: 0x3b424e,
    ink: 0xf4f6f9,
    ink_soft: 0xc9cfd8,
    muted: 0x8d95a3,
    accent: 0xff9f43,
    accent_soft: 0x3d2a12,
    accent_lip: 0xd97b1f,
    on_accent: 0x1a1206,
    danger: 0xf07a6a,
};

/// The accent under a finger: one step brighter, never a different hue.
const ACCENT_HOVER: u32 = 0xffb463;

fn rgba(c: u32, a: f32) -> [f32; 4] {
    [
        ((c >> 16) & 0xff) as f32 / 255.0,
        ((c >> 8) & 0xff) as f32 / 255.0,
        (c & 0xff) as f32 / 255.0,
        a,
    ]
}

/// One frame of a state layer: `current` moving towards `target` at a rate set by
/// the frame's own time, so the same fade takes the same time at any refresh rate.
fn ease_glow(current: f32, target: f32, dt: f32) -> f32 {
    if dt <= 0.0 {
        return current;
    }
    let k = 1.0 - (-dt / NAV_GLOW_TAU).exp();
    (current + (target - current) * k).clamp(0.0, 1.0)
}

/// The faces the launcher draws with. Geist for everything: Material 3 is a
/// typographic scale, not a typeface, and a pixel font is the one thing that made
/// the old pages read as a game menu rather than as a launcher.
pub struct Fonts {
    /// Body text and labels.
    pub body: FontId,
    /// Headings and the page title.
    pub title: FontId,
    /// The display face, for the one number on the Play page.
    pub display: FontId,
    /// Dense rows, captions, units.
    pub small: FontId,
    /// Buttons, which Material 3 sets a little heavier than body text.
    pub label: FontId,
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
    let title = imgui.fonts().add_font(&[FontSource::TtfData {
        data: horizon_theme::fonts::GEIST_SEMIBOLD,
        size_pixels: 26.0 * dpi,
        config: Some(FontConfig::default()),
    }]);
    let display = imgui.fonts().add_font(&[FontSource::TtfData {
        data: horizon_theme::fonts::GEIST_SEMIBOLD,
        size_pixels: 40.0 * dpi,
        config: Some(FontConfig::default()),
    }]);
    let small = imgui.fonts().add_font(&[FontSource::TtfData {
        data: horizon_theme::fonts::GEIST_REGULAR,
        size_pixels: 12.5 * dpi,
        config: Some(FontConfig::default()),
    }]);
    let label = imgui.fonts().add_font(&[FontSource::TtfData {
        data: horizon_theme::fonts::GEIST_MEDIUM,
        size_pixels: 15.0 * dpi,
        config: Some(FontConfig::default()),
    }]);

    let c = &THEME;
    let style = imgui.style_mut();
    style.window_rounding = 0.0;
    style.frame_rounding = M3_CARD;
    style.scrollbar_rounding = M3_PILL;
    style.grab_rounding = M3_PILL;
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
        title,
        display,
        small,
        label,
    }
}

// Material 3's shape scale, in logical pixels: a card, a small card, a button,
// a chip, and a pill. A phone in landscape is about 720 logical pixels tall, so
// the scale is the compact one.
const M3_CARD: f32 = 16.0;
const M3_CARD_SMALL: f32 = 12.0;
const M3_PILL: f32 = 999.0;

/// The rail's width, the height of a destination, and the page's gutter.
const RAIL_W: f32 = 84.0;
const NAV_H: f32 = 56.0;
const NAV_GAP: f32 = 4.0;
const PAD: f32 = 24.0;

/// How far a page's content slides in when the page changes, and how heavy the veil
/// over it starts.
const PAGE_SLIDE: f32 = 10.0;
const PAGE_VEIL: f32 = 0.35;

/// How far a filled button's face drops under a finger, in logical pixels.
const PRESS_DROP: f32 = 1.0;

/// The state layer of a rail destination: its weight at full strength, and the time
/// constant of its approach.
const NAV_GLOW_ALPHA: f32 = 0.14;
const NAV_GLOW_TAU: f32 = 0.09;

/// The background's own grid, so one cell can be sampled out of the atlas: 8 columns
/// by 4 rows of 480x270 frames.
pub const BG_COLS: u32 = 8;
pub const BG_ROWS: u32 = 4;
/// How long one full pass through the artwork takes, in seconds. The pack's own
/// sequence runs at 20 frames a second; thinned to keyframes and cross-faded, this
/// is the speed that still reads as the same motion.
pub const BG_PERIOD: f32 = 5.3;

/// The one geometry every element is placed from.
#[derive(Clone, Copy)]
struct Grid {
    /// The page's left edge: just past the rail.
    x: f32,
    /// The page's width.
    w: f32,
    /// The gutter inside the page.
    pad: f32,
    /// The rail's width.
    side: f32,
}

impl Grid {
    fn new(width: f32, dpi: f32) -> Self {
        let side = RAIL_W * dpi;
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

/// A Material 3 card: a tonal surface, no outline. Elevation is a lighter tone, not
/// a shadow and not a border.
fn panel(draw: &DrawListMut<'_>, min: [f32; 2], max: [f32; 2], face: u32, radius: f32) {
    fill(draw, min, max, face, 1.0, radius);
}

/// A colour at 88% of its own brightness: the face of a filled button under a
/// finger. Material 3 answers a press with a state layer rather than a new colour,
/// but a tonal shift reads better than a translucent overlay on a flat phone screen.
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
/// `opening` is `Some(t)` while the opening runs, with `t` in 0..1. `fade` is how far
/// a page change has got. `reduce_motion` is the system's "remove animations"
/// setting. `background` is the pack's animated artwork, uploaded by the platform;
/// `None` means the page paints its own surface instead.
#[allow(clippy::too_many_arguments)]
pub fn draw(
    ui: &Ui,
    model: &mut LauncherModel,
    fonts: &Fonts,
    dpi: f32,
    width: f32,
    height: f32,
    opening: Option<f32>,
    // The page change, 0 to 1, as the overlay timed it.
    fade: f32,
    // The system's "remove animations" setting.
    reduce_motion: bool,
    // The pack's animated artwork: an atlas of frames, and the frame the platform
    // has reached. `None` before the texture is uploaded, and on the bring-up card.
    background: Option<(TextureId, [u32; 2], f32)>,
    skin: Option<TextureId>,
    out: &mut Vec<LauncherEvent>,
) {
    let c = &THEME;
    MOTION.with(|m| m.set(!reduce_motion));
    if reduce_motion {
        NAV_GLOW.with(|g| g.set([0.0; NAV_ROWS]));
        NAV_HOVER.with(|h| h.set(None));
    }
    let live = !matches!(opening, Some(t) if t < 1.0);
    let mut sink = Sink { events: out, live };
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
            // The page the *model* was built for: the rail applies a tap to
            // `model.page` in this same pass, and the platform only learns about it on
            // the next frame, so the content has to be the page whose data arrived.
            let page = model.page;
            if page != LauncherPage::Settings {
                SETTINGS_PRESS.set(None);
                SETTINGS_DRAG.set(None);
                SETTINGS_SWALLOW.set(false);
                SETTINGS_SCROLL.set(0.0);
                SETTINGS_HELD.set(ui.io().mouse_down[0]);
            }
            // The base surface. The Play page draws the artwork over it; every other
            // page leaves it showing.
            {
                let draw = ui.get_window_draw_list();
                fill(&draw, [0.0, 0.0], [width, height], c.page, 1.0, 0.0);
            }
            if page == LauncherPage::Play {
                play_page(
                    ui,
                    model,
                    fonts,
                    c,
                    dpi,
                    &page_g,
                    height,
                    rise,
                    background,
                    &mut sink,
                );
            } else {
                // A top bar with the page's name and the account chip, then the page.
                let top = top_bar(ui, model, fonts, c, dpi, &page_g, rise, &mut sink);
                match page {
                    LauncherPage::Installations => installations_page(
                        ui, model, fonts, c, dpi, &page_g, height, top, &mut sink,
                    ),
                    LauncherPage::Skins => skins_page(
                        ui, model, fonts, c, dpi, &page_g, height, top, skin, &mut sink,
                    ),
                    LauncherPage::PatchNotes => stories_page(
                        ui,
                        &model.patches,
                        fonts,
                        c,
                        dpi,
                        &page_g,
                        height,
                        top,
                        "PATCH NOTES",
                    ),
                    LauncherPage::News => stories_page(
                        ui,
                        &model.news,
                        fonts,
                        c,
                        dpi,
                        &page_g,
                        height,
                        top,
                        "NEWS",
                    ),
                    LauncherPage::Mods => mods_page(
                        ui, model, fonts, c, dpi, &page_g, height, top, &mut sink,
                    ),
                    LauncherPage::Profile => profile_page(
                        ui, model, fonts, c, dpi, &page_g, height, top, &mut sink,
                    ),
                    LauncherPage::Settings => settings_page(
                        ui, model, fonts, c, dpi, &page_g, height, top, &mut sink,
                    ),
                    LauncherPage::Files => files_page(
                        ui, model, fonts, c, dpi, &page_g, height, top, &mut sink,
                    ),
                    LauncherPage::Multiplayer => multiplayer_page(
                        ui, model, fonts, c, dpi, &page_g, height, top, &mut sink,
                    ),
                    LauncherPage::Play => {}
                }
            }
            rail(ui, model, fonts, c, dpi, width, height, rise, &mut sink);
            // The page change: the content fades up out of the surface. Only the page's
            // region, so the rail the finger is on does not dim under it.
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

/// The artwork, full bleed, sampled one cell at a time out of the atlas.
///
/// The pack's frames are thinned to keyframes, so the motion between two of them is
/// made by drawing the second over the first at a fraction of its weight. That is a
/// cross-fade rather than a step, and on artwork this still it reads as the original
/// sequence: the doves and the cape move, nothing jumps.
fn artwork(
    draw: &DrawListMut<'_>,
    texture: TextureId,
    size: [u32; 2],
    frame: f32,
    width: f32,
    height: f32,
) {
    let (aw, ah) = (size[0] as f32, size[1] as f32);
    if aw <= 0.0 || ah <= 0.0 {
        return;
    }
    let cell_w = aw / BG_COLS as f32;
    let cell_h = ah / BG_ROWS as f32;
    let total = (BG_COLS * BG_ROWS) as usize;
    if total == 0 || cell_w <= 0.0 || cell_h <= 0.0 {
        return;
    }
    // `frame` is the position in the sequence, in cells: the whole part is the frame
    // to show and the fraction is how far the next one has come in.
    let at = frame.rem_euclid(total as f32);
    let index = (at.floor() as usize) % total;
    let next = (index + 1) % total;
    let mix = (at - at.floor()).clamp(0.0, 1.0);
    let cell = |i: usize| {
        let col = (i % BG_COLS as usize) as f32;
        let row = (i / BG_COLS as usize) as f32;
        let u0 = col * cell_w / aw;
        let v0 = row * cell_h / ah;
        (
            [u0, v0],
            [(col + 1.0) * cell_w / aw, v0],
            [(col + 1.0) * cell_w / aw, (row + 1.0) * cell_h / ah],
            [u0, (row + 1.0) * cell_h / ah],
        )
    };
    // Cover: the artwork is 16:9 and the page is whatever the phone is, so the image
    // is scaled to fill and the overflow is cropped evenly rather than letterboxed.
    let (iw, ih) = (cell_w, cell_h);
    let scale = (width / iw).max(height / ih);
    let (dw, dh) = (iw * scale, ih * scale);
    let x0 = (width - dw) / 2.0;
    let y0 = (height - dh) / 2.0;
    let (a, b, d, e) = cell(index);
    draw.add_image_quad(
        texture,
        [x0, y0],
        [x0 + dw, y0],
        [x0 + dw, y0 + dh],
        [x0, y0 + dh],
    )
    .uv(a, b, d, e)
    .build();
    if mix > 0.004 {
        let (a, b, d, e) = cell(next);
        draw.add_image_quad(
            texture,
            [x0, y0],
            [x0 + dw, y0],
            [x0 + dw, y0 + dh],
            [x0, y0 + dh],
        )
        .uv(a, b, d, e)
        .col(rgba(0xffffff, mix))
        .build();
    }
}

/// The front page: the artwork, one scrim, the name of the build, and Play.
///
/// Material 3 keeps a hero page to one action and one line of context. Everything
/// else a player might want is one tap away in the rail, so this page has no cards,
/// no lists and no second button competing with Play.
fn play_page(
    ui: &Ui,
    model: &LauncherModel,
    fonts: &Fonts,
    c: &Css,
    dpi: f32,
    g: &Grid,
    height: f32,
    rise: f32,
    background: Option<(TextureId, [u32; 2], f32)>,
    sink: &mut Sink<'_>,
) {
    let playable = model.playable && !model.instance.is_empty();
    {
        let draw = ui.get_window_draw_list();
        if let Some((texture, size, frame)) = background {
            artwork(&draw, texture, size, frame, g.x + g.w, height);
        }
        // The scrim: clear at the top where the art is, opaque at the bottom where
        // Play sits. One quad, so the ramp is continuous at every pixel.
        let top = height * 0.30;
        let clear = rgba(c.page, 0.0);
        let mid = height * 0.66;
        draw.add_rect_filled_multicolor(
            [g.x, top],
            [g.x + g.w, mid],
            clear,
            clear,
            rgba(c.page, 0.80),
            rgba(c.page, 0.80),
        );
        draw.add_rect_filled_multicolor(
            [g.x, mid],
            [g.x + g.w, height + rise],
            rgba(c.page, 0.80),
            rgba(c.page, 0.80),
            rgba(c.page, 0.97),
            rgba(c.page, 0.97),
        );
        if background.is_none() {
            fill(&draw, [g.x, 0.0], [g.x + g.w, top], c.page, 1.0, 0.0);
        }
        // A soft left edge so the artwork does not meet the rail in a hard line.
        let fade = 96.0 * dpi;
        draw.add_rect_filled_multicolor(
            [g.x, 0.0],
            [g.x + fade, height + rise],
            rgba(c.page, 0.70),
            rgba(c.page, 0.0),
            rgba(c.page, 0.0),
            rgba(c.page, 0.70),
        );
    }

    // The build's name, top left: a small overline and the version.
    {
        let draw = ui.get_window_draw_list();
        let _f = ui.push_font(fonts.small);
        draw.add_text(
            [g.x + g.pad, 26.0 * dpi + rise],
            rgba(c.ink_soft, 0.85),
            "MINECRAFT JAVA EDITION",
        );
    }
    {
        let draw = ui.get_window_draw_list();
        let _f = ui.push_font(fonts.display);
        draw.add_text(
            [g.x + g.pad, 44.0 * dpi + rise],
            rgba(c.ink, 1.0),
            "Horizon",
        );
    }

    // The account chip, top right.
    account_chip(ui, model, fonts, c, dpi, g, rise, sink);

    // Bottom right: Play. The one filled button on the screen.
    let btn_w = 208.0 * dpi;
    let btn_h = 60.0 * dpi;
    let btn_x = g.right(btn_w);
    let btn_y = height + rise - g.pad - btn_h;
    let label = match &model.front {
        Some(text) => text.clone(),
        None if playable => "Play".to_string(),
        None => "Install".to_string(),
    };
    let (face, ink) = if playable {
        (c.accent, c.on_accent)
    } else {
        (c.button, c.muted)
    };
    if filled_button(
        ui, fonts, c, dpi, btn_x, btn_y, btn_w, btn_h, &label, face, ink, M3_PILL,
    ) {
        sink.push(match model.front.as_deref() {
            Some(_) => LauncherEvent::Restart,
            None if playable => LauncherEvent::Play,
            None => LauncherEvent::Install,
        });
    }

    // Bottom left: the build, and what it is doing.
    {
        let draw = ui.get_window_draw_list();
        let x = g.x + g.pad;
        let mut y = btn_y + 2.0 * dpi;
        {
            let _f = ui.push_font(fonts.body);
            draw.add_text([x, y], rgba(c.ink_soft, 1.0), fit(ui, &model.version, btn_x - x - g.pad));
        }
        y += 22.0 * dpi;
        if !model.status.is_empty() {
            let _f = ui.push_font(fonts.small);
            draw.add_text(
                [x, y],
                rgba(c.muted, 1.0),
                fit(ui, &model.status, (btn_x - x - g.pad).max(0.0)),
            );
        }
        if let Some(p) = model.progress {
            let p = p.clamp(0.0, 1.0);
            let w = (btn_x - x - g.pad).min(240.0 * dpi);
            let t = y + 18.0 * dpi;
            fill(&draw, [x, t], [x + w, t + 3.0 * dpi], c.well, 1.0, M3_PILL);
            if p > 0.0 {
                fill(&draw, [x, t], [x + w * p, t + 3.0 * dpi], c.accent, 1.0, M3_PILL);
            }
        }
    }
}

/// The account chip: an avatar and the name, which opens the Accounts page.
fn account_chip(
    ui: &Ui,
    model: &LauncherModel,
    fonts: &Fonts,
    c: &Css,
    dpi: f32,
    g: &Grid,
    rise: f32,
    sink: &mut Sink<'_>,
) {
    let h = 40.0 * dpi;
    let y = 20.0 * dpi + rise;
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
    let name = fit(ui, &model.account, 130.0 * dpi);
    let name_w = text_width(ui, fonts.body, &name);
    let w = 8.0 * dpi + h + 10.0 * dpi + name_w + 14.0 * dpi;
    let x = g.x + g.w - g.pad - w;
    ui.set_cursor_screen_pos([x, y]);
    let clicked = ui.invisible_button("##accountchip", [w, h]);
    let hovered = ui.is_item_hovered();
    {
        let draw = ui.get_window_draw_list();
        // A state layer over the artwork, not a solid plate: the art is the point of
        // this page, so the chip sits on it rather than covering it.
        fill(
            &draw,
            [x, y],
            [x + w, y + h],
            c.ink,
            if hovered { 0.16 } else { 0.10 },
            M3_PILL,
        );
        let face = h - 8.0 * dpi;
        let fx = x + 4.0 * dpi;
        let fy = y + 4.0 * dpi;
        fill(&draw, [fx, fy], [fx + face, fy + face], c.accent, 1.0, face / 2.0);
        {
            let _f = ui.push_font(fonts.label);
            let [iw, ih] = ui.calc_text_size(initial.as_str());
            draw.add_text(
                [fx + (face - iw) / 2.0, fy + (face - ih) / 2.0],
                rgba(c.on_accent, 1.0),
                initial.as_str(),
            );
        }
        {
            let _f = ui.push_font(fonts.body);
            let [_, th] = ui.calc_text_size(name.as_str());
            draw.add_text(
                [fx + face + 10.0 * dpi, y + (h - th) / 2.0],
                rgba(c.ink, 1.0),
                name.as_str(),
            );
        }
    }
    if clicked {
        sink.push(LauncherEvent::Goto(LauncherPage::Profile));
    }
}

/// The top bar on every page that is not the front page: the title, its one line of
/// context, and the account chip. Returns the y the page's content starts at.
#[allow(clippy::too_many_arguments)]
fn top_bar(
    ui: &Ui,
    model: &LauncherModel,
    fonts: &Fonts,
    c: &Css,
    dpi: f32,
    g: &Grid,
    rise: f32,
    sink: &mut Sink<'_>,
) -> f32 {
    let x = g.x + g.pad;
    let y = 26.0 * dpi + rise;
    {
        let draw = ui.get_window_draw_list();
        let _f = ui.push_font(fonts.title);
        draw.add_text([x, y], rgba(c.ink, 1.0), model.page.label());
        let _f = ui.push_font(fonts.small);
        draw.add_text(
            [x, y + 36.0 * dpi],
            rgba(c.muted, 1.0),
            fit(ui, model.page.subtitle(), g.w - g.pad * 2.0 - 180.0 * dpi),
        );
    }
    account_chip(ui, model, fonts, c, dpi, g, rise, sink);
    y + 66.0 * dpi
}

/// The navigation rail: the destinations, and Settings at the bottom.
///
/// Material 3's rail is a pill-shaped indicator behind an icon with the label under
/// it. The indicator is the only thing that moves: it slides from the old row to the
/// new one as a state layer, and the icon it lands under turns to the accent.
#[allow(clippy::too_many_arguments)]
fn rail(
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
    let side = RAIL_W * dpi;
    let _ = width;
    {
        let draw = ui.get_window_draw_list();
        fill(&draw, [0.0, 0.0], [side, height + rise], c.side, 1.0, 0.0);
        // The seam: a tonal step, not a line.
        fill(&draw, [side, 0.0], [side + 1.0, height + rise], c.rule, 0.55, 0.0);
    }
    let row_h = NAV_H * dpi;
    let gap = NAV_GAP * dpi;
    let dt = ui.io().delta_time;
    let before = NAV_HOVER.with(|h| h.get());
    let mut hovered: Option<LauncherPage> = None;
    // The rail is centred vertically in what is left after the brand mark.
    let brand = 46.0 * dpi;
    let total = PAGES.len() as f32 * row_h + (PAGES.len() as f32 - 1.0) * gap;
    let mut y = (height + rise - total) / 2.0 + brand * 0.4;

    // The brand mark: one filled tile with the accent, which is the launcher's own
    // identity rather than a picture of the game.
    {
        let draw = ui.get_window_draw_list();
        let s = 34.0 * dpi;
        let bx = (side - s) / 2.0;
        let by = 20.0 * dpi + rise;
        fill(&draw, [bx, by], [bx + s, by + s], c.accent, 1.0, M3_CARD_SMALL * dpi);
        // A horizon: a bar with the sun coming up over it.
        let cx = bx + s / 2.0;
        draw.add_circle([cx, by + s * 0.60], s * 0.15, rgba(c.on_accent, 1.0)).filled(true).build();
        fill(
            &draw,
            [bx + s * 0.22, by + s * 0.62],
            [bx + s * 0.78, by + s * 0.68],
            c.on_accent,
            1.0,
            1.0,
        );
    }

    for (i, page) in PAGES.iter().enumerate() {
        let page = *page;
        let selected = page == model.page
            || (page == LauncherPage::Files && model.page == LauncherPage::Settings);
        let x = 8.0 * dpi;
        let w = side - 16.0 * dpi;
        let glow = NAV_GLOW.with(|g| g.get()[i]);
        {
            let draw = ui.get_window_draw_list();
            if selected {
                // The indicator: a pill behind the icon, at the accent's container
                // tone. This is Material 3's selected state, not an outline.
                let ih = 32.0 * dpi;
                let iw = 56.0 * dpi;
                fill(
                    &draw,
                    [x + (w - iw) / 2.0, y + 4.0 * dpi],
                    [x + (w + iw) / 2.0, y + 4.0 * dpi + ih],
                    c.accent,
                    1.0,
                    M3_PILL * dpi,
                );
            } else if glow > 0.002 {
                let ih = 32.0 * dpi;
                let iw = 56.0 * dpi;
                fill(
                    &draw,
                    [x + (w - iw) / 2.0, y + 4.0 * dpi],
                    [x + (w + iw) / 2.0, y + 4.0 * dpi + ih],
                    c.ink,
                    glow * NAV_GLOW_ALPHA,
                    M3_PILL * dpi,
                );
            }
            let icon = 22.0 * dpi;
            nav_icon(
                &draw,
                page,
                x + w / 2.0 - icon / 2.0,
                y + 4.0 * dpi + 16.0 * dpi,
                icon,
                if selected { c.on_accent } else { c.ink_soft },
            );
            {
                let _f = ui.push_font(fonts.small);
                let label = page.label();
                let [lw, _] = ui.calc_text_size(label);
                draw.add_text(
                    [x + (w - lw) / 2.0, y + 40.0 * dpi],
                    rgba(if selected { c.ink } else { c.muted }, 1.0),
                    label,
                );
            }
        }
        ui.set_cursor_screen_pos([x, y]);
        let id = format!("##nav{i}");
        let clicked = ui.invisible_button(id.as_str(), [w, row_h]);
        if MOTION.with(|m| m.get()) && (ui.is_item_hovered() || ui.is_item_active()) {
            hovered = Some(page);
        }
        if clicked {
            if sink.live() {
                model.page = page;
            }
            sink.push(LauncherEvent::Goto(page));
        }
        y += row_h + gap;
    }

    // Settings and Quit, pinned to the bottom: things you go to on purpose.
    let mut by = height + rise - 14.0 * dpi - 52.0 * dpi * 2.0 - 6.0 * dpi;
    for (i, page) in [LauncherPage::Settings, LauncherPage::Profile].iter().enumerate() {
        let page = *page;
        let selected = page == model.page;
        let x = 8.0 * dpi;
        let w = side - 16.0 * dpi;
        let h = 52.0 * dpi;
        {
            let draw = ui.get_window_draw_list();
            if selected {
                let ih = 32.0 * dpi;
                let iw = 56.0 * dpi;
                fill(
                    &draw,
                    [x + (w - iw) / 2.0, by + 2.0 * dpi],
                    [x + (w + iw) / 2.0, by + 2.0 * dpi + ih],
                    c.accent,
                    1.0,
                    M3_PILL * dpi,
                );
            }
            let icon = 22.0 * dpi;
            nav_icon(
                &draw,
                page,
                x + w / 2.0 - icon / 2.0,
                by + 2.0 * dpi + 16.0 * dpi,
                icon,
                if selected { c.on_accent } else { c.ink_soft },
            );
            let _f = ui.push_font(fonts.small);
            let label = if page == LauncherPage::Profile {
                "Quit"
            } else {
                page.label()
            };
            let [lw, _] = ui.calc_text_size(label);
            draw.add_text(
                [x + (w - lw) / 2.0, by + 38.0 * dpi],
                rgba(if selected { c.ink } else { c.muted }, 1.0),
                label,
            );
        }
        ui.set_cursor_screen_pos([x, by]);
        let id = format!("##foot{i}");
        if ui.invisible_button(id.as_str(), [w, h]) {
            if page == LauncherPage::Profile {
                sink.push(LauncherEvent::Quit);
            } else {
                sink.push(LauncherEvent::Goto(page));
            }
        }
        by += h + 6.0 * dpi;
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
    NAV_HOVER.with(|h| h.set(hovered));
}
/// A filled button: Material 3's filled style. One tonal face, one label, no bevel
/// and no lip. A press shifts the tone and drops the face a pixel, which is the whole
/// of the feedback a flat button needs.
#[allow(clippy::too_many_arguments)]
fn filled_button(
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
    ink: u32,
    radius: f32,
) -> bool {
    ui.set_cursor_screen_pos([x, y]);
    let clicked = ui.invisible_button(label, [w, h]);
    let down = MOTION.with(|m| m.get()) && ui.is_item_active();
    let hovered = ui.is_item_hovered();
    let drop = if down { PRESS_DROP * dpi } else { 0.0 };
    let top = y + drop;
    let r = if radius >= M3_PILL { h / 2.0 } else { radius * dpi };
    {
        let draw = ui.get_window_draw_list();
        let base = if down {
            darker(face)
        } else if hovered {
            // A state layer: the face brightens one step rather than gaining a border.
            if face == c.accent { ACCENT_HOVER } else { c.raised }
        } else {
            face
        };
        fill(&draw, [x, top], [x + w, top + h], base, 1.0, r);
        {
            let _f = ui.push_font(fonts.label);
            let [tw, th] = ui.calc_text_size(label);
            draw.add_text(
                [x + (w - tw) / 2.0, top + (h - th) / 2.0],
                rgba(ink, 1.0),
                label,
            );
        }
    }
    clicked
}

/// A text button: the low-emphasis action, an accent label with no container.
fn text_button(
    ui: &Ui,
    fonts: &Fonts,
    c: &Css,
    dpi: f32,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    label: &str,
    tone: u32,
) -> bool {
    ui.set_cursor_screen_pos([x, y]);
    let clicked = ui.invisible_button(label, [w, h]);
    let hovered = ui.is_item_hovered() || ui.is_item_active();
    {
        let draw = ui.get_window_draw_list();
        if hovered {
            fill(&draw, [x, y], [x + w, y + h], c.ink, 0.07, M3_PILL * dpi);
        }
        let _f = ui.push_font(fonts.label);
        let [tw, th] = ui.calc_text_size(label);
        draw.add_text(
            [x + (w - tw) / 2.0, y + (h - th) / 2.0],
            rgba(tone, 1.0),
            label,
        );
    }
    clicked
}

/// An outlined button: the medium-emphasis action. One hairline, one label.
#[allow(clippy::too_many_arguments)]
fn outlined_button(
    ui: &Ui,
    fonts: &Fonts,
    c: &Css,
    dpi: f32,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    label: &str,
    tone: u32,
) -> bool {
    ui.set_cursor_screen_pos([x, y]);
    let clicked = ui.invisible_button(label, [w, h]);
    let hovered = ui.is_item_hovered() || ui.is_item_active();
    {
        let draw = ui.get_window_draw_list();
        if hovered {
            fill(&draw, [x, y], [x + w, y + h], tone, 0.10, M3_PILL * dpi);
        }
        stroke(&draw, [x, y], [x + w, y + h], c.bevel, 1.0, M3_PILL * dpi, 1.0);
        let _f = ui.push_font(fonts.label);
        let [tw, th] = ui.calc_text_size(label);
        draw.add_text(
            [x + (w - tw) / 2.0, y + (h - th) / 2.0],
            rgba(tone, 1.0),
            label,
        );
    }
    clicked
}

/// How wide a chip with this label is.
fn chip_width(ui: &Ui, fonts: &Fonts, dpi: f32, label: &str) -> f32 {
    text_width(ui, fonts.small, label) + 26.0 * dpi
}

/// A chip: a small pill for a filter or a tag that can be chosen.
#[allow(clippy::too_many_arguments)]
fn chip(
    ui: &Ui,
    fonts: &Fonts,
    c: &Css,
    dpi: f32,
    x: f32,
    y: f32,
    label: &str,
    selected: bool,
) -> bool {
    let w = chip_width(ui, fonts, dpi, label);
    let h = 30.0 * dpi;
    ui.set_cursor_screen_pos([x, y]);
    let clicked = ui.invisible_button(label, [w, h]);
    let hovered = ui.is_item_hovered() || ui.is_item_active();
    {
        let draw = ui.get_window_draw_list();
        if selected {
            fill(&draw, [x, y], [x + w, y + h], c.accent, 1.0, M3_PILL * dpi);
        } else {
            stroke(&draw, [x, y], [x + w, y + h], c.bevel, 1.0, M3_PILL * dpi, 1.0);
            if hovered {
                fill(&draw, [x, y], [x + w, y + h], c.ink, 0.07, M3_PILL * dpi);
            }
        }
        let _f = ui.push_font(fonts.small);
        let [tw, th] = ui.calc_text_size(label);
        draw.add_text(
            [x + (w - tw) / 2.0, y + (h - th) / 2.0],
            rgba(if selected { c.on_accent } else { c.ink_soft }, 1.0),
            label,
        );
    }
    clicked
}

/// A destination's mark, drawn as geometry in one colour so it takes the tone of the
/// state it is in: on the accent when selected, soft ink when not.
fn nav_icon(draw: &DrawListMut<'_>, page: LauncherPage, x: f32, y: f32, s: f32, col: u32) {
    let u = s / 24.0;
    let px = |gx: f32, gy: f32, gw: f32, gh: f32| {
        [
            x + gx * u,
            y + gy * u,
            x + (gx + gw) * u,
            y + (gy + gh) * u,
        ]
    };
    let block = |gx: f32, gy: f32, gw: f32, gh: f32| {
        let r = px(gx, gy, gw, gh);
        fill(draw, [r[0], r[1]], [r[2], r[3]], col, 1.0, 0.0);
    };
    let round = |gx: f32, gy: f32, gw: f32, gh: f32, rad: f32| {
        let r = px(gx, gy, gw, gh);
        fill(draw, [r[0], r[1]], [r[2], r[3]], col, 1.0, rad * u);
    };
    match page {
        // A play triangle in a rounded square.
        LauncherPage::Play => {
            round(2.0, 2.0, 20.0, 20.0, 5.0);
            let cx = x + 10.5 * u;
            let cy = y + 12.0 * u;
            draw.add_triangle(
                [cx, cy - 5.0 * u],
                [cx, cy + 5.0 * u],
                [cx + 5.5 * u, cy],
                rgba(0x0e1014, 1.0),
            )
            .filled(true)
            .build();
        }
        // A head and shoulders: a player.
        LauncherPage::Skins => {
            draw.add_circle([x + 12.0 * u, y + 8.0 * u], 4.2 * u, rgba(col, 1.0))
                .filled(true)
                .build();
            let r = px(4.0, 14.0, 16.0, 8.0);
            fill(draw, [r[0], r[1]], [r[2], r[3]], col, 1.0, 8.0 * u);
        }
        // A cube: a mod, a block added to the game.
        LauncherPage::Mods => {
            round(3.0, 3.0, 18.0, 18.0, 3.0);
            block(3.0, 3.0, 18.0, 4.0);
            block(10.5, 3.0, 3.0, 18.0);
        }
        // A folder.
        LauncherPage::Files => {
            round(2.0, 5.0, 9.0, 4.0, 1.5);
            round(2.0, 8.0, 20.0, 12.0, 2.5);
        }
        // A signal: a dot with two arcs.
        LauncherPage::Multiplayer => {
            draw.add_circle([x + 12.0 * u, y + 16.0 * u], 2.4 * u, rgba(col, 1.0))
                .filled(true)
                .build();
            stroke(
                draw,
                [x + 6.0 * u, y + 10.0 * u],
                [x + 18.0 * u, y + 16.0 * u],
                col,
                1.0,
                6.0 * u,
                1.6 * u,
            );
        }
        // A gear, drawn as a ring with four teeth.
        LauncherPage::Settings => {
            draw.add_circle([x + 12.0 * u, y + 12.0 * u], 6.5 * u, rgba(col, 1.0))
                .thickness(2.2 * u)
                .build();
            block(10.6, 2.0, 2.8, 4.0);
            block(10.6, 18.0, 2.8, 4.0);
            block(2.0, 10.6, 4.0, 2.8);
            block(18.0, 10.6, 4.0, 2.8);
        }
        // A version: stacked layers.
        LauncherPage::Installations => {
            round(3.0, 4.0, 18.0, 5.0, 2.0);
            round(3.0, 11.0, 18.0, 5.0, 2.0);
            round(3.0, 18.0, 18.0, 3.0, 1.5);
        }
        // A page.
        LauncherPage::PatchNotes => {
            round(5.0, 2.0, 14.0, 20.0, 2.0);
            block(8.0, 7.0, 8.0, 2.0);
            block(8.0, 11.0, 8.0, 2.0);
            block(8.0, 15.0, 5.0, 2.0);
        }
        // A folded paper.
        LauncherPage::News => {
            round(3.0, 4.0, 18.0, 16.0, 2.0);
            block(6.0, 8.0, 8.0, 2.0);
            block(6.0, 12.0, 12.0, 2.0);
        }
        // The account chip's own mark: a head.
        LauncherPage::Profile => {
            draw.add_circle([x + 12.0 * u, y + 9.0 * u], 4.2 * u, rgba(col, 1.0))
                .filled(true)
                .build();
            let r = px(4.0, 15.0, 16.0, 7.0);
            fill(draw, [r[0], r[1]], [r[2], r[3]], col, 1.0, 8.0 * u);
        }
    }
}

/// A player, built the way the game builds one: six boxes with the skin's own UVs,
/// back faces culled, and the overlay layer inflated over the base.
///
/// The parts and their regions are Minecraft's 64x64 layout, so a skin drawn for the
/// game is drawn here exactly as it is in it. The second layer is the hat and the
/// jacket: it is inflated half a pixel and drawn over the part it belongs to, and its
/// own alpha decides what shows, which is what makes a skin with a hood read as a hood
/// rather than as a painted body.
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
    let _ = ui;
    // One model unit is a sixteenth of a block, and the player is 32 units tall from
    // the soles to the crown.
    let u = height / 32.0;
    // A slight downward look, which is how a skin viewer shows a standing player.
    let pitch = 0.10f32;
    let (sy, cy_) = (yaw.sin(), yaw.cos());
    let (sp, cp) = (pitch.sin(), pitch.cos());
    // Model space: x right, y up, z towards the viewer. The origin is between the
    // feet, and `top` is where the crown lands on screen.
    let project = |p: [f32; 3]| -> ([f32; 2], f32) {
        let x1 = p[0] * cy_ + p[2] * sy;
        let z1 = -p[0] * sy + p[2] * cy_;
        let y2 = p[1] * cp - z1 * sp;
        let z2 = p[1] * sp + z1 * cp;
        // A weak perspective: enough for the boxes to read as solid, not enough to
        // bend the silhouette.
        let scale = 1.0 / (1.0 - z2 * 0.006);
        (
            [cx + x1 * u * scale, top + (32.0 - y2) * u * scale],
            z2,
        )
    };
    // Which way a face points, rotated the same way, decides whether it is drawn at
    // all. Without this the far side of every box paints over the near side whenever
    // the depth sort lands within a rounding error of it.
    let facing = |n: [f32; 3]| -> f32 {
        let z1 = -n[0] * sy + n[2] * cy_;
        n[1] * sp + z1 * cp
    };

    /// One box of the model: where it sits, how big it is, and the six UV rects of
    /// its base layer, in the order front, right, back, left, top, bottom.
    struct Part {
        origin: [f32; 3],
        size: [f32; 3],
        base: [[f32; 4]; 6],
        /// The second layer's six rects, or `None` for a skin that has none.
        overlay: Option<[[f32; 4]; 6]>,
    }
    // Front, right, back, left, top, bottom — the order every part below is written
    // in, because it is the order the game's own model uses.
    let parts: [Part; 6] = [
        // Head, 8x8x8, sitting on the shoulders.
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
        // Body, 8x12x4.
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
        // The player's right arm, on the viewer's left: 4x12x4 at x -6.
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
        // The left arm, at x +6. A legacy skin has no second layer and mirrors the
        // right arm's region rather than carrying its own.
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
        // The right leg, 4x12x4 at x -2.
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
        // The left leg, at x +2.
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
    // The shadow the figure stands on: a soft ellipse, so the player is grounded
    // rather than floating in the card.
    {
        let feet = project([0.0, 0.0, 0.0]).0;
        let rx = 7.0 * u;
        let ry = 1.8 * u;
        draw.add_circle([feet[0], feet[1]], rx, rgba(0x000000, 0.30))
            .filled(true)
            .build();
        draw.add_circle([feet[0], feet[1]], rx * 0.62, rgba(0x000000, 0.22))
            .filled(true)
            .build();
        let _ = ry;
    }
    // Every face that is visible, with the depth to paint it at: far first.
    struct Face {
        depth: f32,
        /// The base layer goes down before its own overlay, always.
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
            // The second layer is inflated by half a unit on every side, which is how
            // the game makes a hat sit over a head instead of in it.
            let inflate = if layer == 1 { 0.5 } else { 0.0 };
            let x0 = ox - sx / 2.0 - inflate;
            let x1 = ox + sx / 2.0 + inflate;
            let y0 = oy - inflate;
            let y1 = oy + syz + inflate;
            let z0 = oz - sz / 2.0 - inflate;
            let z1 = oz + sz / 2.0 + inflate;
            // Each face as four corners in screen order — top left, top right,
            // bottom right, bottom left — seen from outside the box. The normals
            // below are in the same order.
            let quads: [[[f32; 3]; 4]; 6] = [
                // Front.
                [
                    [x0, y1, z1],
                    [x1, y1, z1],
                    [x1, y0, z1],
                    [x0, y0, z1],
                ],
                // Right: the player's own right, which is model -x.
                [
                    [x0, y1, z0],
                    [x0, y1, z1],
                    [x0, y0, z1],
                    [x0, y0, z0],
                ],
                // Back.
                [
                    [x1, y1, z0],
                    [x0, y1, z0],
                    [x0, y0, z0],
                    [x1, y0, z0],
                ],
                // Left: model +x.
                [
                    [x1, y1, z1],
                    [x1, y1, z0],
                    [x1, y0, z0],
                    [x1, y0, z1],
                ],
                // Top.
                [
                    [x0, y1, z0],
                    [x1, y1, z0],
                    [x1, y1, z1],
                    [x0, y1, z1],
                ],
                // Bottom.
                [
                    [x0, y0, z1],
                    [x1, y0, z1],
                    [x1, y0, z0],
                    [x0, y0, z0],
                ],
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
                // Light from above and slightly to the front: the top is brightest,
                // the underside darkest, and the sides in between. Without this every
                // face of the box has the same value and the model reads flat.
                let shade = if normal[1] > 0.5 {
                    1.0
                } else if normal[1] < -0.5 {
                    0.62
                } else if normal[0].abs() > 0.5 {
                    0.84
                } else {
                    0.96
                };
                let uv = rects[face_index];
                faces.push(Face {
                    depth,
                    layer,
                    points,
                    uv,
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
        .col(rgba(0xffffff, face.shade))
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
    let w = 420.0 * dpi;
    let h = 210.0 * dpi;
    let x = (width - w) / 2.0;
    let y = (height - h) / 2.0;
    {
        let draw = ui.get_window_draw_list();
        fill(&draw, [x, y], [x + w, y + h], c.panel, 1.0, M3_CARD * 1.6 * dpi);
        let _f = ui.push_font(fonts.title);
        draw.add_text(
            [x + 28.0 * dpi, y + 24.0 * dpi],
            rgba(c.ink, 1.0),
            "Player name",
        );
        let _f = ui.push_font(fonts.small);
        draw.add_text(
            [x + 28.0 * dpi, y + 60.0 * dpi],
            rgba(c.muted, 1.0),
            "This is the name the game shows.",
        );
    }
    let field_y = y + 92.0 * dpi;
    let field_w = w - 56.0 * dpi;
    let focused = field(ui, fonts, c, dpi, x + 28.0 * dpi, field_y, field_w, 46.0 * dpi, &model.draft);
    if focused {
        // The platform owns the keyboard: it sees this flag and raises it.
    }
    let by = y + h - 62.0 * dpi;
    if text_button(
        ui,
        fonts,
        c,
        dpi,
        x + w - 250.0 * dpi,
        by,
        100.0 * dpi,
        44.0 * dpi,
        "Cancel",
        c.ink_soft,
    ) {
        model.editing = false;
        model.draft.clear();
    }
    if filled_button(
        ui,
        fonts,
        c,
        dpi,
        x + w - 140.0 * dpi,
        by,
        112.0 * dpi,
        44.0 * dpi,
        "Save",
        c.accent,
        c.on_accent,
        M3_PILL,
    ) {
        let name = model.draft.trim().to_string();
        if !name.is_empty() {
            sink.push(LauncherEvent::SetAccount(name));
        }
        model.editing = false;
    }
}

/// A text field: a well with a hairline, and the caret at the end of the draft.
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
        fill(&draw, [x, y], [x + w, y + h], c.well, 1.0, M3_CARD_SMALL * dpi);
        stroke(&draw, [x, y], [x + w, y + h], c.accent, 1.0, M3_CARD_SMALL * dpi, 1.5);
        let _f = ui.push_font(fonts.body);
        let shown = fit(ui, text, w - 32.0 * dpi);
        let [_, th] = ui.calc_text_size(shown.as_str());
        draw.add_text(
            [x + 16.0 * dpi, y + (h - th) / 2.0],
            rgba(if text.is_empty() { c.muted } else { c.ink }, 1.0),
            if text.is_empty() { "Player" } else { shown.as_str() },
        );
    }
    ui.set_cursor_screen_pos([x, y]);
    ui.invisible_button("##field", [w, h])
}
/// A page's vertical scroll, for a finger that drags it.
///
/// One number survives between frames — the offset — and the rest is read from the
/// gesture in this frame: where the finger landed, whether it has travelled far
/// enough to be a scroll rather than a tap, and whether it began on a slider.
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
            // A mostly-vertical drag is the page's, even when it began on a row: the
            // rows fill the column, so requiring bare glass would leave nowhere to
            // start a scroll from.
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
    let x = g.x + g.w - 6.0 * dpi;
    let view = scroll.bottom - scroll.top;
    fill(
        &draw,
        [x, scroll.top],
        [x + 3.0 * dpi, scroll.bottom],
        c.well,
        1.0,
        M3_PILL * dpi,
    );
    let thumb = (view / content * view).max(28.0 * dpi).min(view);
    let at = scroll.top + (scroll.offset / scroll.max) * (view - thumb).max(0.0);
    fill(
        &draw,
        [x, at],
        [x + 3.0 * dpi, at + thumb],
        c.bevel,
        1.0,
        M3_PILL * dpi,
    );
}

/// A card: a tonal surface with a title, an optional second line, and a badge.
/// Returns whether it was tapped.
#[allow(clippy::too_many_arguments)]
fn row_card(
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
        // A row is a card at rest and a raised card under a finger. A selected row is
        // raised whether or not it is hovered, which is the state the accent bar marks.
        let face = if selected || hovered { c.raised } else { c.panel };
        panel(&draw, [x, y], [x + w, y + h], face, M3_CARD * dpi);
        if selected {
            // A selected row is marked by its tone and a bar on the leading edge,
            // which is the one place a Material 3 list puts an accent.
            fill(
                &draw,
                [x, y + h * 0.22],
                [x + 3.0 * dpi, y + h * 0.78],
                c.accent,
                1.0,
                M3_PILL * dpi,
            );
        }
        {
            let _f = ui.push_font(fonts.label);
            draw.add_text(
                [x + 18.0 * dpi, y + 12.0 * dpi],
                rgba(c.ink, 1.0),
                fit(ui, title, w - 36.0 * dpi - 80.0 * dpi),
            );
        }
        if !subtitle.is_empty() {
            let _f = ui.push_font(fonts.small);
            draw.add_text(
                [x + 18.0 * dpi, y + 34.0 * dpi],
                rgba(c.muted, 1.0),
                fit(ui, subtitle, w - 36.0 * dpi - 80.0 * dpi),
            );
        }
        if !badge.is_empty() {
            let _f = ui.push_font(fonts.small);
            let [bw, _] = ui.calc_text_size(badge);
            draw.add_text(
                [x + w - 18.0 * dpi - bw, y + 13.0 * dpi],
                rgba(if selected { c.accent } else { c.muted }, 1.0),
                badge,
            );
        }
    }
    clicked
}

/// A heading inside a page, above a run of cards.
fn section(ui: &Ui, fonts: &Fonts, c: &Css, dpi: f32, x: f32, y: f32, text: &str) -> f32 {
    let draw = ui.get_window_draw_list();
    let _f = ui.push_font(fonts.small);
    draw.add_text([x, y], rgba(c.muted, 1.0), text);
    y + 22.0 * dpi
}

/// Versions: the one build this launcher runs, and the way to start another.
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
    let x = g.x + g.pad;
    let w = g.w - g.pad * 2.0;
    let mut y = top;
    let row_h = 62.0 * dpi;
    for row in &model.rows {
        if y + row_h > height {
            break;
        }
        if row_card(
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
        y += row_h + 8.0 * dpi;
    }
    if model.rows.is_empty() {
        let draw = ui.get_window_draw_list();
        let _f = ui.push_font(fonts.body);
        draw.add_text([x, y], rgba(c.muted, 1.0), "No build is installed yet.");
        y += 30.0 * dpi;
    }
    y += 8.0 * dpi;
    if y + 46.0 * dpi <= height {
        if outlined_button(
            ui,
            fonts,
            c,
            dpi,
            x,
            y,
            170.0 * dpi,
            44.0 * dpi,
            "New instance",
            c.ink_soft,
        ) {
            sink.push(LauncherEvent::NewInstance);
        }
    }
}

/// News and Patch Notes are the same page with different words: a column of cards.
#[allow(clippy::too_many_arguments)]
fn stories_page(
    ui: &Ui,
    stories: &[LauncherStory],
    fonts: &Fonts,
    c: &Css,
    dpi: f32,
    g: &Grid,
    height: f32,
    top: f32,
    _title: &str,
) {
    let x = g.x + g.pad;
    let w = g.w - g.pad * 2.0;
    let card_w = (w - 16.0 * dpi) / 2.0;
    let mut col = 0;
    let mut row_top = top;
    let mut row_h = 0.0f32;
    for story in stories {
        let body = wrap(ui, &story.body, card_w - 36.0 * dpi);
        let h = 74.0 * dpi + body.len() as f32 * 18.0 * dpi;
        if col == 0 && row_top + h > height {
            break;
        }
        let cx = x + col as f32 * (card_w + 16.0 * dpi);
        {
            let draw = ui.get_window_draw_list();
            panel(&draw, [cx, row_top], [cx + card_w, row_top + h], c.panel, M3_CARD * dpi);
            {
                let _f = ui.push_font(fonts.label);
                draw.add_text(
                    [cx + 18.0 * dpi, row_top + 16.0 * dpi],
                    rgba(c.ink, 1.0),
                    fit(ui, &story.title, card_w - 36.0 * dpi),
                );
            }
            if !story.tag.is_empty() {
                let _f = ui.push_font(fonts.small);
                draw.add_text(
                    [cx + 18.0 * dpi, row_top + 40.0 * dpi],
                    rgba(c.accent, 1.0),
                    fit(ui, &story.tag, card_w - 36.0 * dpi),
                );
            }
            let _f = ui.push_font(fonts.small);
            let mut ty = row_top + 62.0 * dpi;
            for line in &body {
                draw.add_text([cx + 18.0 * dpi, ty], rgba(c.ink_soft, 1.0), line.as_str());
                ty += 18.0 * dpi;
            }
        }
        row_h = row_h.max(h);
        col += 1;
        if col == 2 {
            col = 0;
            row_top += row_h + 16.0 * dpi;
            row_h = 0.0;
        }
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
    let x = g.x + g.pad;
    let avail_h = (height - top - g.pad).max(120.0 * dpi);
    let side_by_side = g.w > 760.0 * dpi;
    let model_w = if side_by_side {
        (avail_h * 0.62).clamp(180.0 * dpi, 300.0 * dpi)
    } else {
        g.w - g.pad * 2.0
    };
    // The model's card, with the figure standing in it.
    {
        let draw = ui.get_window_draw_list();
        panel(&draw, [x, top], [x + model_w, top + avail_h], c.panel, M3_CARD * dpi);
    }
    let figure_top = top + 24.0 * dpi;
    let figure_h = (avail_h - 76.0 * dpi).max(60.0 * dpi);
    match skin {
        Some(texture) => skin_figure(
            ui,
            &ui.get_window_draw_list(),
            texture,
            model.skin_size,
            x + model_w / 2.0,
            figure_top,
            figure_h,
            model.skin_yaw,
        ),
        None => {
            let draw = ui.get_window_draw_list();
            let _f = ui.push_font(fonts.small);
            let text = "Loading the player...";
            let [tw, _] = ui.calc_text_size(text);
            draw.add_text(
                [x + (model_w - tw) / 2.0, top + avail_h / 2.0],
                rgba(c.muted, 1.0),
                text,
            );
        }
    }
    {
        let draw = ui.get_window_draw_list();
        let _f = ui.push_font(fonts.small);
        let name = model.chosen_skin.as_deref().unwrap_or("Default skin");
        let [nw, _] = ui.calc_text_size(name);
        draw.add_text(
            [x + (model_w - nw) / 2.0, top + avail_h - 34.0 * dpi],
            rgba(c.ink_soft, 1.0),
            fit(ui, name, model_w - 24.0 * dpi),
        );
    }

    let rx = if side_by_side {
        x + model_w + 16.0 * dpi
    } else {
        x
    };
    let rw = if side_by_side {
        g.x + g.w - g.pad - rx
    } else {
        g.w - g.pad * 2.0
    };
    let ry = if side_by_side {
        top
    } else {
        top + avail_h + 16.0 * dpi
    };
    if ry + 44.0 * dpi <= height {
        if filled_button(
            ui,
            fonts,
            c,
            dpi,
            rx,
            ry,
            rw.min(260.0 * dpi),
            44.0 * dpi,
            "Choose a skin",
            c.accent,
            c.on_accent,
            M3_PILL,
        ) {
            sink.push(LauncherEvent::OpenSkins);
        }
    }
    let mut y = ry + 58.0 * dpi;
    y = section(ui, fonts, c, dpi, rx, y, "YOUR SKINS");
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
                    [rx, y + i as f32 * 18.0 * dpi],
                    rgba(c.muted, 1.0),
                    line.as_str(),
                );
            }
        }
    } else {
        for name in &model.skins {
            if y + 44.0 * dpi > height {
                break;
            }
            let chosen = model.chosen_skin.as_deref() == Some(name.as_str());
            if row_card(ui, fonts, c, dpi, rx, y, rw, 44.0 * dpi, name, "", if chosen { "worn" } else { "" }, chosen) {
                sink.push(LauncherEvent::PickSkin(name.clone()));
            }
            y += 44.0 * dpi + 8.0 * dpi;
        }
    }
}

/// Mods: what Ember can add, and what is already installed.
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
    let x = g.x + g.pad;
    let w = g.w - g.pad * 2.0;
    let two = g.w > 820.0 * dpi;
    let col_w = if two { (w - 16.0 * dpi) / 2.0 } else { w };

    // The left column: the registry.
    let mut y = top;
    {
        let draw = ui.get_window_draw_list();
        let _f = ui.push_font(fonts.body);
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
    for m in &model.mods {
        if y + 74.0 * dpi > height {
            break;
        }
        let installed = model.installed_mods.iter().any(|f| f.starts_with(&m.id));
        let busy = model.installing.as_deref() == Some(m.id.as_str());
        {
            let draw = ui.get_window_draw_list();
            panel(&draw, [x, y], [x + col_w, y + 66.0 * dpi], c.panel, M3_CARD * dpi);
            {
                let _f = ui.push_font(fonts.label);
                draw.add_text(
                    [x + 18.0 * dpi, y + 12.0 * dpi],
                    rgba(c.ink, 1.0),
                    fit(ui, &m.name, col_w - 150.0 * dpi),
                );
            }
            {
                let _f = ui.push_font(fonts.small);
                draw.add_text(
                    [x + 18.0 * dpi, y + 34.0 * dpi],
                    rgba(c.muted, 1.0),
                    fit(ui, &m.summary, col_w - 150.0 * dpi),
                );
                draw.add_text(
                    [x + 18.0 * dpi, y + 50.0 * dpi],
                    rgba(c.muted, 0.8),
                    format!("{} · {}", m.id, m.version),
                );
            }
        }
        let bw = 96.0 * dpi;
        let bx = x + col_w - bw - 14.0 * dpi;
        let by = y + 14.0 * dpi;
        if installed {
            text_button(ui, fonts, c, dpi, bx, by, bw, 38.0 * dpi, "Installed", c.muted);
        } else if busy {
            text_button(ui, fonts, c, dpi, bx, by, bw, 38.0 * dpi, "Installing", c.accent);
        } else if filled_button(
            ui,
            fonts,
            c,
            dpi,
            bx,
            by,
            bw,
            38.0 * dpi,
            "Install",
            c.accent,
            c.on_accent,
            M3_PILL,
        ) {
            sink.push(LauncherEvent::InstallMod(m.id.clone()));
        }
        y += 74.0 * dpi;
    }
    if model.mods.is_empty() && !model.mods_loading {
        let draw = ui.get_window_draw_list();
        let _f = ui.push_font(fonts.small);
        draw.add_text(
            [x, y],
            rgba(c.muted, 1.0),
            "No mods are listed. Pull down to refresh, or drop an .hmod in import/mods.",
        );
        y += 24.0 * dpi;
    }
    y += 6.0 * dpi;
    if y + 40.0 * dpi <= height {
        if outlined_button(ui, fonts, c, dpi, x, y, 120.0 * dpi, 38.0 * dpi, "Refresh", c.ink_soft) {
            sink.push(LauncherEvent::RefreshMods);
        }
    }

    // The right column: what is installed, with a way to take it out.
    if two {
        let ix = x + col_w + 16.0 * dpi;
        let mut iy = section(ui, fonts, c, dpi, ix, top, "INSTALLED");
        if model.installed_mods.is_empty() {
            let draw = ui.get_window_draw_list();
            let _f = ui.push_font(fonts.small);
            draw.add_text([ix, iy], rgba(c.muted, 1.0), "Nothing installed yet.");
        }
        for name in &model.installed_mods {
            if iy + 46.0 * dpi > height {
                break;
            }
            {
                let draw = ui.get_window_draw_list();
                panel(&draw, [ix, iy], [ix + col_w, iy + 40.0 * dpi], c.panel, M3_CARD * dpi);
                let _f = ui.push_font(fonts.small);
                draw.add_text(
                    [ix + 16.0 * dpi, iy + 12.0 * dpi],
                    rgba(c.ink_soft, 1.0),
                    fit(ui, name, col_w - 110.0 * dpi),
                );
            }
            if text_button(
                ui,
                fonts,
                c,
                dpi,
                ix + col_w - 92.0 * dpi,
                iy,
                84.0 * dpi,
                40.0 * dpi,
                "Remove",
                c.danger,
            ) {
                sink.push(LauncherEvent::UninstallMod(name.clone()));
            }
            iy += 48.0 * dpi;
        }
    }
}

/// Files: what the game reads, and the way in and out.
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
    let x = g.x + g.pad;
    let w = g.w - g.pad * 2.0;
    let cols = if w > 620.0 * dpi { 3 } else { 2 };
    let gap = 12.0 * dpi;
    let cw = (w - gap * (cols as f32 - 1.0)) / cols as f32;
    let ch = 76.0 * dpi;
    let mut y = top;
    {
        let draw = ui.get_window_draw_list();
        let _f = ui.push_font(fonts.small);
        for (i, line) in wrap(
            ui,
            &format!(
                "Import copies a file from your Downloads into the game's folder. Everything here is also at {}",
                model.instance
            ),
            w,
        )
        .iter()
        .enumerate()
        {
            draw.add_text(
                [x, y + i as f32 * 18.0 * dpi],
                rgba(c.muted, 1.0),
                line.as_str(),
            );
        }
    }
    y += 34.0 * dpi;

    // The four imports and the three exports, as cards in a grid.
    let items: [(&str, &str, LauncherEvent, bool); 7] = [
        ("Skin", "64x64 PNG", LauncherEvent::ImportFiles("skin".into()), true),
        ("Mod", ".hmod file", LauncherEvent::ImportFiles("mod".into()), true),
        ("Pack", ".zip pack", LauncherEvent::ImportFiles("resourcepack".into()), true),
        ("Settings", "a saved set", LauncherEvent::ImportFiles("settings".into()), true),
        ("Export skins", "to Downloads", LauncherEvent::ExportFiles("skins".into()), false),
        ("Export mods", "to Downloads", LauncherEvent::ExportFiles("mods".into()), false),
        ("Export settings", "to Downloads", LauncherEvent::ExportSettings, false),
    ];
    let item_count = items.len();
    for (i, (title, sub, event, is_import)) in items.into_iter().enumerate() {
        let col = i % cols;
        let row = i / cols;
        let cx = x + col as f32 * (cw + gap);
        let cy = y + row as f32 * (ch + gap);
        if cy + ch > height {
            break;
        }
        ui.set_cursor_screen_pos([cx, cy]);
        let id = format!("##file{i}");
        let clicked = ui.invisible_button(id.as_str(), [cw, ch]);
        let hovered = ui.is_item_hovered();
        {
            let draw = ui.get_window_draw_list();
            panel(&draw, [cx, cy], [cx + cw, cy + ch], if hovered { c.raised } else { c.panel }, M3_CARD * dpi);
            // A small mark: an arrow into a tray for an import, out of one for an
            // export. It is the one thing that says which direction a card goes.
            let mx = cx + 18.0 * dpi;
            let my = cy + 26.0 * dpi;
            fill(&draw, [mx, my], [mx + 16.0 * dpi, my + 2.0 * dpi], c.accent, 1.0, 1.0);
            fill(
                &draw,
                [mx + 7.0 * dpi, my - (if is_import { 8.0 } else { -8.0 }) * dpi],
                [mx + 9.0 * dpi, my + (if is_import { 0.0 } else { 0.0 }) * dpi],
                c.accent,
                1.0,
                1.0,
            );
            {
                let _f = ui.push_font(fonts.label);
                draw.add_text(
                    [cx + 44.0 * dpi, cy + 16.0 * dpi],
                    rgba(c.ink, 1.0),
                    fit(ui, title, cw - 56.0 * dpi),
                );
            }
            {
                let _f = ui.push_font(fonts.small);
                draw.add_text(
                    [cx + 44.0 * dpi, cy + 40.0 * dpi],
                    rgba(c.muted, 1.0),
                    fit(ui, sub, cw - 56.0 * dpi),
                );
            }
        }
        if clicked {
            sink.push(event);
        }
    }
    let rows_used = (item_count as f32 / cols as f32).ceil();
    let by = y + rows_used * (ch + gap) + 6.0 * dpi;
    if by + 44.0 * dpi <= height {
        if outlined_button(
            ui,
            fonts,
            c,
            dpi,
            x,
            by,
            190.0 * dpi,
            42.0 * dpi,
            "Open the folder",
            c.ink_soft,
        ) {
            sink.push(LauncherEvent::RevealFiles);
        }
    }
}

/// Hosting: the address friends join on, and the one button that starts it.
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
    let x = g.x + g.pad;
    let w = g.w - g.pad * 2.0;
    let card_h = 132.0 * dpi;
    {
        let draw = ui.get_window_draw_list();
        panel(&draw, [x, top], [x + w, top + card_h], c.panel, M3_CARD * dpi);
        {
            let _f = ui.push_font(fonts.label);
            draw.add_text([x + 20.0 * dpi, top + 18.0 * dpi], rgba(c.ink, 1.0), "This device");
        }
        {
            let _f = ui.push_font(fonts.display);
            let text = model.hosting.as_deref().unwrap_or("Not hosting");
            draw.add_text(
                [x + 20.0 * dpi, top + 44.0 * dpi],
                rgba(if model.hosting.is_some() { c.accent } else { c.muted }, 1.0),
                fit(ui, text, w - 40.0 * dpi),
            );
        }
        let _f = ui.push_font(fonts.small);
        draw.add_text(
            [x + 20.0 * dpi, top + 96.0 * dpi],
            rgba(c.muted, 1.0),
            fit(
                ui,
                if model.hosting.is_some() {
                    "Friends on this network can join at that address."
                } else {
                    "Open a world first, then host it. It stays on this device."
                },
                w - 40.0 * dpi,
            ),
        );
    }
    let mut y = top + card_h + 16.0 * dpi;
    if model.hosting.is_some() {
        if filled_button(ui, fonts, c, dpi, x, y, 170.0 * dpi, 46.0 * dpi, "Stop hosting", c.button, c.ink, M3_PILL) {
            sink.push(LauncherEvent::StopHosting);
        }
    } else if filled_button(ui, fonts, c, dpi, x, y, 170.0 * dpi, 46.0 * dpi, "Host", c.accent, c.on_accent, M3_PILL) {
        sink.push(LauncherEvent::HostWorld(0));
    }
    y += 62.0 * dpi;
    y = section(ui, fonts, c, dpi, x, y, "WORLDS ON THIS DEVICE");
    let mut shown = 0;
    for row in &model.rows {
        if y + 62.0 * dpi > height {
            break;
        }
        if row_card(ui, fonts, c, dpi, x, y, w, 56.0 * dpi, &row.title, &row.subtitle, &row.badge, false) {
            sink.push(LauncherEvent::HostWorld(row.id));
        }
        y += 64.0 * dpi;
        shown += 1;
    }
    if shown == 0 {
        let draw = ui.get_window_draw_list();
        let _f = ui.push_font(fonts.small);
        draw.add_text([x, y], rgba(c.muted, 1.0), "No worlds yet. Play once to make one.");
    }
}

/// Accounts: who is playing, and how to sign in.
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
    let x = g.x + g.pad;
    let w = g.w - g.pad * 2.0;
    let mut y = section(ui, fonts, c, dpi, x, top, "ACCOUNTS");
    for account in &model.accounts {
        if y + 62.0 * dpi > height {
            break;
        }
        let badge = if account.selected { "playing" } else { "" };
        if row_card(
            ui,
            fonts,
            c,
            dpi,
            x,
            y,
            w - 92.0 * dpi,
            56.0 * dpi,
            &account.name,
            &account.kind,
            badge,
            account.selected,
        ) {
            sink.push(LauncherEvent::SwitchAccount(
                model
                    .accounts
                    .iter()
                    .position(|a| a.name == account.name)
                    .unwrap_or(0) as u32,
            ));
        }
        if text_button(
            ui,
            fonts,
            c,
            dpi,
            x + w - 84.0 * dpi,
            y,
            84.0 * dpi,
            56.0 * dpi,
            "Remove",
            c.muted,
        ) {
            sink.push(LauncherEvent::RemoveAccount(
                model
                    .accounts
                    .iter()
                    .position(|a| a.name == account.name)
                    .unwrap_or(0) as u32,
            ));
        }
        y += 64.0 * dpi;
    }

    y += 6.0 * dpi;
    if y + 46.0 * dpi <= height {
        let half = (w - 12.0 * dpi) / 2.0;
        if model.sign_in_waiting {
            if outlined_button(
                ui,
                fonts,
                c,
                dpi,
                x,
                y,
                half,
                44.0 * dpi,
                "Cancel sign-in",
                c.danger,
            ) {
                sink.push(LauncherEvent::CancelSignIn);
            }
        } else if filled_button(
            ui,
            fonts,
            c,
            dpi,
            x,
            y,
            half,
            44.0 * dpi,
            "Sign in with Microsoft",
            c.accent,
            c.on_accent,
            M3_PILL,
        ) {
            sink.push(LauncherEvent::SignInMicrosoft);
        }
        if outlined_button(
            ui,
            fonts,
            c,
            dpi,
            x + half + 12.0 * dpi,
            y,
            half,
            44.0 * dpi,
            "Add offline account",
            c.ink_soft,
        ) {
            sink.push(LauncherEvent::AddAccount);
        }
        y += 58.0 * dpi;
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

    if y + 30.0 * dpi <= height {
        y = section(ui, fonts, c, dpi, x, y, "TOUCH CONTROLS");
        let java = model.controls_preset != "bedrock";
        let mut cx = x;
        if chip(ui, fonts, c, dpi, cx, y, "Java classic", java) && !java {
            sink.push(LauncherEvent::SetControls("java".into()));
        }
        cx += chip_width(ui, fonts, dpi, "Java classic") + 10.0 * dpi;
        if chip(ui, fonts, c, dpi, cx, y, "Bedrock", !java) && java {
            sink.push(LauncherEvent::SetControls("bedrock".into()));
        }
        y += 44.0 * dpi;
    }

    // The facts: the build, the engine, the platform.
    if y + 20.0 * dpi <= height {
        y = section(ui, fonts, c, dpi, x, y, "ABOUT");
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
                let _f = ui.push_font(fonts.small);
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

/// Settings: the launcher's own controls first, then the game's.
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
    let x = g.x + g.pad;
    let w = g.w - g.pad * 2.0;
    let row_h = 58.0 * dpi;
    let gap = 8.0 * dpi;
    let group_h = 34.0 * dpi;

    // The content's height, so the offset can be clamped to what there is to see.
    let mut content = 0.0;
    let mut last_group = "";
    for setting in &model.settings {
        if !setting.group.is_empty() && setting.group.as_str() != last_group {
            content += group_h;
            last_group = setting.group.as_str();
        }
        content += row_h + gap;
    }
    let bottom = (height - 12.0 * dpi).max(top);
    let scroll = scroll_begin(ui, g, top, bottom, content, dpi);
    let swallowed = scroll.swallowed;

    {
        let draw = ui.get_window_draw_list();
        draw.with_clip_rect_intersect([g.x, top], [g.x + g.w, bottom], || {
            let mut y = top - scroll.offset;
            let mut last_group = "";
            for setting in &model.settings {
                if !setting.group.is_empty() && setting.group.as_str() != last_group {
                    if y + group_h > top && y < bottom {
                        let _f = ui.push_font(fonts.small);
                        draw.add_text(
                            [x, y + 8.0 * dpi],
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
                if y + row_h <= top || y >= bottom {
                    y += row_h + gap;
                    continue;
                }
                let hit_y = y.max(top);
                let hit_h = ((y + row_h).min(bottom) - hit_y).max(0.0);
                panel(&draw, [x, y], [x + w, y + row_h], c.panel, M3_CARD * dpi);
                {
                    let _f = ui.push_font(fonts.body);
                    draw.add_text(
                        [x + 18.0 * dpi, y + 10.0 * dpi],
                        rgba(c.ink_soft, 1.0),
                        fit(ui, &setting.label, w * 0.60),
                    );
                }
                if setting.toggle {
                    let tw = 48.0 * dpi;
                    let th = 26.0 * dpi;
                    let tx = x + w - 18.0 * dpi - tw;
                    let ty = y + (row_h - th) / 2.0;
                    fill(
                        &draw,
                        [tx, ty],
                        [tx + tw, ty + th],
                        if setting.on { c.accent } else { c.button },
                        1.0,
                        th / 2.0,
                    );
                    let knob = th - 8.0 * dpi;
                    let kx = if setting.on {
                        tx + tw - knob - 4.0 * dpi
                    } else {
                        tx + 4.0 * dpi
                    };
                    fill(
                        &draw,
                        [kx, ty + 4.0 * dpi],
                        [kx + knob, ty + 4.0 * dpi + knob],
                        if setting.on { c.on_accent } else { c.ink_soft },
                        1.0,
                        knob / 2.0,
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
                    let track_x = x + 18.0 * dpi + low_w + 10.0 * dpi;
                    let track_w =
                        (x + w - 18.0 * dpi - high_w - 10.0 * dpi - track_x).max(60.0 * dpi);
                    let ty = y + 36.0 * dpi;
                    let span = (setting.max - setting.min).max(f32::EPSILON);
                    let t = ((setting.value - setting.min) / span).clamp(0.0, 1.0);
                    fill(
                        &draw,
                        [track_x, ty],
                        [track_x + track_w, ty + 4.0 * dpi],
                        c.button,
                        1.0,
                        M3_PILL * dpi,
                    );
                    if t > 0.0 {
                        fill(
                            &draw,
                            [track_x, ty],
                            [track_x + track_w * t, ty + 4.0 * dpi],
                            c.accent,
                            1.0,
                            M3_PILL * dpi,
                        );
                    }
                    draw.add_circle(
                        [track_x + track_w * t, ty + 2.0 * dpi],
                        7.0 * dpi,
                        rgba(c.ink, 1.0),
                    )
                    .filled(true)
                    .build();
                    {
                        let _f = ui.push_font(fonts.small);
                        let text = setting_text(setting, setting.value);
                        let [tw, _] = ui.calc_text_size(text.as_str());
                        draw.add_text(
                            [x + w - 18.0 * dpi - tw, y + 10.0 * dpi],
                            rgba(c.ink, 1.0),
                            text.as_str(),
                        );
                        draw.add_text(
                            [x + 18.0 * dpi, ty - 6.0 * dpi],
                            rgba(c.muted, 0.9),
                            low.as_str(),
                        );
                        draw.add_text(
                            [x + w - 18.0 * dpi - high_w, ty - 6.0 * dpi],
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
                    [x, top + 8.0 * dpi],
                    rgba(c.muted, 1.0),
                    "The game's options appear once the pack is installed.",
                );
            }
        });
    }
    scroll_end(ui, g, c, dpi, &scroll, content);
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

/// The loading surface: the launcher's own background under a scrim, the mark, a
/// spinner, and the one line that says what is happening.
///
/// It is drawn instead of the launcher, not over it, so the page behind it cannot be
/// tapped while the game is coming up. The spinner is a ring with an arc that turns:
/// no dots, no steps, nothing that implies a percentage it does not have.
pub fn draw_loading(
    ui: &Ui,
    screen: &LoadingScreen,
    fonts: &Fonts,
    dpi: f32,
    width: f32,
    height: f32,
    background: Option<(TextureId, [u32; 2], f32)>,
    elapsed: f32,
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
                fill(&draw, [0.0, 0.0], [width, height], c.page, 1.0, 0.0);
                if let Some((texture, size, frame)) = background {
                    artwork(&draw, texture, size, frame, width, height);
                }
                // The scrim: dark enough for a word over it, light enough that the
                // artwork still reads as the thing behind the game.
                fill(&draw, [0.0, 0.0], [width, height], c.page, 0.72, 0.0);
            }
            let cx = width / 2.0;
            let cy = height / 2.0;

            // The spinner: a track and an arc that turns once a second.
            let radius = 26.0 * dpi;
            let ring_y = cy - 46.0 * dpi;
            {
                let draw = ui.get_window_draw_list();
                draw.add_circle([cx, ring_y], radius, rgba(c.ink, 0.14))
                    .thickness(3.0 * dpi)
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
                        draw.add_line(from, p, rgba(c.accent, 1.0))
                            .thickness(3.0 * dpi)
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
                draw.add_text(
                    [cx - tw / 2.0, cy + 6.0 * dpi],
                    rgba(c.ink, 1.0),
                    title,
                );
                let _f = ui.push_font(fonts.small);
                let detail = if screen.detail.is_empty() {
                    "This takes a moment on a phone."
                } else {
                    screen.detail.as_str()
                };
                let [dw, _] = ui.calc_text_size(detail);
                draw.add_text(
                    [cx - dw / 2.0, cy + 6.0 * dpi + th + 10.0 * dpi],
                    rgba(c.muted, 1.0),
                    detail,
                );
                let _ = dw;
            }
            // The bar, only when there is a real fraction to show.
            if let Some(p) = screen.progress {
                let p = p.clamp(0.0, 1.0);
                let bw = 240.0 * dpi;
                let bx = cx - bw / 2.0;
                let by = cy + 78.0 * dpi;
                let draw = ui.get_window_draw_list();
                fill(&draw, [bx, by], [bx + bw, by + 4.0 * dpi], c.button, 1.0, M3_PILL * dpi);
                if p > 0.0 {
                    fill(
                        &draw,
                        [bx, by],
                        [bx + bw * p, by + 4.0 * dpi],
                        c.accent,
                        1.0,
                        M3_PILL * dpi,
                    );
                }
            }
        });
}
