//! The launcher's front end, drawn in imgui over the game's own surface.
//!
//! The desktop launcher is a GPUI app, and GPUI has no Android backend: a phone
//! runs one activity with one winit loop, so the launcher cannot be a second
//! program the way it is on a desktop. It is drawn here instead, in the imgui
//! layer that already paints the touch controls, so it shares the loop, the
//! device and the font atlas with the game.
//!
//! The split is deliberate. This module knows how a launcher looks and nothing
//! about what is installed: the platform feeds it a [`LauncherModel`] of text and
//! numbers it has gathered from the engine, and it hands back [`LauncherEvent`]s
//! for the platform to act on. That keeps the crate free of the engine, the way
//! the rest of the overlay is.
//!
//! The look is Horizon's own — the `Blocks` palette and Pixelify Sans display
//! face from [`horizon_theme`], the same tokens playhorizon.dev and the desktop
//! launcher are built from — laid out as a top bar over one centred column of
//! cards, with the panels drawn by hand so it reads as a designed app rather than
//! a form.
//!
//! Every element is placed from a single `Grid`: one gutter, one column, one
//! right edge. The old screen grew a sidebar, a hero and a rail that each did
//! their own arithmetic, and the Play card's padding ended up a few pixels off the
//! world rows beneath it. Here there is nowhere for that to happen: a card and a
//! row both start at `grid.x` and end at `grid.x + grid.w`, and every right-aligned
//! action ends at `Grid::right`.

use imgui::{Condition, DrawListMut, FontConfig, FontId, FontSource, StyleColor, Ui, WindowFlags};

/// The pages the top bar's tabs switch between.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum LauncherPage {
    #[default]
    Play,
    Instances,
    Versions,
    Skins,
    Mods,
    Settings,
    About,
}

impl LauncherPage {
    pub fn label(self) -> &'static str {
        match self {
            LauncherPage::Play => "Play",
            LauncherPage::Instances => "Instances",
            LauncherPage::Versions => "Versions",
            LauncherPage::Skins => "Skins",
            LauncherPage::Mods => "Mods",
            LauncherPage::Settings => "Settings",
            LauncherPage::About => "About",
        }
    }
}

/// The tab row's order, left to right.
pub const PAGES: [LauncherPage; 7] = [
    LauncherPage::Play,
    LauncherPage::Instances,
    LauncherPage::Versions,
    LauncherPage::Skins,
    LauncherPage::Mods,
    LauncherPage::Settings,
    LauncherPage::About,
];

/// How many tabs there are. Kept as its own constant so the widths can be a fixed
/// array: the bar is laid out every frame and a `Vec` per tab would be waste.
const TABS: usize = PAGES.len();

/// One line in a list: a world, an instance, a version, a skin.
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
    /// Extra one-line facts for the About page.
    pub facts: Vec<(String, String)>,
    /// The name field is open, which on a phone means the keyboard belongs to it. The
    /// launcher raises it on the tap and the platform reads it back; the text in the
    /// field is the platform's, gathered a frame at a time from the IME.
    pub editing: bool,
    pub draft: String,
}

/// What the player asked for, for the platform to carry out.
#[derive(Clone, PartialEq, Eq, Debug)]
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
    OpenSettings,
    OpenSkins,
    /// Write the settings archive into import/.
    ExportSettings,
    /// Read the newest settings archive from import/.
    ImportSettings,
    Quit,
}

/// The `Blocks` palette, pulled from the shared design tokens so the phone
/// matches the website and the desktop launcher. Colours are `0xRRGGBB`.
pub struct Css {
    pub page: u32,
    pub side: u32,
    pub panel: u32,
    pub raised: u32,
    pub button: u32,
    pub well: u32,
    pub rule: u32,
    pub bevel: u32,
    pub ink: u32,
    pub ink_soft: u32,
    pub muted: u32,
    pub accent: u32,
    pub face: u32,
    pub on_face: u32,
}

const B: horizon_theme::Blocks = horizon_theme::BLOCKS_DARK;

pub const THEME: Css = Css {
    page: B.page,
    side: B.side,
    panel: B.panel,
    raised: B.raised,
    button: B.button,
    well: B.well,
    rule: B.rule,
    bevel: B.bevel,
    ink: B.ink,
    ink_soft: B.ink_soft,
    muted: B.muted,
    accent: B.accent,
    face: B.face,
    on_face: B.on_face,
};

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
    Fonts { body, pixel, title }
}

/// The page gutter, the top bar, the column's ceiling and the two card heights, in
/// logical pixels. One set of numbers, so no card can be padded differently from
/// the one above it.
const PAD: f32 = 24.0;
const BAR: f32 = 56.0;
const COLUMN: f32 = 1120.0;
const ROW: f32 = 68.0;
const HERO: f32 = 132.0;
/// The air between two tabs, and the tap padding a tab label gets on each side.
const TAB_GAP: f32 = 4.0;
const TAB_PAD: f32 = 12.0;
/// A phone narrower than this in framebuffer pixels cannot hold the full labels.
const COMPACT_BELOW: f32 = 900.0;

/// The one geometry every element is placed from.
struct Grid {
    /// The column's left edge. Every card and row starts here.
    x: f32,
    /// The column's width. Every card and row is exactly this wide.
    w: f32,
    /// The gutter: the top bar's inset, every card's inner padding and the space
    /// between two cards are all this one number.
    pad: f32,
    /// The space between cards. The same as `pad`, kept apart because it reads as a
    /// different idea where it is used.
    gap: f32,
    /// The top bar's bottom edge, and therefore where the content begins above.
    top: f32,
}

impl Grid {
    /// `rise` is the opening animation's offset: the whole interface is drawn this
    /// far below where it belongs while it arrives.
    fn new(width: f32, dpi: f32, rise: f32) -> Self {
        let pad = PAD * dpi;
        // Never negative: a window narrower than two gutters is not a phone, but the
        // arithmetic below would put every card off the left edge if it were allowed.
        let w = (width - 2.0 * pad).max(0.0).min(COLUMN * dpi);
        Self {
            x: (width - w) / 2.0,
            w,
            pad,
            gap: pad,
            top: BAR * dpi + rise,
        }
    }

    /// Where a right-aligned action of width `w` starts. Every Play button, row chip
    /// and footer button ends on this one edge, which is what the old screen's
    /// hand-rolled paddings never did.
    fn right(&self, w: f32) -> f32 {
        self.x + self.w - self.pad - w
    }
}
fn fill(draw: &DrawListMut<'_>, min: [f32; 2], max: [f32; 2], col: u32, a: f32, r: f32) {
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
    draw.add_rect(min, max, rgba(col, a))
        .rounding(r)
        .thickness(t)
        .build();
}

/// A card: its face, and the one-pixel rule that separates it from the page.
fn panel(
    draw: &DrawListMut<'_>,
    c: &Css,
    min: [f32; 2],
    max: [f32; 2],
    face: u32,
    radius: f32,
) {
    fill(draw, min, max, face, 1.0, radius);
    stroke(draw, min, max, c.rule, 1.0, radius, 1.0);
}

/// The width of `text` in a given face, without leaving that face pushed: measuring
/// with the wrong font is the other way a hand-built grid drifts.
fn text_width(ui: &Ui, font: FontId, text: &str) -> f32 {
    let _f = ui.push_font(font);
    ui.calc_text_size(text)[0]
}

/// A small rounded label whose right edge is `right`. Returns the width it took, so
/// the chip beside it can be placed without measuring anything twice. The chip always
/// measures and draws in the pixel face, so the caller's font stack cannot change how
/// wide it comes out.
fn chip(
    draw: &DrawListMut<'_>,
    ui: &Ui,
    fonts: &Fonts,
    dpi: f32,
    right: f32,
    cy: f32,
    text: &str,
    face: u32,
    ink: u32,
) -> f32 {
    let w = chip_width(ui, fonts, dpi, text);
    let h = 26.0 * dpi;
    let x = right - w;
    let _f = ui.push_font(fonts.pixel);
    let th = ui.calc_text_size(text)[1];
    fill(
        draw,
        [x, cy - h / 2.0],
        [right, cy + h / 2.0],
        face,
        1.0,
        8.0 * dpi,
    );
    draw.add_text([x + 10.0 * dpi, cy - th / 2.0], rgba(ink, 1.0), text);
    w
}

fn chip_width(ui: &Ui, fonts: &Fonts, dpi: f32, text: &str) -> f32 {
    let _f = ui.push_font(fonts.pixel);
    ui.calc_text_size(text)[0] + 20.0 * dpi
}

/// The one big action in a card. imgui's own button is kept — the platform's tap
/// handling is built around its rectangle — but its colours are pushed first so it
/// wears the theme rather than the default grey.
#[allow(clippy::too_many_arguments)]
fn action_button(
    ui: &Ui,
    fonts: &Fonts,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    label: &str,
    face: u32,
    ink: u32,
    hover: u32,
) -> bool {
    ui.set_cursor_screen_pos([x, y]);
    let _b = ui.push_style_color(StyleColor::Button, rgba(face, 1.0));
    let _hover = ui.push_style_color(StyleColor::ButtonHovered, rgba(hover, 1.0));
    let _active = ui.push_style_color(StyleColor::ButtonActive, rgba(hover, 1.0));
    let _text = ui.push_style_color(StyleColor::Text, rgba(ink, 1.0));
    let _f = ui.push_font(fonts.pixel);
    ui.button_with_size(label, [w, h])
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

/// The whole launcher, edge to edge. Navigation is applied to `model` in place, so
/// the tab that was pressed and the page it switched to are drawn in the same pass;
/// the platform still gets [`LauncherEvent`]s for actions it owns (Play, Quit, …).
#[allow(clippy::too_many_arguments)]
pub fn draw(
    ui: &Ui,
    model: &mut LauncherModel,
    fonts: &Fonts,
    dpi: f32,
    width: f32,
    height: f32,
    opening: Option<f32>,
    out: &mut Vec<LauncherEvent>,
) {
    let c = &THEME;
    // The opening owns the glass until it is spent. Its last frame is the first one
    // where a finger is allowed to land on anything.
    let live = !matches!(opening, Some(t) if t < 1.0);
    let mut sink = Sink { events: out, live };
    let rise = match opening {
        // The veil does most of the work; this is the travel that makes the interface
        // feel like it is arriving rather than fading. It is the one number the layout
        // is offset by, so the bar and the content rise together.
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
            let draw = ui.get_window_draw_list();
            // The page is behind everything on every frame, the opening included: the
            // glass is never the game's own surface showing through.
            fill(&draw, [0.0, 0.0], [width, height], c.page, 1.0, 0.0);
            if matches!(opening, Some(t) if t <= 0.0) {
                splash(ui, fonts, c, dpi, width, height);
                return;
            }
            let g = Grid::new(width, dpi, rise);
            let quit_in_bar = top_bar(ui, &mut *model, fonts, c, dpi, width, &g, &mut sink);
            let y = page_header(ui, model, fonts, c, dpi, &g);
            match model.page {
                LauncherPage::Play => {
                    play_page(ui, model, fonts, c, dpi, &g, y, height, &mut sink)
                }
                LauncherPage::Instances | LauncherPage::Versions => {
                    list_page(ui, model, fonts, c, dpi, &g, y, height, &mut sink)
                }
                LauncherPage::Skins => {
                    skins_page(ui, fonts, c, dpi, &g, y, height, &mut sink)
                }
                LauncherPage::Settings => {
                    settings_page(ui, model, fonts, c, dpi, &g, y, height, &mut sink)
                }
                LauncherPage::Mods => mods_page(ui, c, dpi, &g, y, height),
                LauncherPage::About => {
                    about_page(ui, model, fonts, c, dpi, &g, y, height, quit_in_bar, &mut sink)
                }
            }
            if let Some(t) = opening {
                // The reliable half of the entrance: the interface is already drawn,
                // so the veil is what fades. The rise above is the polish on top of it.
                let ease = horizon_theme::ease_out_cubic(t);
                if ease < 1.0 {
                    fill(&draw, [0.0, 0.0], [width, height], c.page, 1.0 - ease, 0.0);
                }
            }
        });
    if model.editing && live {
        rename_dialog(ui, model, fonts, c, dpi, width, height, &mut sink);
    }
}

/// The hold before the interface arrives: the wordmark on the glass and nothing else.
/// The accent bar under it is already at its full width because there is nothing to
/// measure against yet — the wait is the whole point of the frame.
fn splash(ui: &Ui, fonts: &Fonts, c: &Css, dpi: f32, width: f32, height: f32) {
    let draw = ui.get_window_draw_list();
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

/// How the top bar chose to fit this frame: which labels, how much air around them,
/// and where the account chip ended up. Made once, then only read, so the drawing
/// pass cannot disagree with the measurements that decided it.
struct BarPlan {
    compact: bool,
    pad: f32,
    /// The width the account chip's text column got: the full two-line width, the
    /// name alone, or nothing at all when the bar could not hold it.
    label: f32,
    /// Whether the chip kept the "Tap to rename" line under the name.
    hint: bool,
    widths: [f32; TABS],
    tabs_w: f32,
    account_x: f32,
    quit: bool,
}

/// The width the wordmark and its "LAUNCHER" line take before the tabs begin.
fn word_block(compact: bool, dpi: f32, mark_short: f32, mark_full: f32, launcher: f32) -> f32 {
    if compact {
        mark_short
    } else {
        mark_full + 10.0 * dpi + launcher
    }
}

/// The top bar: the wordmark, the seven tabs and the account, all placed from the
/// right edge inwards so the row cannot run off a narrow screen. Returns whether the
/// quit fitted beside them; the About page keeps it when it did not.
fn top_bar(
    ui: &Ui,
    model: &mut LauncherModel,
    fonts: &Fonts,
    c: &Css,
    dpi: f32,
    width: f32,
    g: &Grid,
    sink: &mut Sink<'_>,
) -> bool {
    let bar_h = BAR * dpi;
    let bar_y = g.top - bar_h;
    let cy = bar_y + bar_h / 2.0;
    let draw = ui.get_window_draw_list();
    fill(&draw, [0.0, bar_y], [width, g.top], c.side, 1.0, 0.0);
    fill(
        &draw,
        [0.0, g.top - 1.0],
        [width, g.top],
        c.rule,
        1.0,
        0.0,
    );

    let name = if model.account.is_empty() {
        "Player"
    } else {
        model.account.as_str()
    };
    let quit_w = text_width(ui, fonts.pixel, "Quit") + 24.0 * dpi;
    let mark_full = text_width(ui, fonts.pixel, "HORIZON");
    let mark_short = text_width(ui, fonts.pixel, "H");
    let launcher_w = text_width(ui, fonts.body, "LAUNCHER");
    let face = 30.0 * dpi;
    // The chip holds two lines — the name and the hint under it — so its label column
    // is as wide as the longer of the two, and no wider than the cap. A long name is
    // cut with an ellipsis rather than allowed to eat the tabs. `name_only` and `0.0`
    // are the two fallbacks the plan below may choose when the bar runs out of room.
    let name_only = text_width(ui, fonts.body, name).min(170.0 * dpi);
    let name_full = name_only
        .max(text_width(ui, fonts.body, "Tap to rename"))
        .min(170.0 * dpi);
    // A portrait phone is always compact: its width is the short edge the scale was
    // taken from, so it is 720 dpi by definition and the full labels cannot fit.
    let compact_forced = width < COMPACT_BELOW * dpi;

    // The bar is planned before a pixel of it is drawn. Each candidate is a set of
    // labels, a tab padding and an account label width; it fits when the tabs, laid
    // from the account leftwards, still clear the wordmark. The first that fits wins,
    // so the row is never clipped and never overlapped — it keeps its full labels,
    // then the quit, then the account's hint line, then the account's name, in that
    // order, because the tabs are the navigation and cannot go anywhere.
    let mut plan = None;
    'outer: for quit in [true, false] {
        for (compact, pad, label, hint) in [
            (false, TAB_PAD * dpi, name_full, true),
            (false, 8.0 * dpi, name_full, true),
            (true, TAB_PAD * dpi, name_full, true),
            (true, TAB_PAD * dpi, name_only, false),
            (true, 8.0 * dpi, name_only, false),
            (true, 4.0 * dpi, name_only, false),
            (true, 4.0 * dpi, 0.0, false),
        ] {
            if compact_forced && !compact {
                continue;
            }
            let widths = tab_widths(ui, fonts, compact, pad);
            let tabs_w = tab_total(&widths, dpi);
            let aw = face + 10.0 * dpi + label + 20.0 * dpi;
            let right = width - g.pad - aw - if quit { 14.0 * dpi + quit_w } else { 0.0 };
            let left =
                g.pad + word_block(compact, dpi, mark_short, mark_full, launcher_w) + 20.0 * dpi;
            if left + tabs_w <= right - 16.0 * dpi {
                plan = Some(BarPlan {
                    compact,
                    pad,
                    label,
                    hint,
                    widths,
                    tabs_w,
                    account_x: right,
                    quit,
                });
                break 'outer;
            }
        }
    }
    // Nothing fits on a screen smaller than any phone: the last candidate is still
    // better than no bar, and the tabs are the part that must stay reachable.
    let plan = plan.unwrap_or_else(|| {
        let widths = tab_widths(ui, fonts, true, 4.0 * dpi);
        let tabs_w = tab_total(&widths, dpi);
        BarPlan {
            compact: true,
            pad: 4.0 * dpi,
            label: 0.0,
            hint: false,
            widths,
            tabs_w,
            account_x: width - g.pad - (face + 10.0 * dpi + 20.0 * dpi),
            quit: false,
        }
    });
    let compact = plan.compact;
    let tab_pad = plan.pad;
    let label_w = plan.label;
    let widths = plan.widths;
    let tabs_w = plan.tabs_w;
    let account_w = face + 10.0 * dpi + label_w + 20.0 * dpi;
    let account_x = plan.account_x;
    let quit_in_bar = plan.quit;
    // Derived from the account's edge rather than recomputed from the screen: the
    // plan already reserved exactly this gap, so the two cannot disagree.
    let quit_x = account_x + account_w + 14.0 * dpi;
    let left = g.pad + word_block(compact, dpi, mark_short, mark_full, launcher_w) + 20.0 * dpi;

    {
        let _f = ui.push_font(fonts.pixel);
        draw.add_text(
            [g.pad, cy - 9.0 * dpi],
            rgba(c.ink, 1.0),
            if compact { "H" } else { "HORIZON" },
        );
    }
    if !compact {
        draw.add_text(
            [g.pad + mark_full + 10.0 * dpi, cy - 8.0 * dpi],
            rgba(c.muted, 1.0),
            "LAUNCHER",
        );
    }

    // The account chip: a face, the name, and the line that says it can be changed.
    let fx = account_x;
    let fy = cy - face / 2.0;
    fill(
        &draw,
        [fx, fy],
        [fx + face, fy + face],
        c.face,
        1.0,
        8.0 * dpi,
    );
    let initial: String = name.chars().take(1).collect::<String>().to_uppercase();
    {
        let _f = ui.push_font(fonts.pixel);
        let [iw, ih] = ui.calc_text_size(initial.as_str());
        draw.add_text(
            [fx + (face - iw) / 2.0, fy + (face - ih) / 2.0],
            rgba(c.on_face, 1.0),
            initial.as_str(),
        );
    }
    let text_x = fx + face + 10.0 * dpi;
    // A name is user data, so it is cut rather than trusted to fit. The hint line is
    // the first thing the plan gives up, and the name the second: a bar with no room
    // for either still has the face and the whole chip as a tap target.
    if plan.label > 0.0 {
        if plan.hint {
            draw.add_text(
                [text_x, cy - 17.0 * dpi],
                rgba(c.ink, 1.0),
                fit(ui, name, label_w),
            );
            draw.add_text(
                [text_x, cy + 2.0 * dpi],
                rgba(c.muted, 1.0),
                "Tap to rename",
            );
        } else {
            draw.add_text(
                [text_x, cy - 8.0 * dpi],
                rgba(c.ink, 1.0),
                fit(ui, name, label_w),
            );
        }
    }
    ui.set_cursor_screen_pos([account_x, bar_y]);
    if ui.invisible_button("##account", [account_w, bar_h - 1.0]) {
        if sink.live() {
            model.editing = true;
        }
        sink.push(LauncherEvent::RenameAccount);
    }

    let mut tx = (account_x - 16.0 * dpi - tabs_w).max(left);
    for (i, page) in PAGES.iter().enumerate() {
        let page = *page;
        let tw = widths[i];
        let label = if compact {
            short_label(page)
        } else {
            page.label()
        };
        let active = page == model.page;
        {
            let _f = ui.push_font(fonts.pixel);
            draw.add_text(
                [tx + tab_pad, cy - 9.0 * dpi],
                rgba(if active { c.ink } else { c.muted }, 1.0),
                label,
            );
        }
        if active {
            fill(
                &draw,
                [tx + tab_pad, g.top - 3.0 * dpi],
                [tx + tw - tab_pad, g.top - 1.0],
                c.accent,
                1.0,
                0.0,
            );
        }
        ui.set_cursor_screen_pos([tx, bar_y]);
        let id = format!("##tab{i}");
        if ui.invisible_button(id.as_str(), [tw, bar_h - 1.0]) {
            if sink.live() {
                // Applied here, not on a round trip to the engine: the loading
                // surface redraws on demand, so the page the finger landed on has to
                // be the page drawn by the pass that is already under way.
                model.page = page;
            }
            sink.push(LauncherEvent::Goto(page));
        }
        tx += tw + TAB_GAP * dpi;
    }

    if quit_in_bar {
        {
            let _f = ui.push_font(fonts.pixel);
            draw.add_text(
                [quit_x + 12.0 * dpi, cy - 9.0 * dpi],
                rgba(c.muted, 1.0),
                "Quit",
            );
        }
        ui.set_cursor_screen_pos([quit_x, bar_y]);
        if ui.invisible_button("##quit", [quit_w, bar_h - 1.0]) {
            sink.push(LauncherEvent::Quit);
        }
    }
    quit_in_bar
}

fn tab_widths(ui: &Ui, fonts: &Fonts, compact: bool, pad: f32) -> [f32; TABS] {
    let mut widths = [0.0; TABS];
    let _f = ui.push_font(fonts.pixel);
    for (i, page) in PAGES.iter().enumerate() {
        let label = if compact {
            short_label(*page)
        } else {
            page.label()
        };
        widths[i] = ui.calc_text_size(label)[0] + 2.0 * pad;
    }
    widths
}

fn tab_total(widths: &[f32; TABS], dpi: f32) -> f32 {
    widths.iter().sum::<f32>() + TAB_GAP * dpi * (TABS - 1) as f32
}

/// The tab label when the bar is too narrow for the full one. Dropping to initials
/// was the other option and it makes seven tabs that all read alike.
fn short_label(page: LauncherPage) -> &'static str {
    match page {
        LauncherPage::Play => "Play",
        LauncherPage::Instances => "Inst",
        LauncherPage::Versions => "Vers",
        LauncherPage::Skins => "Skin",
        LauncherPage::Mods => "Mods",
        LauncherPage::Settings => "Set",
        LauncherPage::About => "About",
    }
}

fn subtitle(page: LauncherPage) -> &'static str {
    match page {
        LauncherPage::Play => "Jump back in, or start something new.",
        LauncherPage::Instances => {
            "Separate worlds, settings and skins on the one supported version."
        }
        LauncherPage::Versions => {
            "This native build plays Minecraft Java 26.3; other versions are listed for reference."
        }
        LauncherPage::Skins => "Your skin comes from your account, or a PNG in import/skins.",
        LauncherPage::Mods => "Component mods need a runtime this build does not carry yet.",
        LauncherPage::Settings => "Video, controls, language and accessibility.",
        LauncherPage::About => "What this build is.",
    }
}

/// The page's title and its one line of explanation. Returns the y the page's own
/// content starts at, so every page begins on the same line.
fn page_header(
    ui: &Ui,
    model: &LauncherModel,
    fonts: &Fonts,
    c: &Css,
    dpi: f32,
    g: &Grid,
) -> f32 {
    let y = g.top + g.pad;
    let draw = ui.get_window_draw_list();
    {
        let _f = ui.push_font(fonts.title);
        draw.add_text([g.x, y], rgba(c.ink, 1.0), model.page.label());
    }
    draw.add_text(
        [g.x, y + 44.0 * dpi],
        rgba(c.muted, 1.0),
        subtitle(model.page),
    );
    y + 84.0 * dpi
}

fn row_height(dpi: f32) -> f32 {
    ROW * dpi
}

fn play_page(
    ui: &Ui,
    model: &LauncherModel,
    fonts: &Fonts,
    c: &Css,
    dpi: f32,
    g: &Grid,
    y0: f32,
    height: f32,
    sink: &mut Sink<'_>,
) {
    let playable = model.playable && !model.instance.is_empty();
    let hero = HERO * dpi;
    let btn_w = 176.0 * dpi;
    let btn_h = 44.0 * dpi;
    // Nothing is drawn under the bottom edge: a phone has no wheel, so a card that
    // would be cut in half is worse than a card that is not there.
    if y0 + hero > height {
        return;
    }
    let draw = ui.get_window_draw_list();
    panel(
        &draw,
        c,
        [g.x, y0],
        [g.x + g.w, y0 + hero],
        c.panel,
        14.0 * dpi,
    );

    let text_w = (g.w - 2.0 * g.pad - btn_w - g.gap).max(0.0);
    {
        let _f = ui.push_font(fonts.title);
        let title = if model.instance.is_empty() {
            "No instance".to_string()
        } else {
            model.instance.clone()
        };
        draw.add_text(
            [g.x + g.pad, y0 + g.pad],
            rgba(c.ink, 1.0),
            fit(ui, &title, text_w),
        );
    }
    draw.add_text(
        [g.x + g.pad, y0 + g.pad + 38.0 * dpi],
        rgba(c.muted, 1.0),
        format!("Minecraft Java {} · native", model.version),
    );

    // The track sits on the card's bottom inner edge, inset by the gutter on both
    // sides, and the status line lives in the band above it: at 132 the title, the
    // version and a message only fit if the message is given its own line.
    let track_top = y0 + hero - g.pad - 4.0 * dpi;
    if !model.status.is_empty() {
        // Above the track and cut to the room left of the button: an install error is
        // as long as the URL in it.
        draw.add_text(
            [g.x + g.pad, track_top - 6.0 * dpi - 16.0 * dpi],
            rgba(c.muted, 1.0),
            fit(ui, &model.status, text_w),
        );
    }
    if let Some(progress) = model.progress {
        let p = progress.clamp(0.0, 1.0);
        let left = g.x + g.pad;
        let right = g.x + g.w - g.pad;
        fill(
            &draw,
            [left, track_top],
            [right, track_top + 4.0 * dpi],
            c.rule,
            1.0,
            2.0 * dpi,
        );
        if p > 0.0 {
            fill(
                &draw,
                [left, track_top],
                [left + (right - left) * p, track_top + 4.0 * dpi],
                c.accent,
                1.0,
                2.0 * dpi,
            );
        }
    }

    // The one button on the front page does the one thing that is missing: play what
    // is installed, install what is not — or, when the platform has something else to
    // offer (a restart after the card's own install), say that instead.
    let label = match &model.front {
        Some(text) => text.clone(),
        None if playable => "Play".to_string(),
        None => "Install".to_string(),
    };
    if action_button(
        ui,
        fonts,
        g.right(btn_w),
        y0 + (hero - btn_h) / 2.0,
        btn_w,
        btn_h,
        &label,
        if playable { c.face } else { c.button },
        if playable { c.on_face } else { c.muted },
        if playable { c.accent } else { c.raised },
    ) {
        sink.push(match model.front.as_deref() {
            Some(_) => LauncherEvent::Restart,
            None if playable => LauncherEvent::Play,
            None => LauncherEvent::Install,
        });
    }

    let mut y = y0 + hero + g.gap;
    {
        let _f = ui.push_font(fonts.pixel);
        draw.add_text([g.x, y], rgba(c.muted, 1.0), "WORLDS");
    }
    y += 26.0 * dpi;
    if model.rows.is_empty() {
        let h = 96.0 * dpi;
        if y + h > height {
            return;
        }
        panel(
            &draw,
            c,
            [g.x, y],
            [g.x + g.w, y + h],
            c.panel,
            12.0 * dpi,
        );
        {
            let _f = ui.push_font(fonts.pixel);
            draw.add_text(
                [g.x + g.pad, y + g.pad],
                rgba(c.ink, 1.0),
                "No worlds yet",
            );
        }
        draw.add_text(
            [g.x + g.pad, y + g.pad + 25.0 * dpi],
            rgba(c.muted, 1.0),
            "Make one from Singleplayer.",
        );
    } else {
        let h = row_height(dpi);
        for row in &model.rows {
            // A phone has no wheel, so the list is not scrollable: a row that would
            // run under the bottom edge is not drawn at all rather than half drawn.
            if y + h > height {
                break;
            }
            row_card(ui, fonts, c, dpi, g, y, row, LauncherEvent::PlayRow, true, sink);
            y += h + g.gap;
        }
    }
}

fn list_page(
    ui: &Ui,
    model: &LauncherModel,
    fonts: &Fonts,
    c: &Css,
    dpi: f32,
    g: &Grid,
    y0: f32,
    height: f32,
    sink: &mut Sink<'_>,
) {
    let h = row_height(dpi);
    let mut y = y0;
    for row in &model.rows {
        if y + h > height {
            break;
        }
        // A listed instance or version is picked, not played: the rows on this page
        // are not worlds, and a tap must not open one that happens to share its index.
        row_card(
            ui,
            fonts,
            c,
            dpi,
            g,
            y,
            row,
            LauncherEvent::SelectRow,
            false,
            sink,
        );
        y += h + g.gap;
    }
    if model.page != LauncherPage::Instances {
        return;
    }
    let btn_w = 180.0 * dpi;
    let btn_h = 44.0 * dpi;
    if y + btn_h <= height {
        if action_button(
            ui,
            fonts,
            g.right(btn_w),
            y,
            btn_w,
            btn_h,
            "New instance",
            c.button,
            c.ink,
            c.raised,
        ) {
            sink.push(LauncherEvent::NewInstance);
        }
        y += btn_h + 12.0 * dpi;
    }
    if !model.status.is_empty() && y + 18.0 * dpi <= height {
        let draw = ui.get_window_draw_list();
        draw.add_text(
            [g.x, y],
            rgba(c.muted, 1.0),
            fit(ui, &model.status, g.w),
        );
    }
}

/// One row: a world on the Play page, an instance or a version elsewhere. The height,
/// the column and the right edge come from the grid, which is why a row lines up with
/// the Play card above it without either of them knowing about the other.
fn row_card(
    ui: &Ui,
    fonts: &Fonts,
    c: &Css,
    dpi: f32,
    g: &Grid,
    y: f32,
    row: &LauncherRow,
    on_row: fn(u32) -> LauncherEvent,
    play_chip: bool,
    sink: &mut Sink<'_>,
) {
    let h = row_height(dpi);
    let draw = ui.get_window_draw_list();
    panel(
        &draw,
        c,
        [g.x, y],
        [g.x + g.w, y + h],
        if row.selected { c.raised } else { c.panel },
        12.0 * dpi,
    );

    let tile = 36.0 * dpi;
    let tx = g.x + g.pad;
    let ty = y + (h - tile) / 2.0;
    fill(
        &draw,
        [tx, ty],
        [tx + tile, ty + tile],
        c.face,
        1.0,
        10.0 * dpi,
    );
    let initial: String = row
        .title
        .chars()
        .take(1)
        .collect::<String>()
        .to_uppercase();
    {
        let _f = ui.push_font(fonts.pixel);
        let [iw, ih] = ui.calc_text_size(initial.as_str());
        draw.add_text(
            [tx + (tile - iw) / 2.0, ty + (tile - ih) / 2.0],
            rgba(c.on_face, 1.0),
            initial.as_str(),
        );
    }

    // The chips are measured first so the title can be cut to the room they leave:
    // a long world name used to run underneath its own badge.
    let badge_w = if row.badge.is_empty() {
        0.0
    } else {
        chip_width(ui, fonts, dpi, &row.badge) + 8.0 * dpi
    };
    let play_w = if play_chip && row.enabled {
        chip_width(ui, fonts, dpi, "Play") + 8.0 * dpi
    } else {
        0.0
    };
    let text_x = g.x + g.pad + 48.0 * dpi;
    let text_w = (g.x + g.w - g.pad - text_x - badge_w - play_w - 12.0 * dpi).max(0.0);
    let title_col = if row.enabled { c.ink } else { c.muted };
    {
        let _f = ui.push_font(fonts.pixel);
        draw.add_text(
            [text_x, y + 12.0 * dpi],
            rgba(title_col, 1.0),
            fit(ui, &row.title, text_w),
        );
    }
    draw.add_text(
        [text_x, y + 34.0 * dpi],
        rgba(c.muted, 1.0),
        fit(ui, &row.subtitle, text_w),
    );

    let cy = y + h / 2.0;
    let mut rx = g.x + g.w - g.pad;
    if !row.badge.is_empty() {
        rx -= chip(
            &draw,
            ui,
            fonts,
            dpi,
            rx,
            cy,
            &row.badge,
            c.well,
            c.muted,
        ) + 8.0 * dpi;
    }
    if play_chip && row.enabled {
        // The chip is the affordance, and it sits inside the row's own hit target, so
        // the thing that looks tappable and the thing that answers are the same rect.
        let _ = chip(&draw, ui, fonts, dpi, rx, cy, "Play", c.button, c.ink);
    }

    // Placed last so it covers the chips it just drew: one target for the whole row.
    ui.set_cursor_screen_pos([g.x, y]);
    let id = format!("##row{}", row.id);
    if ui.invisible_button(id.as_str(), [g.w, h]) && row.enabled {
        sink.push(on_row(row.id));
    }
}

fn skins_page(
    ui: &Ui,
    fonts: &Fonts,
    c: &Css,
    dpi: f32,
    g: &Grid,
    y: f32,
    height: f32,
    sink: &mut Sink<'_>,
) {
    let body = "Your skin comes from your account, or from a PNG dropped into import/skins.";
    let lines = wrap(ui, body, g.w - 2.0 * g.pad);
    let btn_h = 44.0 * dpi;
    let h = card_height(g, dpi, lines.len(), btn_h);
    if y + h > height {
        return;
    }
    let draw = ui.get_window_draw_list();
    panel(&draw, c, [g.x, y], [g.x + g.w, y + h], c.panel, 14.0 * dpi);
    draw_lines(&draw, c, dpi, g, g.x, y, &lines);
    let btn_w = (240.0 * dpi).min(g.w - 2.0 * g.pad);
    if action_button(
        ui,
        fonts,
        g.right(btn_w),
        y + h - g.pad - btn_h,
        btn_w,
        btn_h,
        "Open the skin picker",
        c.button,
        c.ink,
        c.raised,
    ) {
        sink.push(LauncherEvent::OpenSkins);
    }
}

fn settings_page(
    ui: &Ui,
    model: &LauncherModel,
    fonts: &Fonts,
    c: &Css,
    dpi: f32,
    g: &Grid,
    y: f32,
    height: f32,
    sink: &mut Sink<'_>,
) {
    // Two cards want the width of a tablet; on a phone they stack, and the column is
    // what decides, not the glass.
    let side = g.w >= 720.0 * dpi;
    let card_w = if side { (g.w - g.gap) / 2.0 } else { g.w };
    let lines1 = wrap(
        ui,
        "Video, controls, language and accessibility live in the game's own settings.",
        card_w - 2.0 * g.pad,
    );
    let lines2 = wrap(
        ui,
        "Settings, keys and button layouts as one file in import/.",
        card_w - 2.0 * g.pad,
    );
    let btn_h = 44.0 * dpi;
    let h = card_height(g, dpi, lines1.len().max(lines2.len()), btn_h);
    let x2 = if side { g.x + card_w + g.gap } else { g.x };
    let y2 = if side { y } else { y + h + g.gap };

    if y + h <= height {
        let draw = ui.get_window_draw_list();
        panel(&draw, c, [g.x, y], [g.x + card_w, y + h], c.panel, 14.0 * dpi);
        draw_lines(&draw, c, dpi, g, g.x, y, &lines1);
        let btn_w = (200.0 * dpi).min(card_w - 2.0 * g.pad);
        if action_button(
            ui,
            fonts,
            g.x + card_w - g.pad - btn_w,
            y + h - g.pad - btn_h,
            btn_w,
            btn_h,
            "Open settings",
            c.button,
            c.ink,
            c.raised,
        ) {
            sink.push(LauncherEvent::OpenSettings);
        }
    }

    if y2 + h <= height {
        let draw = ui.get_window_draw_list();
        panel(&draw, c, [x2, y2], [x2 + card_w, y2 + h], c.panel, 14.0 * dpi);
        draw_lines(&draw, c, dpi, g, x2, y2, &lines2);
        // Both buttons share the card's right edge and the gutter between them, so
        // the pair is placed from the edge inwards like everything else.
        let two = ((card_w - 2.0 * g.pad - g.pad) / 2.0).max(0.0);
        let import_x = x2 + card_w - g.pad - two;
        let export_x = import_x - g.pad - two;
        let by = y2 + h - g.pad - btn_h;
        if action_button(
            ui,
            fonts,
            export_x,
            by,
            two,
            btn_h,
            "Export settings",
            c.button,
            c.ink,
            c.raised,
        ) {
            sink.push(LauncherEvent::ExportSettings);
        }
        if action_button(
            ui,
            fonts,
            import_x,
            by,
            two,
            btn_h,
            "Import settings",
            c.button,
            c.ink,
            c.raised,
        ) {
            sink.push(LauncherEvent::ImportSettings);
        }
    }

    let after = if side { y + h } else { y2 + h };
    if !model.status.is_empty() {
        let sy = after + g.gap;
        if sy + 18.0 * dpi <= height {
            let draw = ui.get_window_draw_list();
            draw.add_text(
                [g.x, sy],
                rgba(c.muted, 1.0),
                fit(ui, &model.status, g.w),
            );
        }
    }
}

fn mods_page(ui: &Ui, c: &Css, dpi: f32, g: &Grid, y: f32, height: f32) {
    let lines = wrap(
        ui,
        "Horizon's component mods need a runtime this build does not carry yet. Modpacks \
         ported to 26.3 run as resource and data packs.",
        g.w - 2.0 * g.pad,
    );
    let h = g.pad * 2.0 + lines.len().max(1) as f32 * 20.0 * dpi;
    if y + h > height {
        return;
    }
    let draw = ui.get_window_draw_list();
    panel(&draw, c, [g.x, y], [g.x + g.w, y + h], c.panel, 14.0 * dpi);
    draw_lines(&draw, c, dpi, g, g.x, y, &lines);
}

fn about_page(
    ui: &Ui,
    model: &LauncherModel,
    fonts: &Fonts,
    c: &Css,
    dpi: f32,
    g: &Grid,
    y0: f32,
    height: f32,
    quit_in_bar: bool,
    sink: &mut Sink<'_>,
) {
    let h = 48.0 * dpi;
    let mut y = y0;
    for (key, value) in &model.facts {
        if y + h > height {
            break;
        }
        let draw = ui.get_window_draw_list();
        panel(&draw, c, [g.x, y], [g.x + g.w, y + h], c.panel, 10.0 * dpi);
        let [vw, vh] = ui.calc_text_size(value.as_str());
        let key_w = (g.w - 2.0 * g.pad - vw - g.gap).max(0.0);
        draw.add_text(
            [g.x + g.pad, y + (h - 15.0 * dpi) / 2.0],
            rgba(c.muted, 1.0),
            fit(ui, key, key_w),
        );
        draw.add_text(
            [g.x + g.w - g.pad - vw, y + (h - vh) / 2.0],
            rgba(c.ink, 1.0),
            value.as_str(),
        );
        y += h + g.gap;
    }
    if quit_in_bar {
        return;
    }
    // The bar had no room for it, so the page that describes the build also offers the
    // way out of it.
    let btn_w = 120.0 * dpi;
    let btn_h = 44.0 * dpi;
    if y + btn_h <= height
        && action_button(
            ui,
            fonts,
            g.right(btn_w),
            y,
            btn_w,
            btn_h,
            "Quit",
            c.button,
            c.ink,
            c.raised,
        )
    {
        sink.push(LauncherEvent::Quit);
    }
}

fn card_height(g: &Grid, dpi: f32, lines: usize, button: f32) -> f32 {
    g.pad * 2.0 + lines.max(1) as f32 * 20.0 * dpi + g.gap + button
}

fn draw_lines(
    draw: &DrawListMut<'_>,
    c: &Css,
    dpi: f32,
    g: &Grid,
    x: f32,
    y: f32,
    lines: &[String],
) {
    for (i, line) in lines.iter().enumerate() {
        draw.add_text(
            [x + g.pad, y + g.pad + i as f32 * 20.0 * dpi],
            rgba(c.muted, 1.0),
            line.as_str(),
        );
    }
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

/// The text cut to what fits, with an ellipsis. A status line is the one string the
/// launcher does not get to choose the length of — an install failure carries a URL —
/// and a clipped message reads better than one that runs under the panel beside it.
fn fit(ui: &Ui, text: &str, max_w: f32) -> String {
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

/// The name field, on a pass of its own over the whole screen. The phone's keyboard
/// covers the bottom half, so the card sits high and the launcher behind it waits; a
/// tap anywhere outside is a way back out.
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
                fill(&draw, [x, y], [x + w, y + h], c.panel, 1.0, 16.0);
                stroke(&draw, [x, y], [x + w, y + h], c.bevel, 1.0, 16.0, 1.0);
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
                field(
                    &draw,
                    ui,
                    fonts,
                    c,
                    dpi,
                    x + inner,
                    y + 68.0 * dpi,
                    keep,
                    &model.draft,
                );
            }

            ui.set_cursor_screen_pos([x + inner, y + h - 54.0 * dpi]);
            {
                let _b = ui.push_style_color(StyleColor::Button, rgba(c.face, 1.0));
                let _hover = ui.push_style_color(StyleColor::ButtonHovered, rgba(c.accent, 1.0));
                let _a = ui.push_style_color(StyleColor::ButtonActive, rgba(c.accent, 1.0));
                let _t = ui.push_style_color(StyleColor::Text, rgba(c.on_face, 1.0));
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
    fill(
        draw,
        [x, y],
        [x + width, y + 50.0 * dpi],
        c.well,
        1.0,
        10.0 * dpi,
    );
    fill(
        draw,
        [x, y + 49.0 * dpi],
        [x + width, y + 51.0 * dpi],
        c.accent,
        1.0,
        0.0,
    );
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
