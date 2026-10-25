//! The imgui layer: the buttons the player presses, the panel that changes them,
//! and the renderer that puts both over the swapchain.
//!
//! Two kinds of imgui live here, and the difference is why. The settings panel is
//! ordinary imgui — windows, sliders, checkboxes — driven by the one pointer imgui
//! knows, which is the first finger down. The HUD is not: a HUD has to answer to
//! three fingers at once and a mouse has only one, so the buttons are rectangles
//! this file places and hit-tests itself, drawn into an imgui window's draw list so
//! that they still arrive in imgui's one submission with its one font atlas. While
//! the layout is being edited the same rectangles become imgui windows, because
//! dragging a button to where your thumb actually reaches is the one thing a phone
//! cannot do without widgets.

use std::error::Error;
use std::path::{Path, PathBuf};
use std::time::Instant;

use horizon_controls::{Action, Hud, Layout, Rect};
use imgui::{
    Condition, ConfigFlags, Context, DrawListMut, FontConfig, FontSource, ImColor32, MouseButton,
    Ui, WindowFlags,
};

/// The button at rest: dark enough to see through, bright enough to find.
const IDLE: [u8; 3] = [12, 16, 24];
/// The button a finger is on. Orange, because it has to read against a world that is
/// mostly green and blue, and because it is the colour the game uses for what is
/// selected.
const PRESSED: [u8; 3] = [255, 138, 34];
/// The outline at rest, and the outline under a finger.
const EDGE: [u8; 3] = [188, 202, 218];
const EDGE_ACTIVE: [u8; 3] = [255, 214, 150];
/// The legend.
const LEGEND: [u8; 3] = [238, 242, 248];

/// The finger imgui is seeing. It only exists while something on screen can be
/// clicked: a pointer fed during play would report a hover nobody asked for.
struct Pointer {
    /// Which finger this is, so that a second one does not move the cursor.
    finger: Option<u64>,
    position: [f32; 2],
    down: bool,
}

impl Pointer {
    fn idle() -> Self {
        Self {
            finger: None,
            position: [0.0, 0.0],
            down: false,
        }
    }
}

/// The overlay, the layout it draws and the device it draws on.
///
/// The imgui context outlives the renderer, because `Context::create` panics if a
/// context already exists and the activity loses its surface every time it is
/// backgrounded. So the context and the layout are made once, and only the wgpu
/// objects come and go with the window.
pub struct Overlay {
    imgui: Context,
    renderer: Option<horizon_imgui::Renderer>,
    pub hud: Hud,
    /// Where the layout is read from and written to.
    path: PathBuf,
    panel: bool,
    /// The panel as it was drawn last frame. A finger inside it belongs to the panel
    /// and to nothing else, and the only place to learn that from is the window
    /// imgui has just laid out.
    panel_rect: Option<Rect>,
    pointer: Pointer,
    /// One style and one font size for the life of the process, decided from the
    /// first screen the activity gets. Rebuilding a font atlas mid-session to chase a
    /// rotation would cost a texture upload on a phone that is already rotating, and
    /// the layout scales with the screen on its own.
    dpi: f32,
    fonts_added: bool,
    show_status: bool,
    frame_started: Instant,
}

impl Overlay {
    /// An overlay that has not seen a device yet. The context is created here, once
    /// and for the whole process.
    pub fn new(layout: Layout, path: PathBuf) -> Self {
        let mut imgui = Context::create();
        {
            let io = imgui.io_mut();
            // A finger is not a mouse: imgui should activate a widget when the pointer
            // comes up rather than when it goes down, and should not ask the platform
            // for a cursor there is no way to show.
            io.config_flags |= ConfigFlags::IS_TOUCH_SCREEN | ConfigFlags::NO_MOUSE_CURSOR_CHANGE;
        }
        Self {
            imgui,
            renderer: None,
            hud: Hud::new(layout),
            path,
            panel: false,
            panel_rect: None,
            pointer: Pointer::idle(),
            dpi: 1.0,
            fonts_added: false,
            show_status: true,
            frame_started: Instant::now(),
        }
    }

    /// The same, with the layout read from disk if someone has saved one.
    pub fn load(path: PathBuf) -> Self {
        let layout = Layout::load(&path).unwrap_or_default();
        Self::new(layout, path)
    }

    /// The device is here: choose the text size and the style from the screen it has
    /// been given, and hand imgui a pipeline that writes this swapchain's format.
    pub fn attach(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        width: u32,
        height: u32,
    ) {
        if self.renderer.is_none() {
            // A phone and a tablet want different text; the short edge is the one that
            // decides, because it is the axis a thumb crosses.
            self.dpi = (width.min(height) as f32 / 720.0).clamp(0.8, 2.5);
            self.imgui.style_mut().scale_all_sizes(self.dpi);
            if !self.fonts_added {
                self.imgui.fonts().add_font(&[FontSource::DefaultFontData {
                    config: Some(FontConfig {
                        size_pixels: (13.0 * self.dpi).max(13.0),
                        pixel_snap_h: true,
                        ..Default::default()
                    }),
                }]);
                self.fonts_added = true;
            }
            let mut config = horizon_imgui::RendererConfig::new_srgb();
            // The colors imgui hands over are already in the space the game writes:
            // the swapchain is a non-sRGB target by design, so the shader must not
            // encode them a second time. The output format is the attachment's own.
            config.texture_format = format;
            self.renderer = Some(horizon_imgui::Renderer::new(
                &mut self.imgui,
                device,
                queue,
                config,
            ));
            log::info!(
                "overlay: imgui draws at {width}x{height} scale {}, {} buttons, {}",
                self.dpi,
                self.hud.layout.controls.len(),
                self.path.display()
            );
        }
        self.resize(width, height);
    }

    /// The surface is gone; the pipeline, the buffers and the font texture go with
    /// it. The context and the layout stay, and no finger is holding anything.
    pub fn detach(&mut self) {
        self.renderer = None;
        self.pointer = Pointer::idle();
        self.hud.release_all();
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        let io = self.imgui.io_mut();
        io.display_size = [width as f32, height as f32];
        // Everything here is in the framebuffer's own pixels: touch reports them, the
        // test card is placed in them, and the swapchain is that size.
        io.display_framebuffer_scale = [1.0, 1.0];
    }

    /// Whether something on screen wants the finger: the panel, or the layout while
    /// it is being dragged around.
    fn interactive(&self) -> bool {
        self.panel || self.hud.edit
    }

    /// A finger went down. It goes to the layout unless the panel already has it, and
    /// to imgui when there is a window to click.
    pub fn finger_down(&mut self, id: u64, x: f32, y: f32, width: f32, height: f32) {
        let inside_panel = self.panel_rect.is_some_and(|rect| rect.contains(x, y));
        if self.hud.edit || !inside_panel {
            self.hud.down(id, x, y, width, height);
        }
        if self.interactive() && (self.pointer.finger.is_none() || inside_panel) {
            let was_down = self.pointer.down;
            self.pointer = Pointer {
                finger: Some(id),
                position: [x, y],
                down: true,
            };
            if !was_down {
                self.imgui.io_mut().add_mouse_button_event(MouseButton::Left, true);
            }
        }
    }

    pub fn finger_move(&mut self, id: u64, x: f32, y: f32) {
        self.hud.drag(id, x, y);
        if self.pointer.finger == Some(id) {
            self.pointer.position = [x, y];
        }
    }

    pub fn finger_up(&mut self, id: u64) {
        self.hud.up(id);
        if self.pointer.finger == Some(id) {
            self.pointer.finger = None;
            self.pointer.down = false;
            // imgui has to be told the button came up, or every widget stays
            // half-pressed until the next finger arrives.
            self.imgui
                .io_mut()
                .add_mouse_button_event(MouseButton::Left, false);
        }
    }

    /// The presses of the frame, drawn, and the ones the platform layer has to hand
    /// to the game. What belongs to the overlay it keeps: a tap on MENU opens the
    /// panel and is never a keystroke.
    pub fn frame(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pass: &mut wgpu::RenderPass<'_>,
        width: f32,
        height: f32,
        status: &str,
    ) -> Result<Vec<(Action, bool)>, Box<dyn Error>> {
        let edges = self.hud.take_edges();
        for (action, down) in edges.iter().filter(|(action, _)| *action == &Action::Menu) {
            if *down {
                self.panel = !self.panel;
                log::info!(
                    "overlay: the settings panel is {}",
                    if self.panel { "open" } else { "closed" }
                );
            }
        }
        let game: Vec<(Action, bool)> = edges
            .into_iter()
            .filter(|(action, _)| !action.is_overlay())
            .collect();

        // One borrow of `self`, every field it needs: the renderer and the context are
        // two fields of the same struct, and imgui holds them both at once.
        let Overlay {
            imgui,
            hud,
            path,
            panel,
            panel_rect,
            pointer,
            dpi,
            show_status,
            frame_started,
            renderer,
            ..
        } = self;
        let Some(renderer) = renderer.as_mut() else {
            return Err("the overlay has no device to draw on".into());
        };

        {
            let io = imgui.io_mut();
            io.update_delta_time(frame_started.elapsed());
            *frame_started = Instant::now();
            io.display_size = [width, height];
            if pointer.finger.is_some() {
                io.add_mouse_pos_event(pointer.position);
            }
        }

        // Everything the panel and the editor can change is taken out of the layout
        // into a local first: the closure that draws a window holds the borrow imgui
        // just gave us, and `self` is not in it to be borrowed twice.
        let mut edit = hud.edit;
        let mut opacity = hud.layout.opacity;
        let mut scale = hud.layout.scale;
        let mut sensitivity = hud.layout.look_sensitivity;
        let mut drag_to_look = hud.layout.drag_to_look;
        let mut save = false;
        let mut reset = false;
        let mut close = false;
        let buttons = hud.buttons(width, height);
        let held = hud.pressed_actions();
        let mut moved = Vec::new();
        let mut panel_now = None;

        {
            let ui = imgui.new_frame();
            if *panel {
                panel_now = settings(
                    ui,
                    status,
                    &mut scale,
                    &mut opacity,
                    &mut sensitivity,
                    &mut drag_to_look,
                    &mut edit,
                    show_status,
                    &mut save,
                    &mut reset,
                    &mut close,
                    *dpi,
                );
            }
            if edit {
                moved = editor(ui, &buttons);
            } else {
                hud_buttons(
                    ui,
                    &buttons,
                    &held,
                    opacity,
                    *dpi,
                    *show_status,
                    status,
                    width,
                    height,
                );
            }
        }

        hud.layout.scale = scale;
        hud.layout.opacity = opacity;
        hud.layout.look_sensitivity = sensitivity;
        hud.layout.drag_to_look = drag_to_look;
        hud.edit = edit;
        for (action, center) in moved {
            hud.move_button(action, center, width, height);
        }
        if reset {
            hud.layout = Layout::default();
            log::info!("overlay: the layout is the one the build ships with");
        }
        if save {
            hud.save(path);
        }
        if close {
            *panel = false;
        }
        *panel_rect = panel_now;

        let draw_data = imgui.render();
        renderer
            .render(draw_data, queue, device, pass)
            .map_err(|error| format!("imgui: {error}"))?;
        Ok(game)
    }

    pub fn hud(&self) -> &Hud {
        &self.hud
    }

    pub fn panel_open(&self) -> bool {
        self.panel
    }

    /// Where the layout lives, for a message that has to name it.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// One button: its body, its edge and its legend. The body exists to aim at and the
/// legend to read, so the text keeps a floor of its own under the layout's opacity —
/// a see-through "MINE" is a button you cannot find again.
fn paint(draw: &DrawListMut<'_>, ui: &Ui, action: Action, rect: Rect, pressed: bool, opacity: f32) {
    let min = [rect.x, rect.y];
    let max = [rect.x + rect.width, rect.y + rect.height];
    let fill = if pressed { PRESSED } else { IDLE };
    let edge = if pressed { EDGE_ACTIVE } else { EDGE };
    let rounding = rect.rounding();
    draw.add_rect(min, max, rgba(fill, opacity))
        .filled(true)
        .rounding(rounding)
        .build();
    draw.add_rect(min, max, rgba(edge, opacity * if pressed { 1.0 } else { 0.75 }))
        .rounding(rounding)
        .thickness(2.0)
        .build();
    let text = action.text();
    let [width, height] = ui.calc_text_size(&text);
    let [center_x, center_y] = rect.center();
    draw.add_text(
        [center_x - width / 2.0, center_y - height / 2.0],
        rgba(LEGEND, opacity + 0.35),
        &text,
    );
}

/// A style color at a fraction of its own strength. imgui's colors are bytes, so the
/// layout's opacity is a float that has to be told where it stops.
fn rgba(color: [u8; 3], alpha: f32) -> ImColor32 {
    let [red, green, blue] = color;
    ImColor32::from_rgba(red, green, blue, (255.0 * alpha.clamp(0.0, 1.0)) as u8)
}

/// The HUD while it is being used: one window the size of the screen that takes no
/// input at all, so that imgui is the painter and never the player.
fn hud_buttons(
    ui: &Ui,
    buttons: &[(Action, Rect)],
    held: &[Action],
    opacity: f32,
    dpi: f32,
    show_status: bool,
    status: &str,
    width: f32,
    height: f32,
) {
    ui.window("Horizon controls")
        .flags(
            WindowFlags::NO_BACKGROUND
                | WindowFlags::NO_DECORATION
                | WindowFlags::NO_MOVE
                | WindowFlags::NO_NAV
                | WindowFlags::NO_FOCUS_ON_APPEARING
                | WindowFlags::NO_BRING_TO_FRONT_ON_FOCUS,
        )
        .mouse_inputs(false)
        .position([0.0, 0.0], Condition::Always)
        .size([width, height], Condition::Always)
        .build(|| {
            let draw = ui.get_window_draw_list();
            for (action, rect) in buttons {
                paint(&draw, ui, *action, *rect, held.contains(action), opacity);
            }
            if show_status {
                // The line a frame dump reads instead of logcat: what the loop is
                // doing, and what the screen is answering.
                draw.add_text(
                    [8.0 * dpi, (height - 26.0 * dpi).max(0.0)],
                    rgba(LEGEND, 1.0),
                    status,
                );
            }
        });
}

/// The HUD while it is being edited: every button is a window of its own, and a
/// window with no title bar is one you can drag by its body. The positions come back
/// out of imgui, because the point of the exercise is that the player, not the build,
/// decides where a button is.
fn editor(ui: &Ui, buttons: &[(Action, Rect)]) -> Vec<(Action, [f32; 2])> {
    let mut moved = Vec::new();
    for (action, rect) in buttons {
        let label = format!("##{}", action.text());
        ui.window(&label)
            .flags(
                WindowFlags::NO_RESIZE
                    | WindowFlags::NO_SCROLLBAR
                    | WindowFlags::NO_TITLE_BAR
                    | WindowFlags::NO_COLLAPSE
                    | WindowFlags::NO_NAV
                    | WindowFlags::NO_FOCUS_ON_APPEARING,
            )
            .position([rect.x, rect.y], Condition::Always)
            .size([rect.width, rect.height], Condition::Always)
            .bg_alpha(0.2)
            .movable(true)
            .build(|| {
                let draw = ui.get_window_draw_list();
                paint(&draw, ui, *action, *rect, false, 1.0);
                let [x, y] = ui.window_pos();
                let [width, height] = ui.window_size();
                moved.push((*action, [x + width / 2.0, y + height / 2.0]));
            });
    }
    moved
}

/// The settings panel. Everything a player can reach without a file manager and
/// without this repository is behind one of these widgets.
fn settings(
    ui: &Ui,
    status: &str,
    scale: &mut f32,
    opacity: &mut f32,
    sensitivity: &mut f32,
    drag_to_look: &mut bool,
    edit: &mut bool,
    show_status: &mut bool,
    save: &mut bool,
    reset: &mut bool,
    close: &mut bool,
    dpi: f32,
) -> Rect {
    let mut rect = Rect {
        x: 0.0,
        y: 0.0,
        width: 0.0,
        height: 0.0,
    };
    ui.window("Horizon")
        .position([12.0 * dpi, 12.0 * dpi], Condition::Always)
        .flags(WindowFlags::NO_COLLAPSE | WindowFlags::NO_FOCUS_ON_APPEARING)
        .build(|| {
            ui.text(status);
            ui.separator();
            if ui.checkbox("Move the buttons", edit) {
                log::info!(
                    "overlay: editing the layout is {}",
                    if *edit { "on" } else { "off" }
                );
            }
            ui.slider("Button size", 0.6, 1.8, scale);
            ui.slider("Opacity", 0.08, 1.0, opacity);
            ui.slider("Look sensitivity", 1.0, 30.0, sensitivity);
            ui.checkbox("Drag the glass to look", drag_to_look);
            ui.checkbox("Show the frame line", show_status);
            ui.separator();
            if ui.button("Save") {
                *save = true;
            }
            ui.same_line();
            if ui.button("Reset") {
                *reset = true;
            }
            ui.same_line();
            if ui.button("Close") {
                *close = true;
            }
            let [x, y] = ui.window_pos();
            let [width, height] = ui.window_size();
            rect = Rect {
                x,
                y,
                width,
                height,
            };
        });
    rect
}
