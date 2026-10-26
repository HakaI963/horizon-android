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
//! launcher are built from — laid out as a sidebar, a hero panel and cards, with
//! the panels drawn by hand so it reads as a designed app rather than a form.

use imgui::{Condition, DrawListMut, FontConfig, FontId, FontSource, StyleColor, Ui, WindowFlags};

/// The pages the sidebar switches between.
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

/// The sidebar's order.
pub const PAGES: [LauncherPage; 7] = [
    LauncherPage::Play,
    LauncherPage::Instances,
    LauncherPage::Versions,
    LauncherPage::Skins,
    LauncherPage::Mods,
    LauncherPage::Settings,
    LauncherPage::About,
];

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
    /// The account card was tapped: the platform opens a field and starts feeding it.
    RenameAccount,
    /// A new player name for the account card to keep.
    SetAccount(String),
    OpenSettings,
    OpenSkins,
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

fn fill(draw: &DrawListMut<'_>, min: [f32; 2], max: [f32; 2], col: u32, a: f32, r: f32) {
    draw.add_rect(min, max, rgba(col, a)).filled(true).rounding(r).build();
}

fn stroke(draw: &DrawListMut<'_>, min: [f32; 2], max: [f32; 2], col: u32, a: f32, r: f32, t: f32) {
    draw.add_rect(min, max, rgba(col, a)).rounding(r).thickness(t).build();
}

/// The whole launcher, edge to edge. Navigation is applied to `model` in place, so
/// the sidebar and the content it just switched to are drawn in the same pass; the
/// platform still gets [`LauncherEvent`]s for actions it owns (Play, Quit, …).
pub fn draw(
    ui: &Ui,
    model: &mut LauncherModel,
    fonts: &Fonts,
    dpi: f32,
    width: f32,
    height: f32,
    out: &mut Vec<LauncherEvent>,
) {
    let c = &THEME;
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
            // Page background, painted once behind everything.
            {
                let draw = ui.get_window_draw_list();
                fill(&draw, [0.0, 0.0], [width, height], c.page, 1.0, 0.0);
            }
            let sidebar = (236.0 * dpi).min(width * 0.42);
            sidebar_panel(ui, model, fonts, c, dpi, sidebar, height, out);
            ui.set_cursor_screen_pos([sidebar, 0.0]);
            let _content = ui.push_style_color(StyleColor::ChildBg, [0.0, 0.0, 0.0, 0.0]);
            let _ = ui
                .child_window("##content")
                .size([width - sidebar, -1.0])
                .build(|| {
                    content(ui, model, fonts, c, dpi, width - sidebar, out);
                });
            if model.editing {
                rename_dialog(ui, model, fonts, c, dpi, width, height, out);
            }
        });
}

fn sidebar_panel(
    ui: &Ui,
    model: &mut LauncherModel,
    fonts: &Fonts,
    c: &Css,
    dpi: f32,
    sidebar: f32,
    height: f32,
    out: &mut Vec<LauncherEvent>,
) {
    let _side = ui.push_style_color(StyleColor::ChildBg, rgba(c.side, 1.0));
    let _ = ui
        .child_window("##sidebar")
        .size([sidebar, height])
        .build(|| {
            let pad = 18.0 * dpi;
            ui.set_cursor_pos([pad, 22.0 * dpi]);
            {
                let _f = ui.push_font(fonts.title);
                ui.text("HORIZON");
            }
            ui.set_cursor_pos([pad + 1.0, 66.0 * dpi]);
            ui.text_colored(rgba(c.muted, 1.0), "LAUNCHER");
            {
                let draw = ui.get_window_draw_list();
                fill(&draw, [pad, 88.0 * dpi], [sidebar - pad, 89.0 * dpi], c.accent, 1.0, 0.0);
            }
            ui.set_cursor_pos([0.0, 108.0 * dpi]);
            for page in PAGES {
                let selected = page == model.page;
                let [x, y] = ui.cursor_screen_pos();
                let h = 46.0 * dpi;
                let w = sidebar;
                {
                    let draw = ui.get_window_draw_list();
                    if selected {
                        fill(&draw, [x + 10.0 * dpi, y], [w - 10.0 * dpi, y + h], c.raised, 1.0, 10.0);
                        fill(&draw, [x + 4.0 * dpi, y + 12.0 * dpi], [x + 7.0 * dpi, y + h - 12.0 * dpi], c.accent, 1.0, 2.0);
                    }
                    let _f = ui.push_font(fonts.pixel);
                    draw.add_text(
                        [x + 24.0 * dpi, y + h / 2.0 - 9.0 * dpi],
                        rgba(if selected { c.ink } else { c.muted }, 1.0),
                        page.label(),
                    );
                }
                if ui.invisible_button(page.label(), [w, h]) {
                    // Applied here, not on a round trip to the engine: the loading
                    // surface redraws on demand, so the page the finger landed on has
                    // to be the page drawn by the pass that is already under way.
                    model.page = page;
                    out.push(LauncherEvent::Goto(page));
                }
            }
            // Footer: where this build stands, a way out, and the account under it.
            let ay = height - 64.0 * dpi;
            let fy = ay - 46.0 * dpi;
            let quit = "Quit";
            let qw = ui.calc_text_size(quit)[0] + 16.0 * dpi;
            {
                let draw = ui.get_window_draw_list();
                draw.add_text(
                    [pad, fy + 16.0 * dpi],
                    rgba(c.muted, 1.0),
                    format!("{} · native", model.version),
                );
                draw.add_text([sidebar - pad - qw, fy + 16.0 * dpi], rgba(c.muted, 1.0), quit);
                fill(&draw, [pad, ay], [sidebar - pad, ay + 48.0 * dpi], c.panel, 1.0, 10.0);
                stroke(&draw, [pad, ay], [sidebar - pad, ay + 48.0 * dpi], c.rule, 1.0, 10.0, 1.0);
                fill(&draw, [pad + 12.0 * dpi, ay + 14.0 * dpi], [pad + 34.0 * dpi, ay + 36.0 * dpi], c.face, 1.0, 6.0);
                let name = if model.account.is_empty() { "Player" } else { model.account.as_str() };
                draw.add_text([pad + 44.0 * dpi, ay + 10.0 * dpi], rgba(c.ink, 1.0), name);
                draw.add_text([pad + 44.0 * dpi, ay + 28.0 * dpi], rgba(c.muted, 1.0), "Tap to rename");
            }
            ui.set_cursor_screen_pos([sidebar - pad - qw, fy]);
            if ui.invisible_button("##quit", [qw, 34.0 * dpi]) {
                out.push(LauncherEvent::Quit);
            }
            ui.set_cursor_screen_pos([pad, ay]);
            if ui.invisible_button("##account", [sidebar - 2.0 * pad, 48.0 * dpi]) {
                model.editing = true;
                out.push(LauncherEvent::RenameAccount);
            }
        });
}

fn content(
    ui: &Ui,
    model: &LauncherModel,
    fonts: &Fonts,
    c: &Css,
    dpi: f32,
    width: f32,
    out: &mut Vec<LauncherEvent>,
) {
    let pad = 28.0 * dpi;
    let [ox, oy] = ui.cursor_screen_pos();
    let rail = if width > 720.0 * dpi {
        (320.0 * dpi).min(width * 0.34)
    } else {
        0.0
    };
    let col = width - rail;
    ui.set_cursor_pos([pad, 26.0 * dpi]);
    {
        let _f = ui.push_font(fonts.title);
        ui.text(model.page.label());
    }
    ui.set_cursor_pos([pad, 68.0 * dpi]);
    let sub = match model.page {
        LauncherPage::Play => "Jump back in, or start something new.",
        LauncherPage::Instances => "Separate worlds, settings and skins on the one supported version.",
        LauncherPage::Versions => "This native build plays Minecraft Java 26.3; other versions are listed for reference.",
        LauncherPage::Skins => "Your skin comes from your account, or a PNG in import/skins.",
        LauncherPage::Mods => "Component mods need a runtime this build does not carry yet.",
        LauncherPage::Settings => "Video, controls, language and accessibility.",
        LauncherPage::About => "What this build is.",
    };
    ui.text_colored(rgba(c.muted, 1.0), sub);
    ui.set_cursor_pos([pad, 100.0 * dpi]);

    match model.page {
        LauncherPage::Play => play_page(ui, model, fonts, c, dpi, col, pad, out),
        LauncherPage::Instances | LauncherPage::Versions => {
            list_page(ui, model, fonts, c, dpi, col, pad, out)
        }
        LauncherPage::Skins => {
            action_card(ui, fonts, c, dpi, col, pad, "Skins", "Open the skin picker", "Choose your account skin, or drop a PNG into import/skins and load it.", LauncherEvent::OpenSkins, out);
        }
        LauncherPage::Settings => {
            action_card(ui, fonts, c, dpi, col, pad, "Settings", "Open settings", "Video, controls, language and accessibility live in the game's own settings.", LauncherEvent::OpenSettings, out);
        }
        LauncherPage::Mods => {
            info_card(ui, fonts, c, dpi, col, pad, "Mods", "Horizon's component mods need a runtime this build does not carry yet. Modpacks ported to 26.3 run as resource and data packs.");
        }
        LauncherPage::About => about_page(ui, model, fonts, c, dpi, col, pad),
    }
    if rail > 0.0 {
        rail_card(ui, model, fonts, c, dpi, ox + col, oy + 26.0 * dpi, rail - pad);
    }
}

/// The column beside every page: what this build is, in the few lines that are worth
/// reading once and never again. It also keeps a wide screen from being one long card
/// with a desert to its right.
fn rail_card(
    ui: &Ui,
    model: &LauncherModel,
    fonts: &Fonts,
    c: &Css,
    dpi: f32,
    x: f32,
    y: f32,
    width: f32,
) {
    let inner = width - 2.0 * 20.0 * dpi;
    let rows = model.facts.len().max(1) as f32;
    let h = (58.0 + rows * 38.0 + 46.0) * dpi;
    let draw = ui.get_window_draw_list();
    fill(&draw, [x, y], [x + width, y + h], c.panel, 1.0, 14.0);
    stroke(&draw, [x, y], [x + width, y + h], c.rule, 1.0, 14.0, 1.0);
    let _f = ui.push_font(fonts.pixel);
    draw.add_text([x + 20.0 * dpi, y + 18.0 * dpi], rgba(c.muted, 1.0), "THIS BUILD");
    let _g = ui.push_font(fonts.body);
    let mut ry = y + 48.0 * dpi;
    for (key, value) in &model.facts {
        draw.add_text([x + 20.0 * dpi, ry], rgba(c.muted, 1.0), key.as_str());
        draw.add_text(
            [x + 20.0 * dpi, ry + 17.0 * dpi],
            rgba(c.ink, 1.0),
            value.as_str(),
        );
        ry += 38.0 * dpi;
    }
    fill(&draw, [x + 20.0 * dpi, ry + 4.0 * dpi], [x + 20.0 * dpi + inner, ry + 5.0 * dpi], c.rule, 1.0, 0.0);
    draw.add_text(
        [x + 20.0 * dpi, ry + 16.0 * dpi],
        rgba(c.muted, 1.0),
        "Java 26.3 only: the engine is\nbuilt for one version at a time.",
    );
    let _ = ui.dummy([width, h]);
}

fn play_page(
    ui: &Ui,
    model: &LauncherModel,
    fonts: &Fonts,
    c: &Css,
    dpi: f32,
    width: f32,
    pad: f32,
    out: &mut Vec<LauncherEvent>,
) {
    let hero_h = 148.0 * dpi;
    let [x, y] = ui.cursor_screen_pos();
    let w = width - 2.0 * pad;
    let playable = model.playable && !model.instance.is_empty();
    {
        let draw = ui.get_window_draw_list();
        fill(&draw, [x, y], [x + w, y + hero_h], c.panel, 1.0, 14.0);
        stroke(&draw, [x, y], [x + w, y + hero_h], c.rule, 1.0, 14.0, 1.0);
        let _f = ui.push_font(fonts.title);
        draw.add_text(
            [x + 24.0 * dpi, y + 26.0 * dpi],
            rgba(c.ink, 1.0),
            if model.instance.is_empty() { "No instance" } else { model.instance.as_str() },
        );
        let _g = ui.push_font(fonts.body);
        draw.add_text(
            [x + 24.0 * dpi, y + 74.0 * dpi],
            rgba(c.muted, 1.0),
            format!("Minecraft Java {} · native", model.version),
        );
    }
    let btn_w = 168.0 * dpi;
    ui.set_cursor_screen_pos([x + w - btn_w - 24.0 * dpi, y + (hero_h - 44.0 * dpi) / 2.0]);
    {
        let _b = ui.push_style_color(
            StyleColor::Button,
            rgba(if playable { c.face } else { c.button }, 1.0),
        );
        let _bh = ui.push_style_color(
            StyleColor::ButtonHovered,
            rgba(if playable { c.accent } else { c.raised }, 1.0),
        );
        let _ba = ui.push_style_color(StyleColor::ButtonActive, rgba(c.accent, 1.0));
        let _t = ui.push_style_color(
            StyleColor::Text,
            rgba(if playable { c.on_face } else { c.muted }, 1.0),
        );
        let _f = ui.push_font(fonts.pixel);
        // The one button on the front page does the one thing that is missing: play
        // what is installed, install what is not.
        if ui.button_with_size(if playable { "Play" } else { "Install" }, [btn_w, 44.0 * dpi]) {
            out.push(if playable {
                LauncherEvent::Play
            } else {
                LauncherEvent::Install
            });
        }
    }
    if !model.status.is_empty() {
        ui.set_cursor_screen_pos([x + w - btn_w - 24.0 * dpi, y + hero_h - 34.0 * dpi]);
        ui.text_colored(rgba(c.muted, 1.0), model.status.as_str());
    }
    ui.set_cursor_screen_pos([x, y + hero_h + 20.0 * dpi]);

    ui.text_colored(rgba(c.muted, 1.0), "WORLDS");
    ui.dummy([0.0, 6.0 * dpi]);
    if model.rows.is_empty() {
        let [x, y] = ui.cursor_screen_pos();
        let w = width - 2.0 * pad;
        let h = 78.0 * dpi;
        {
            let draw = ui.get_window_draw_list();
            fill(&draw, [x, y], [x + w, y + h], c.panel, 1.0, 12.0);
            stroke(&draw, [x, y], [x + w, y + h], c.rule, 1.0, 12.0, 1.0);
            let _f = ui.push_font(fonts.pixel);
            draw.add_text([x + 20.0 * dpi, y + 18.0 * dpi], rgba(c.ink, 1.0), "No worlds yet");
            let _g = ui.push_font(fonts.body);
            draw.add_text(
                [x + 20.0 * dpi, y + 44.0 * dpi],
                rgba(c.muted, 1.0),
                "Play starts a new one, or make one from Singleplayer.",
            );
        }
        ui.dummy([0.0, h]);
    } else {
        for row in &model.rows {
            row_card(ui, fonts, c, dpi, width, pad, row, out, LauncherEvent::PlayRow);
        }
    }
}

fn list_page(
    ui: &Ui,
    model: &LauncherModel,
    fonts: &Fonts,
    c: &Css,
    dpi: f32,
    width: f32,
    pad: f32,
    out: &mut Vec<LauncherEvent>,
) {
    for row in &model.rows {
        row_card(ui, fonts, c, dpi, width, pad, row, out, if model.page == LauncherPage::Instances { LauncherEvent::PlayRow } else { LauncherEvent::SelectRow });
    }
    if model.page == LauncherPage::Instances {
        ui.dummy([0.0, 6.0 * dpi]);
        let [x, y] = ui.cursor_screen_pos();
        let w = 200.0 * dpi;
        let h = 44.0 * dpi;
        {
            let draw = ui.get_window_draw_list();
            fill(&draw, [x, y], [x + w, y + h], c.button, 1.0, 10.0);
            stroke(&draw, [x, y], [x + w, y + h], c.rule, 1.0, 10.0, 1.0);
            let _f = ui.push_font(fonts.pixel);
            draw.add_text([x + 20.0 * dpi, y + h / 2.0 - 9.0 * dpi], rgba(c.ink, 1.0), "New instance");
        }
        if ui.invisible_button("##new", [w, h]) {
            out.push(LauncherEvent::NewInstance);
        }
    }
    if !model.status.is_empty() {
        ui.dummy([0.0, 10.0 * dpi]);
        ui.text_colored(rgba(c.muted, 1.0), model.status.as_str());
    }
}

fn row_card(
    ui: &Ui,
    fonts: &Fonts,
    c: &Css,
    dpi: f32,
    width: f32,
    pad: f32,
    row: &LauncherRow,
    out: &mut Vec<LauncherEvent>,
    on_row: fn(u32) -> LauncherEvent,
) {
    let [x, y] = ui.cursor_screen_pos();
    let w = width - 2.0 * pad;
    let h = 66.0 * dpi;
    {
        let draw = ui.get_window_draw_list();
        fill(&draw, [x, y], [x + w, y + h], if row.selected { c.raised } else { c.panel }, 1.0, 12.0);
        stroke(&draw, [x, y], [x + w, y + h], c.rule, 1.0, 12.0, 1.0);
        fill(&draw, [x + 14.0 * dpi, y + 15.0 * dpi], [x + 51.0 * dpi, y + h - 15.0 * dpi], c.face, 1.0, 8.0);
        let title_col = if row.enabled { c.ink } else { c.muted };
        let _f = ui.push_font(fonts.pixel);
        draw.add_text([x + 64.0 * dpi, y + 14.0 * dpi], rgba(title_col, 1.0), row.title.as_str());
        draw.add_text([x + 64.0 * dpi, y + 38.0 * dpi], rgba(c.muted, 1.0), row.subtitle.as_str());
        if !row.badge.is_empty() {
            let bw = ui.calc_text_size(row.badge.as_str())[0] + 20.0 * dpi;
            fill(&draw, [x + w - bw - 14.0 * dpi, y + h / 2.0 - 12.0 * dpi], [x + w - 14.0 * dpi, y + h / 2.0 + 12.0 * dpi], c.well, 1.0, 8.0);
            draw.add_text([x + w - bw - 4.0 * dpi, y + h / 2.0 - 8.0 * dpi], rgba(c.muted, 1.0), row.badge.as_str());
        }
    }
    let id = format!("##row{}", row.id);
    if ui.invisible_button(id.as_str(), [w, h]) && row.enabled {
        out.push(on_row(row.id));
    }
}

fn action_card(
    ui: &Ui,
    fonts: &Fonts,
    c: &Css,
    dpi: f32,
    width: f32,
    pad: f32,
    title: &str,
    button: &str,
    body: &str,
    event: LauncherEvent,
    out: &mut Vec<LauncherEvent>,
) {
    let _ = title;
    let [x, y] = ui.cursor_screen_pos();
    let w = width - 2.0 * pad;
    let h = 150.0 * dpi;
    {
        let draw = ui.get_window_draw_list();
        fill(&draw, [x, y], [x + w, y + h], c.panel, 1.0, 14.0);
        stroke(&draw, [x, y], [x + w, y + h], c.rule, 1.0, 14.0, 1.0);
        draw.add_text([x + 24.0 * dpi, y + 24.0 * dpi], rgba(c.muted, 1.0), body);
    }
    ui.set_cursor_screen_pos([x + 24.0 * dpi, y + h - 56.0 * dpi]);
    {
        let _b = ui.push_style_color(StyleColor::Button, rgba(c.button, 1.0));
        let _t = ui.push_style_color(StyleColor::Text, rgba(c.ink, 1.0));
        let _f = ui.push_font(fonts.pixel);
        if ui.button_with_size(button, [220.0 * dpi, 40.0 * dpi]) {
            out.push(event);
        }
    }
    ui.set_cursor_screen_pos([x, y + h + 20.0 * dpi]);
}

fn info_card(
    ui: &Ui,
    fonts: &Fonts,
    c: &Css,
    dpi: f32,
    width: f32,
    pad: f32,
    title: &str,
    body: &str,
) {
    let _ = (fonts, title);
    let [x, y] = ui.cursor_screen_pos();
    let w = width - 2.0 * pad;
    let h = 120.0 * dpi;
    {
        let draw = ui.get_window_draw_list();
        fill(&draw, [x, y], [x + w, y + h], c.panel, 1.0, 14.0);
        stroke(&draw, [x, y], [x + w, y + h], c.rule, 1.0, 14.0, 1.0);
        draw.add_text([x + 24.0 * dpi, y + 24.0 * dpi], rgba(c.ink_soft, 1.0), body);
    }
    ui.dummy([0.0, h]);
}

fn about_page(ui: &Ui, model: &LauncherModel, fonts: &Fonts, c: &Css, dpi: f32, width: f32, pad: f32) {
    let _ = fonts;
    for (key, value) in &model.facts {
        let [x, y] = ui.cursor_screen_pos();
        let w = width - 2.0 * pad;
        let h = 54.0 * dpi;
        {
            let draw = ui.get_window_draw_list();
            fill(&draw, [x, y], [x + w, y + h], c.panel, 1.0, 10.0);
            stroke(&draw, [x, y], [x + w, y + h], c.rule, 1.0, 10.0, 1.0);
            draw.add_text([x + 20.0 * dpi, y + 16.0 * dpi], rgba(c.muted, 1.0), key.as_str());
            draw.add_text([x + 160.0 * dpi, y + 16.0 * dpi], rgba(c.ink, 1.0), value.as_str());
        }
        ui.dummy([0.0, h + 6.0 * dpi]);
    }
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
    out: &mut Vec<LauncherEvent>,
) {
    let w = (460.0 * dpi).min(width - 32.0 * dpi);
    let h = 200.0 * dpi;
    let x = (width - w) / 2.0;
    let y = 60.0 * dpi;
    let keep = w - 44.0 * dpi;
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
                draw.add_text(
                    [x + 22.0 * dpi, y + 20.0 * dpi],
                    rgba(c.ink, 1.0),
                    "Player name",
                );
                draw.add_text(
                    [x + w - 22.0 * dpi - 148.0 * dpi, y + 24.0 * dpi],
                    rgba(c.muted, 1.0),
                    "A-Z a-z 0-9 _",
                );
                let _g = ui.push_font(fonts.body);
                draw.add_text(
                    [x + 22.0 * dpi, y + 48.0 * dpi],
                    rgba(c.muted, 1.0),
                    format!("Signed in as {}", if model.account.is_empty() { "Player" } else { model.account.as_str() }),
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
                    x + 22.0 * dpi,
                    y + 68.0 * dpi,
                    keep,
                    &model.draft,
                );
            }

            ui.set_cursor_screen_pos([x + 22.0 * dpi, y + h - 54.0 * dpi]);
            {
                let _b = ui.push_style_color(StyleColor::Button, rgba(c.face, 1.0));
                let _hover = ui.push_style_color(StyleColor::ButtonHovered, rgba(c.accent, 1.0));
                let _a = ui.push_style_color(StyleColor::ButtonActive, rgba(c.accent, 1.0));
                let _t = ui.push_style_color(StyleColor::Text, rgba(c.on_face, 1.0));
                let _f = ui.push_font(fonts.pixel);
                if ui.button_with_size("Save", [button, 38.0 * dpi]) {
                    out.push(LauncherEvent::SetAccount(model.draft.clone()));
                    model.editing = false;
                }
            }
            ui.set_cursor_screen_pos([x + w - 22.0 * dpi - button, y + h - 54.0 * dpi]);
            {
                let _b = ui.push_style_color(StyleColor::Button, rgba(c.button, 1.0));
                let _t = ui.push_style_color(StyleColor::Text, rgba(c.muted, 1.0));
                let _f = ui.push_font(fonts.pixel);
                if ui.button_with_size("Cancel", [button, 38.0 * dpi]) {
                    model.editing = false;
                }
            }
            // Away from the card either way is a way out: the bands above and below,
            // which is where the keyboard and the rest of the launcher are.
            for (i, (top, bottom)) in [(0.0, y), (y + h, height)].into_iter().enumerate() {
                ui.set_cursor_screen_pos([0.0, top]);
                let id = format!("##away{i}");
                if ui.invisible_button(id.as_str(), [width, bottom - top]) {
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
    let caret = x + 14.0 * dpi + ui.calc_text_size(draft)[0];
    fill(&draw, [x, y], [x + width, y + 50.0 * dpi], c.well, 1.0, 10.0);
    fill(&draw, [x, y + 49.0 * dpi], [x + width, y + 51.0 * dpi], c.accent, 1.0, 0.0);
    let _f = ui.push_font(fonts.body);
    if draft.is_empty() {
        draw.add_text([x + 14.0 * dpi, y + 15.0 * dpi], rgba(c.muted, 1.0), "New name");
    } else {
        draw.add_text([x + 14.0 * dpi, y + 15.0 * dpi], rgba(c.ink, 1.0), draft);
    }
    fill(
        &draw,
        [caret.min(x + width - 3.0 * dpi), y + 13.0 * dpi],
        [caret.min(x + width - 3.0 * dpi) + 2.0 * dpi, y + 37.0 * dpi],
        c.accent,
        1.0,
        1.0,
    );
}
