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

use imgui::{Condition, DrawListMut, FontConfig, FontId, FontSource, StyleColor, Ui, WindowFlags};

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
    /// The `.hmod` files already in `import/mods/`, by name.
    pub installed_mods: Vec<String>,
    /// Where the mod registry's website is, so the page can offer it.
    pub mods_site: String,
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
    /// Open the engine's server list.
    OpenMultiplayer,
    /// A skin file from `import/skins/` was chosen.
    PickSkin(String),
    /// Write the settings archive into import/.
    ExportSettings,
    /// Read the newest settings archive from import/.
    ImportSettings,
    Quit,
    /// A slider moved: `options.txt` key and its new value.
    SetOption(String, f32),
    /// A switch flipped.
    ToggleOption(String, bool),
    /// Install this mod from the registry.
    InstallMod(String),
    /// Fetch the registry again.
    RefreshMods,
    /// Open the mod registry's site in a browser.
    OpenModsSite,
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

/// The one geometry every element is placed from: the sidebar's edge is where the
/// page begins, and the page's own gutter is the same number everywhere.
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
/// trick behind a button that looks pressable on a flat screen.
fn raised_face(
    draw: &DrawListMut<'_>,
    min: [f32; 2],
    max: [f32; 2],
    face: u32,
    bevel: u32,
    lip: u32,
    radius: f32,
    dpi: f32,
) {
    fill(draw, min, max, face, 1.0, radius);
    // The bevel is the top two pixels; the lip is the bottom three.
    fill(
        draw,
        [min[0] + radius * 0.5, min[1]],
        [max[0] - radius * 0.5, min[1] + 2.0 * dpi],
        bevel,
        1.0,
        0.0,
    );
    fill(
        draw,
        [min[0] + radius * 0.5, max[1] - 3.0 * dpi],
        [max[0] - radius * 0.5, max[1]],
        lip,
        1.0,
        0.0,
    );
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
/// `None` once spent. `panorama` says the engine has already drawn the title's cube
/// map behind this window, so the front page must leave that half of the glass alone.
/// Navigation is applied to `model` in place, so the destination that was pressed and
/// the page it switched to are drawn in the same pass.
#[allow(clippy::too_many_arguments)]
pub fn draw(
    ui: &Ui,
    model: &mut LauncherModel,
    fonts: &Fonts,
    dpi: f32,
    width: f32,
    height: f32,
    opening: Option<f32>,
    panorama: bool,
    out: &mut Vec<LauncherEvent>,
) {
    let c = &THEME;
    // The opening owns the glass until it is spent. Its last frame is the first one
    // where a finger is allowed to land on anything.
    let live = !matches!(opening, Some(t) if t < 1.0);
    let mut sink = Sink { events: out, live };
    let g = Grid::new(width, dpi);
    // The interface arrives by rising a little and fading in; the rise is one number
    // the whole layout is offset by, so the column and the page move together.
    let rise = match opening {
        Some(t) => horizon_theme::RISE * (1.0 - horizon_theme::ease_out_cubic(t)) * dpi,
        None => 0.0,
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
                splash(ui, fonts, c, dpi, width, height);
                return;
            }
            // The column is drawn first because it is what applies a tab tap to the
            // model. Reading `model.page` before it would decide the background from
            // the page that was up a frame ago, which is how a page ended up drawn
            // over the panorama with nothing behind it.
            sidebar(ui, model, fonts, c, dpi, width, height, rise, &mut sink);
            // The Play page is the title panorama with things laid over it, so the
            // launcher must not paint over the glass there: the engine's cube map is
            // already drawn behind this window. Every other page is opaque. When there
            // is no panorama behind it — the bring-up card — the page paints itself.
            let over_panorama = panorama && model.page == LauncherPage::Play;
            if !over_panorama {
                let draw = ui.get_window_draw_list();
                // Only the page's own region: the column has already painted itself,
                // and covering it here would undo the selection highlight.
                fill(&draw, [g.x, 0.0], [width, height], c.page, 1.0, 0.0);
            }
            match model.page {
                LauncherPage::Play => play_page(
                    ui,
                    model,
                    fonts,
                    c,
                    dpi,
                    &g,
                    height,
                    rise,
                    over_panorama,
                    &mut sink,
                ),
                LauncherPage::Installations => {
                    installations_page(ui, model, fonts, c, dpi, &g, height, rise, &mut sink)
                }
                LauncherPage::Skins => {
                    skins_page(ui, model, fonts, c, dpi, &g, height, rise, &mut sink)
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
                        &g,
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
                    &g,
                    height,
                    rise,
                ),
                LauncherPage::Mods => {
                    mods_page(ui, model, fonts, c, dpi, &g, height, rise, &mut sink)
                }
                LauncherPage::Profile => {
                    profile_page(ui, model, fonts, c, dpi, &g, height, rise, &mut sink)
                }
                LauncherPage::Settings => {
                    settings_page(ui, model, fonts, c, dpi, &g, height, rise, &mut sink)
                }
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
        {
            let draw = ui.get_window_draw_list();
            if selected {
                // The reference's selection is a green outline with a dark face, not a
                // filled green bar: the label stays white and the block is the mark.
                fill(&draw, [x, y], [x + w, y + row_h], c.raised, 1.0, 3.0 * dpi);
                stroke(&draw, [x, y], [x + w, y + row_h], c.accent, 1.0, 3.0 * dpi, 2.0);
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
        if ui.invisible_button(id.as_str(), [w, row_h]) {
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
        // page's own colour at the same weight. Stacked bands are the gradient —
        // imgui's vertex gradient is one more API to get wrong for a wash this soft.
        let bands = 14;
        let top = height * 0.30;
        let span = (height + rise - top) / bands as f32;
        for i in 0..bands {
            let t = i as f32 / (bands - 1) as f32;
            let a = if over_panorama {
                0.10 + 0.78 * t * t
            } else {
                0.45 + 0.55 * t
            };
            fill(
                &draw,
                [g.x, top + i as f32 * span],
                [g.x + g.w, top + (i as f32 + 1.0) * span + 1.0],
                c.page,
                a,
                0.0,
            );
        }
        if !over_panorama {
            fill(&draw, [g.x, 0.0], [g.x + g.w, top], c.page, 0.45, 0.0);
        }
        // And a soft edge under the column, so the seam is a transition and not a cut.
        for i in 0..10 {
            let t = i as f32 / 9.0;
            fill(
                &draw,
                [g.x + t * 90.0 * dpi, 0.0],
                [g.x + (t + 0.1) * 90.0 * dpi, height + rise],
                c.page,
                0.35 * (1.0 - t),
                0.0,
            );
        }
    }

    // Top right: the two things a player reaches for besides Play. They are pinned to
    // the page's top-right corner in the reference, not to the content column, which
    // is why they sit above the art rather than beside the title.
    let bw = 108.0 * dpi;
    let bh = 32.0 * dpi;
    let by = 12.0 * dpi + rise;
    let mut bx = g.x + g.w - g.pad - bw;
    if raised_button(ui, fonts, c, dpi, bx, by, bw, bh, "MULTIPLAYER", c.accent, c.accent_lip, c.on_accent) {
        // Servers live in the engine's own multiplayer screen, so this is the one
        // button on the page that hands over to the game.
        sink.push(LauncherEvent::OpenMultiplayer);
    }
    bx -= bw + 8.0 * dpi;
    if raised_button(ui, fonts, c, dpi, bx, by, bw, bh, "FILES", c.button, c.bevel, c.ink) {
        sink.push(LauncherEvent::Goto(LauncherPage::Installations));
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
    let draw = ui.get_window_draw_list();
    raised_face(&draw, [x, y], [x + w, y + h], face, c.bevel, lip, 4.0 * dpi, dpi);
    {
        let _f = ui.push_font(fonts.pixel);
        let [tw, th] = ui.calc_text_size(label);
        draw.add_text(
            [x + (w - tw) / 2.0, y + (h - th) / 2.0 - 1.0 * dpi],
            rgba(ink, 1.0),
            label,
        );
    }
    ui.set_cursor_screen_pos([x, y]);
    ui.invisible_button(label, [w, h])
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
    bx += btn_w + 36.0 * dpi;
    // A search field, which is a well rather than a widget: the phone's keyboard
    // commits words to the activity, so this is drawn and the typing is the
    // platform's, exactly like the name field.
    let sw = (g.x + g.w - g.pad - bx).max(80.0 * dpi);
    {
        let draw = ui.get_window_draw_list();
        fill(&draw, [bx, y], [bx + sw, y + btn_h], c.well, 1.0, 3.0 * dpi);
        stroke(&draw, [bx, y], [bx + sw, y + btn_h], c.rule, 1.0, 3.0 * dpi, 1.0);
        draw.add_text(
            [bx + 14.0 * dpi, y + 12.0 * dpi],
            rgba(c.muted, 1.0),
            "Search installations...",
        );
    }
    y += btn_h + 22.0 * dpi;

    // The version filter, as three ticks. They are labels here; the platform decides
    // what each one means when instances become real.
    {
        let draw = ui.get_window_draw_list();
        let _f = ui.push_font(fonts.small);
        draw.add_text([g.x + g.pad, y + 4.0 * dpi], rgba(c.muted, 1.0), "VERSIONS:");
        let mut tx = g.x + g.pad + text_width(ui, fonts.small, "VERSIONS:") + 18.0 * dpi;
        for (i, label) in ["Releases", "Snapshots", "Modded"].iter().enumerate() {
            let box_ = 18.0 * dpi;
            let on = i == 0;
            fill(&draw, [tx, y + 2.0 * dpi], [tx + box_, y + 2.0 * dpi + box_], if on { c.accent } else { c.well }, 1.0, 2.0 * dpi);
            stroke(&draw, [tx, y + 2.0 * dpi], [tx + box_, y + 2.0 * dpi + box_], if on { c.accent } else { c.bevel }, 1.0, 2.0 * dpi, 1.0);
            draw.add_text([tx + box_ + 8.0 * dpi, y + 4.0 * dpi], rgba(c.ink_soft, 1.0), *label);
            tx += box_ + 8.0 * dpi + text_width(ui, fonts.small, label) + 20.0 * dpi;
        }
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
        let sub = "Click '+ New...' above to download and install any version of Minecraft!";
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
        draw.add_text(
            [g.x + g.pad, y],
            rgba(c.muted, 1.0),
            fit(ui, &model.status, g.w - 2.0 * g.pad),
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

/// Skins: the model on the left, the wardrobe on the right. The player is drawn as
/// a stack of blocks from their own skin, which is the one picture a skin picker
/// has to get right.
fn skins_page(
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
    let y = heading(ui, fonts, c, dpi, g, "SKINS & WARDROBE", rise);
    let avail_h = height + rise - y - g.pad;
    // The model panel takes the left third and the wardrobe the rest; on a narrow
    // page the wardrobe drops under the model instead.
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
        player_figure(&draw, c, dpi, mx + model_w / 2.0, y + model_h * 0.46, (model_h * 0.34).min(model_w * 0.5));
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
        let line = "Classic Model (4px Arms) · Drag 360°";
        let [lw, _] = ui.calc_text_size(line);
        draw.add_text(
            [mx + (model_w - lw) / 2.0, y + model_h - 36.0 * dpi],
            rgba(c.accent, 1.0),
            line,
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

    // SKINS / CAPES, then the model kind, then the way in.
    let seg_h = 44.0 * dpi;
    let half = (ww - 10.0 * dpi) / 2.0;
    if wy + seg_h <= height + rise {
        let draw = ui.get_window_draw_list();
        raised_face(&draw, [wx, wy], [wx + half, wy + seg_h], c.accent, c.bevel, c.accent_lip, 3.0 * dpi, dpi);
        {
            let _f = ui.push_font(fonts.small);
            let [tw, th] = ui.calc_text_size("SKINS");
            draw.add_text([wx + (half - tw) / 2.0, wy + (seg_h - th) / 2.0], rgba(c.on_accent, 1.0), "SKINS");
        }
        raised_face(&draw, [wx + half + 10.0 * dpi, wy], [wx + ww, wy + seg_h], c.button, c.bevel, c.well, 3.0 * dpi, dpi);
        {
            let _f = ui.push_font(fonts.small);
            let [tw, th] = ui.calc_text_size("CAPES");
            draw.add_text([wx + half + 10.0 * dpi + (half - tw) / 2.0, wy + (seg_h - th) / 2.0], rgba(c.muted, 1.0), "CAPES");
        }
        wy += seg_h + 18.0 * dpi;
    }
    if wy + 20.0 * dpi <= height + rise {
        let draw = ui.get_window_draw_list();
        let _f = ui.push_font(fonts.small);
        draw.add_text([wx, wy], rgba(c.muted, 1.0), "PLAYER MODEL");
        wy += 26.0 * dpi;
    }
    if wy + seg_h <= height + rise {
        let draw = ui.get_window_draw_list();
        raised_face(&draw, [wx, wy], [wx + half, wy + seg_h], c.accent, c.bevel, c.accent_lip, 3.0 * dpi, dpi);
        {
            let _f = ui.push_font(fonts.small);
            let [tw, th] = ui.calc_text_size("CLASSIC (4px)");
            draw.add_text([wx + (half - tw) / 2.0, wy + (seg_h - th) / 2.0], rgba(c.on_accent, 1.0), "CLASSIC (4px)");
        }
        raised_face(&draw, [wx + half + 10.0 * dpi, wy], [wx + ww, wy + seg_h], c.button, c.bevel, c.well, 3.0 * dpi, dpi);
        {
            let _f = ui.push_font(fonts.small);
            let [tw, th] = ui.calc_text_size("SLIM (3px)");
            draw.add_text([wx + half + 10.0 * dpi + (half - tw) / 2.0, wy + (seg_h - th) / 2.0], rgba(c.muted, 1.0), "SLIM (3px)");
        }
        wy += seg_h + 18.0 * dpi;
    }
    if wy + seg_h <= height + rise
        && raised_button(ui, fonts, c, dpi, wx, wy, ww, seg_h, "+ BROWSE CUSTOM SKIN (PNG)", c.accent, c.accent_lip, c.on_accent)
    {
        sink.push(LauncherEvent::OpenSkins);
    }
    wy += seg_h + 22.0 * dpi;

    // The library: every PNG in import/skins, then the ones that ship.
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
        let line = "No PNGs in import/skins yet. Drop one there and reopen this page.";
        draw.add_text(
            [wx, cy + 8.0 * dpi],
            rgba(c.muted, 1.0),
            fit(ui, line, ww),
        );
    }
}

/// A player as blocks: head, body, arms, legs. Deliberately not a 3D model — the
/// picker's job is to show which file is chosen, and a flat figure reads at a
/// glance where a rotating one needs a finger to mean anything.
fn player_figure(draw: &DrawListMut<'_>, c: &Css, dpi: f32, cx: f32, cy: f32, s: f32) {
    let u = s / 8.0;
    let head = [cx - u * 1.4, cy - u * 4.0];
    fill(draw, head, [head[0] + u * 2.8, head[1] + u * 2.8], 0xc79a72, 1.0, u * 0.2);
    // Hair, eyes and mouth: enough to read as a face and no more.
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
    // only visible after a refresh.
    let bw = 150.0 * dpi;
    let bh = 36.0 * dpi;
    let mut bx = g.x + g.pad;
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
            rgba(if model.status.starts_with("Could not") { c.danger } else { c.muted }, 1.0),
            fit(ui, &model.status, g.x + g.w - g.pad - bx - bw - 64.0 * dpi),
        );
    }
    y += 40.0 * dpi;

    if model.mods.is_empty() {
        let draw = ui.get_window_draw_list();
        let line = "No mods to show. Refresh to ask the registry again.";
        draw.add_text([g.x + g.pad, y], rgba(c.muted, 1.0), line);
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
        let label = if installed { "Installed" } else { "Install" };
        let face = if installed { c.button } else { c.accent };
        let ink = if installed { c.muted } else { c.on_accent };
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
    let face = 72.0 * dpi;
    {
        let draw = ui.get_window_draw_list();
        panel(&draw, c, [g.x + g.pad, y], [g.x + g.w - g.pad, y + face + 32.0 * dpi], c.panel, 4.0 * dpi);
        fill(&draw, [g.x + g.pad + 16.0 * dpi, y + 16.0 * dpi], [g.x + g.pad + 16.0 * dpi + face, y + 16.0 * dpi + face], c.accent, 1.0, 4.0 * dpi);
        {
            let _f = ui.push_font(fonts.title);
            let initial: String = model.account.chars().take(1).collect::<String>().to_uppercase();
            let initial = if initial.is_empty() { "P".to_string() } else { initial };
            let [iw, ih] = ui.calc_text_size(initial.as_str());
            draw.add_text(
                [g.x + g.pad + 16.0 * dpi + (face - iw) / 2.0, y + 16.0 * dpi + (face - ih) / 2.0],
                rgba(c.on_accent, 1.0),
                initial.as_str(),
            );
        }
        {
            let _f = ui.push_font(fonts.pixel);
            draw.add_text(
                [g.x + g.pad + face + 32.0 * dpi, y + 26.0 * dpi],
                rgba(c.ink, 1.0),
                fit(ui, &model.account, g.w - 2.0 * g.pad - face - 48.0 * dpi),
            );
        }
        draw.add_text(
            [g.x + g.pad + face + 32.0 * dpi, y + 54.0 * dpi],
            rgba(c.muted, 1.0),
            "Offline account · tap to rename",
        );
    }
    ui.set_cursor_screen_pos([g.x + g.pad, y]);
    if ui.invisible_button("##rename", [g.w - 2.0 * g.pad, face + 32.0 * dpi]) {
        if sink.live() {
            // The launcher cannot write the model it was handed, so the rename flag
            // travels as an event and the platform turns it on.
        }
        sink.push(LauncherEvent::RenameAccount);
    }
    y += face + 48.0 * dpi;

    for (key, value) in &model.facts {
        if y + 44.0 * dpi > height + rise {
            break;
        }
        let draw = ui.get_window_draw_list();
        panel(&draw, c, [g.x + g.pad, y], [g.x + g.w - g.pad, y + 44.0 * dpi], c.panel, 3.0 * dpi);
        draw.add_text([g.x + g.pad + 16.0 * dpi, y + 14.0 * dpi], rgba(c.muted, 1.0), key.as_str());
        let [vw, _] = ui.calc_text_size(value.as_str());
        draw.add_text(
            [g.x + g.w - g.pad - 16.0 * dpi - vw, y + 14.0 * dpi],
            rgba(c.ink, 1.0),
            value.as_str(),
        );
        y += 44.0 * dpi + 8.0 * dpi;
    }

    let btn_w = 200.0 * dpi;
    let btn_h = 44.0 * dpi;
    if y + btn_h <= height + rise {
        if raised_button(ui, fonts, c, dpi, g.x + g.pad, y, btn_w, btn_h, "SETTINGS", c.button, c.bevel, c.ink) {
            sink.push(LauncherEvent::Goto(LauncherPage::Settings));
        }
        if raised_button(ui, fonts, c, dpi, g.x + g.pad + btn_w + 10.0 * dpi, y, btn_w, btn_h, "EXPORT", c.button, c.bevel, c.ink) {
            sink.push(LauncherEvent::ExportSettings);
        }
        if raised_button(ui, fonts, c, dpi, g.x + g.pad + (btn_w + 10.0 * dpi) * 2.0, y, btn_w, btn_h, "IMPORT", c.button, c.bevel, c.ink) {
            sink.push(LauncherEvent::ImportSettings);
        }
    }
}

/// The launcher's own settings: sliders and switches that write `options.txt`, so a
/// player never has to enter the game to change how it looks or how the controls
/// feel. The game reads the same file, so this is the same setting either way.
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
    y += 42.0 * dpi;
    let row_h = 56.0 * dpi;
    let label_w = 300.0 * dpi;
    for setting in &model.settings {
        if y + row_h > height + rise {
            break;
        }
        let draw = ui.get_window_draw_list();
        panel(&draw, c, [g.x + g.pad, y], [g.x + g.w - g.pad, y + row_h], c.panel, 3.0 * dpi);
        draw.add_text(
            [g.x + g.pad + 16.0 * dpi, y + row_h / 2.0 - 8.0 * dpi],
            rgba(c.ink_soft, 1.0),
            fit(ui, &setting.label, label_w),
        );
        let cx = g.x + g.pad + 16.0 * dpi + label_w;
        let cw = (g.x + g.w - g.pad - 16.0 * dpi - cx).max(80.0 * dpi);
        if setting.toggle {
            let tw = 56.0 * dpi;
            let th = 28.0 * dpi;
            let tx = g.x + g.w - g.pad - 16.0 * dpi - tw;
            let ty = y + (row_h - th) / 2.0;
            fill(&draw, [tx, ty], [tx + tw, ty + th], if setting.on { c.accent } else { c.well }, 1.0, th / 2.0);
            let knob = th - 8.0 * dpi;
            let kx = if setting.on { tx + tw - knob - 4.0 * dpi } else { tx + 4.0 * dpi };
            fill(&draw, [kx, ty + 4.0 * dpi], [kx + knob, ty + 4.0 * dpi + knob], c.ink, 1.0, knob / 2.0);
            ui.set_cursor_screen_pos([tx, ty]);
            let id = format!("##set{}", setting.key);
            if ui.invisible_button(id.as_str(), [tw, th]) {
                sink.push(LauncherEvent::ToggleOption(setting.key.clone(), !setting.on));
            }
        } else {
            // The track is the well and the fill is the value; the handle is a block,
            // because everything else on this screen is a block.
            let ty = y + row_h / 2.0 - 3.0 * dpi;
            fill(&draw, [cx, ty], [cx + cw, ty + 6.0 * dpi], c.well, 1.0, 3.0 * dpi);
            let span = (setting.max - setting.min).max(f32::EPSILON);
            let t = ((setting.value - setting.min) / span).clamp(0.0, 1.0);
            if t > 0.0 {
                fill(&draw, [cx, ty], [cx + cw * t, ty + 6.0 * dpi], c.accent, 1.0, 3.0 * dpi);
            }
            fill(
                &draw,
                [cx + cw * t - 5.0 * dpi, ty - 7.0 * dpi],
                [cx + cw * t + 5.0 * dpi, ty + 13.0 * dpi],
                c.ink,
                1.0,
                2.0 * dpi,
            );
            // The value, right of the track and left of the edge.
            let text = format!("{:.0}", setting.value);
            let _f = ui.push_font(fonts.small);
            let [tw, _] = ui.calc_text_size(text.as_str());
            draw.add_text(
                [g.x + g.w - g.pad - 16.0 * dpi - tw, y + row_h / 2.0 - 8.0 * dpi],
                rgba(c.muted, 1.0),
                text.as_str(),
            );
            // The hit target is the whole track's band, and the value comes from
            // where the finger landed: a slider you have to hit a 10px handle on is a
            // slider a thumb cannot use.
            let at = ui.io().mouse_pos[0];
            ui.set_cursor_screen_pos([cx, y]);
            let id = format!("##set{}", setting.key);
            if ui.invisible_button(id.as_str(), [cw, row_h]) {
                let frac = ((at - cx) / cw).clamp(0.0, 1.0);
                let value = setting.min + (setting.max - setting.min) * frac;
                sink.push(LauncherEvent::SetOption(setting.key.clone(), value));
            }
        }
        y += row_h + 8.0 * dpi;
    }
    if model.settings.is_empty() {
        let draw = ui.get_window_draw_list();
        draw.add_text(
            [g.x + g.pad, y + 8.0 * dpi],
            rgba(c.muted, 1.0),
            "No settings are available until the pack is installed.",
        );
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
