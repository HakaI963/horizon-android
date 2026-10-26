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
//! face from [`horizon_theme`], the same tokens the website (playhorizon.dev) and
//! the desktop launcher are built from — so the phone, the site and the desktop
//! read as one product.

use imgui::{Condition, FontConfig, FontId, FontSource, StyleColor, Ui, WindowFlags};

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
}

/// What the player asked for, for the platform to carry out.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum LauncherEvent {
    Goto(LauncherPage),
    /// The big Play button on the Play page.
    Play,
    /// A row that launches something (a world, an instance).
    PlayRow(u32),
    /// A row that only becomes the chosen one (a version, a skin).
    SelectRow(u32),
    NewInstance,
    Install,
    EditAccount,
    OpenSettings,
    OpenSkins,
    Quit,
}

/// The `Blocks` palette, pulled straight from the shared design tokens so the
/// phone matches the website and the desktop launcher. Colours are `0xRRGGBB`.
pub struct Css {
    pub page: u32,
    pub side: u32,
    pub panel: u32,
    pub raised: u32,
    pub button: u32,
    pub well: u32,
    pub rule: u32,
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
    ink: B.ink,
    ink_soft: B.ink_soft,
    muted: B.muted,
    accent: B.accent,
    face: B.face,
    on_face: B.on_face,
};

fn rgb(c: u32) -> [f32; 3] {
    [
        ((c >> 16) & 0xff) as f32 / 255.0,
        ((c >> 8) & 0xff) as f32 / 255.0,
        (c & 0xff) as f32 / 255.0,
    ]
}

fn rgba(c: u32, a: f32) -> [f32; 4] {
    let [r, g, b] = rgb(c);
    [r, g, b, a]
}

/// The faces the launcher draws with: Geist for body, Pixelify Sans for the
/// blocky headings, navigation and buttons.
pub struct Fonts {
    pub body: FontId,
    pub pixel: FontId,
    pub title: FontId,
}

/// Load the fonts and set the `Blocks` theme. Called once, where the fonts are
/// added. The first font added becomes the default, so Geist is what the touch
/// buttons' legends and any un-pushed text draw with.
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
        size_pixels: 16.0 * dpi,
        config: Some(FontConfig::default()),
    }]);
    let title = imgui.fonts().add_font(&[FontSource::TtfData {
        data: horizon_theme::fonts::PIXEL_BOLD,
        size_pixels: 30.0 * dpi,
        config: Some(FontConfig::default()),
    }]);

    let c = &THEME;
    let style = imgui.style_mut();
    style.window_rounding = horizon_theme::RADIUS_PLATE;
    style.frame_rounding = horizon_theme::RADIUS;
    style.scrollbar_rounding = horizon_theme::RADIUS;
    style.grab_rounding = horizon_theme::RADIUS;
    style.window_border_size = 0.0;
    style.frame_border_size = 0.0;
    style.scrollbar_size = 10.0 * dpi;
    style.item_spacing = [10.0 * dpi, 8.0 * dpi];
    style.window_padding = [16.0 * dpi, 16.0 * dpi];
    style.frame_padding = [10.0 * dpi, 7.0 * dpi];
    let colors = &mut style.colors;
    colors[StyleColor::Text as usize] = rgba(c.ink, 1.0);
    colors[StyleColor::TextDisabled as usize] = rgba(c.muted, 1.0);
    colors[StyleColor::TextSelectedBg as usize] = rgba(c.accent, 0.6);
    colors[StyleColor::WindowBg as usize] = rgba(c.page, 1.0);
    colors[StyleColor::ChildBg as usize] = [0.0, 0.0, 0.0, 0.0];
    colors[StyleColor::PopupBg as usize] = rgba(c.panel, 1.0);
    colors[StyleColor::Border as usize] = rgba(c.rule, 1.0);
    colors[StyleColor::FrameBg as usize] = rgba(c.well, 1.0);
    colors[StyleColor::FrameBgHovered as usize] = rgba(c.raised, 1.0);
    colors[StyleColor::FrameBgActive as usize] = rgba(c.raised, 1.0);
    colors[StyleColor::Button as usize] = rgba(c.button, 1.0);
    colors[StyleColor::ButtonHovered as usize] = rgba(c.raised, 1.0);
    colors[StyleColor::ButtonActive as usize] = rgba(c.accent, 1.0);
    colors[StyleColor::Header as usize] = rgba(c.raised, 1.0);
    colors[StyleColor::HeaderHovered as usize] = rgba(c.raised, 1.0);
    colors[StyleColor::HeaderActive as usize] = rgba(c.accent, 1.0);
    colors[StyleColor::Separator as usize] = rgba(c.rule, 1.0);
    colors[StyleColor::ScrollbarBg as usize] = rgba(c.page, 0.0);
    colors[StyleColor::ScrollbarGrab as usize] = rgba(c.rule, 1.0);
    colors[StyleColor::ScrollbarGrabHovered as usize] = rgba(c.muted, 1.0);
    colors[StyleColor::ScrollbarGrabActive as usize] = rgba(c.ink, 1.0);
    colors[StyleColor::SliderGrab as usize] = rgba(c.accent, 1.0);
    colors[StyleColor::SliderGrabActive as usize] = rgba(c.ink, 1.0);
    colors[StyleColor::CheckMark as usize] = rgba(c.accent, 1.0);
    Fonts { body, pixel, title }
}

/// The whole launcher, edge to edge. Events the player raises go into `out`.
pub fn draw(
    ui: &Ui,
    model: &LauncherModel,
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
                | WindowFlags::NO_FOCUS_ON_APPEARING
                | WindowFlags::NO_BRING_TO_FRONT_ON_FOCUS,
        )
        .position([0.0, 0.0], Condition::Always)
        .size([width, height], Condition::Always)
        .build(|| {
            let sidebar = (220.0 * dpi).min(width * 0.4);

            let _side = ui.push_style_color(StyleColor::ChildBg, rgba(c.side, 1.0));
            let _ = ui
                .child_window("##sidebar")
                .size([sidebar, -1.0])
                .build(|| {
                    ui.dummy([0.0, 12.0 * dpi]);
                    {
                        let _f = ui.push_font(fonts.title);
                        let _t = ui.push_style_color(StyleColor::Text, rgba(c.ink, 1.0));
                        ui.text("HORIZON");
                    }
                    ui.dummy([0.0, 16.0 * dpi]);
                    for page in PAGES {
                        let current = page == model.page;
                        let _b = ui.push_style_color(
                            StyleColor::Button,
                            rgba(if current { c.raised } else { c.side }, 1.0),
                        );
                        let _t = ui.push_style_color(
                            StyleColor::Text,
                            rgba(if current { c.ink } else { c.muted }, 1.0),
                        );
                        let _f = ui.push_font(fonts.pixel);
                        if ui.button_with_size(page.label(), [-1.0, 42.0 * dpi]) {
                            log::info!("launcher: nav {page:?} clicked");
                            out.push(LauncherEvent::Goto(page));
                        }
                    }
                });

            let content_w = width - sidebar;
            ui.set_cursor_screen_pos([sidebar, 0.0]);
            let _content = ui.push_style_color(StyleColor::ChildBg, rgba(c.page, 1.0));
            let _ = ui
                .child_window("##content")
                .size([content_w, -1.0])
                .build(|| {
                    topbar(ui, model, fonts, c, out);
                    ui.separator();
                    match model.page {
                        LauncherPage::Play => play(ui, model, fonts, c, dpi, out),
                        LauncherPage::Instances => list(
                            ui,
                            model,
                            fonts,
                            c,
                            dpi,
                            out,
                            "Instances",
                            "Separate worlds, settings and skins on the one supported version.",
                            LauncherEvent::PlayRow,
                            Some(LauncherEvent::NewInstance),
                        ),
                        LauncherPage::Versions => list(
                            ui,
                            model,
                            fonts,
                            c,
                            dpi,
                            out,
                            "Versions",
                            "This native build plays Minecraft Java 26.3. Other versions need their own engine port and are shown for reference.",
                            LauncherEvent::SelectRow,
                            None,
                        ),
                        LauncherPage::Skins => {
                            heading(ui, fonts, c, "Skins");
                            ui.text_wrapped(
                                "Choose the account skin, or load a PNG from the import/skins folder.",
                            );
                            ui.dummy([0.0, 8.0 * dpi]);
                            action_button(ui, fonts, c, "Open skin picker", dpi, out, LauncherEvent::OpenSkins);
                        }
                        LauncherPage::Mods => {
                            heading(ui, fonts, c, "Mods");
                            ui.text_wrapped(
                                "Horizon's own component mods need a runtime this build does not carry yet. Modpacks ported to 26.3 run as resource and data packs.",
                            );
                        }
                        LauncherPage::Settings => {
                            heading(ui, fonts, c, "Settings");
                            ui.text_wrapped("Video, controls, language and accessibility.");
                            ui.dummy([0.0, 8.0 * dpi]);
                            action_button(ui, fonts, c, "Open settings", dpi, out, LauncherEvent::OpenSettings);
                        }
                        LauncherPage::About => {
                            heading(ui, fonts, c, "About");
                            for (key, value) in &model.facts {
                                let _t = ui.push_style_color(StyleColor::Text, rgba(c.muted, 1.0));
                                ui.text(format!("{key}"));
                                ui.same_line();
                                ui.text(value.as_str());
                            }
                            ui.dummy([0.0, 6.0 * dpi]);
                            ui.text_wrapped(
                                "Horizon — a native Minecraft Java client in Rust. Not affiliated with Mojang or Microsoft.",
                            );
                        }
                    }
                });
        });
}

fn topbar(ui: &Ui, model: &LauncherModel, fonts: &Fonts, c: &Css, out: &mut Vec<LauncherEvent>) {
    let _f = ui.push_font(fonts.pixel);
    ui.text(model.page.label());
    if !model.account.is_empty() {
        ui.same_line();
        let _t = ui.push_style_color(StyleColor::Text, rgba(c.muted, 1.0));
        if ui.button(format!("{}  ·  Rename", model.account).as_str()) {
            out.push(LauncherEvent::EditAccount);
        }
        ui.same_line();
        if ui.button("Quit") {
            out.push(LauncherEvent::Quit);
        }
    }
}

fn play(
    ui: &Ui,
    model: &LauncherModel,
    fonts: &Fonts,
    c: &Css,
    dpi: f32,
    out: &mut Vec<LauncherEvent>,
) {
    ui.dummy([0.0, 10.0 * dpi]);
    {
        let _f = ui.push_font(fonts.title);
        ui.text(if model.instance.is_empty() {
            "No instance"
        } else {
            model.instance.as_str()
        });
    }
    ui.text_colored(
        rgba(c.muted, 1.0),
        format!("Minecraft Java {} · native", model.version),
    );
    ui.dummy([0.0, 16.0 * dpi]);

    let playable = model.playable && !model.instance.is_empty();
    let clicked = {
        let _f = ui.push_font(fonts.pixel);
        let _b = ui.push_style_color(
            StyleColor::Button,
            rgba(if playable { c.face } else { c.button }, 1.0),
        );
        let _bh = ui.push_style_color(
            StyleColor::ButtonHovered,
            rgba(if playable { c.face } else { c.raised }, 1.0),
        );
        let _ba = ui.push_style_color(StyleColor::ButtonActive, rgba(c.accent, 1.0));
        let _t = ui.push_style_color(
            StyleColor::Text,
            rgba(if playable { c.on_face } else { c.muted }, 1.0),
        );
        ui.button_with_size(
            if playable { "Play" } else { "Not installed" },
            [-1.0, 54.0 * dpi],
        )
    };
    if clicked && playable {
        out.push(LauncherEvent::Play);
    }
    ui.dummy([0.0, 18.0 * dpi]);

    if !model.status.is_empty() {
        ui.text_colored(rgba(c.muted, 1.0), model.status.as_str());
    }
    if let Some(p) = model.progress {
        ui.text_colored(rgba(c.muted, 1.0), format!("{:.0}%", p * 100.0));
    }

    if !model.rows.is_empty() {
        ui.dummy([0.0, 10.0 * dpi]);
        ui.text_colored(rgba(c.muted, 1.0), "Recent worlds");
        for row in &model.rows {
            list_row(ui, row, fonts, c, dpi, out, LauncherEvent::PlayRow);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn list(
    ui: &Ui,
    model: &LauncherModel,
    fonts: &Fonts,
    c: &Css,
    dpi: f32,
    out: &mut Vec<LauncherEvent>,
    title: &str,
    blurb: &str,
    on_row: fn(u32) -> LauncherEvent,
    action: Option<LauncherEvent>,
) {
    heading(ui, fonts, c, title);
    ui.text_wrapped(blurb);
    ui.dummy([0.0, 8.0 * dpi]);
    for row in &model.rows {
        list_row(ui, row, fonts, c, dpi, out, on_row);
    }
    if let Some(event) = action {
        ui.dummy([0.0, 8.0 * dpi]);
        action_button(ui, fonts, c, "New instance", dpi, out, event);
    }
    if !model.status.is_empty() {
        ui.dummy([0.0, 8.0 * dpi]);
        ui.text_colored(rgba(c.muted, 1.0), model.status.as_str());
    }
}

fn heading(ui: &Ui, fonts: &Fonts, c: &Css, text: &str) {
    let _f = ui.push_font(fonts.title);
    ui.text_colored(rgba(c.ink, 1.0), text);
}

/// A secondary, full-width-ish action button in the panel style.
fn action_button(
    ui: &Ui,
    fonts: &Fonts,
    c: &Css,
    label: &str,
    dpi: f32,
    out: &mut Vec<LauncherEvent>,
    event: LauncherEvent,
) {
    let _b = ui.push_style_color(StyleColor::Button, rgba(c.button, 1.0));
    let _t = ui.push_style_color(StyleColor::Text, rgba(c.ink, 1.0));
    let _f = ui.push_font(fonts.pixel);
    if ui.button_with_size(label, [220.0 * dpi, 40.0 * dpi]) {
        out.push(event);
    }
}

fn list_row(
    ui: &Ui,
    row: &LauncherRow,
    fonts: &Fonts,
    c: &Css,
    dpi: f32,
    out: &mut Vec<LauncherEvent>,
    on_row: fn(u32) -> LauncherEvent,
) {
    let mut label = if row.subtitle.is_empty() {
        row.title.clone()
    } else {
        format!("{}  ·  {}", row.title, row.subtitle)
    };
    if !row.badge.is_empty() {
        label = format!("{label}      {}", row.badge);
    }
    let _b = ui.push_style_color(
        StyleColor::Button,
        rgba(if row.selected { c.raised } else { c.panel }, 1.0),
    );
    let _t = ui.push_style_color(
        StyleColor::Text,
        rgba(if row.enabled { c.ink } else { c.muted }, 1.0),
    );
    let _f = ui.push_font(fonts.pixel);
    let clicked = ui.button_with_size(label.as_str(), [-1.0, 50.0 * dpi]);
    if clicked && row.enabled {
        out.push(on_row(row.id));
    }
}
