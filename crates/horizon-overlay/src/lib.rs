//! The imgui layer: the buttons the player presses, the panel that changes them,
//! and the renderer that puts both over whatever the platform just drew.
//!
//! Two kinds of imgui live here, and the difference is why. The settings panel is
//! ordinary imgui — windows, sliders, checkboxes — driven by the one pointer imgui
//! knows, which is the first finger down. The HUD is not: a HUD has to answer to
//! three fingers at once and a mouse has only one, so the buttons are rectangles
//! [`horizon_controls`] hit-tests from the raw touches, drawn into an imgui window's
//! draw list so that they still arrive in imgui's one submission with its one font
//! atlas. Editing the layout is the same rectangles with the same fingers: a button
//! follows the thumb that picked it up, which is the one thing a phone cannot do
//! without a mouse.
//!
//! The crate knows nothing about windows, activities or engines. A platform layer
//! brings it up with the path its layout lives at, hands it the device when a
//! surface appears, and calls [`Overlay::frame`] with a render pass already begun
//! and the size of the glass; what comes back is the presses the game should route.

use std::cell::RefCell;
use std::error::Error;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use horizon_controls::{Action, Hud, Layout, Rect};
use imgui::{
    Condition, ConfigFlags, Context, DrawListMut, ImColor32, MouseButton, Ui, WindowFlags,
};

mod launcher;
pub use launcher::{
    LauncherAccount, LauncherEvent, LauncherMod, LauncherModel, LauncherPage, LauncherRow,
    LauncherSetting, LauncherStory,
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
    down: bool,
}

impl Pointer {
    fn idle() -> Self {
        Self {
            finger: None,
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
    /// The launcher's front page, drawn in place of the HUD at boot.
    launcher: bool,
    /// When the launcher came up, so its opening animation can run once per open.
    launcher_since: Option<Instant>,
    /// What to show, gathered by the platform from the engine each frame.
    launcher_model: LauncherModel,
    /// What the player asked for, drained by the platform each frame.
    launcher_events: Vec<LauncherEvent>,
    /// The Geist faces the launcher draws with, once the fonts are added.
    launcher_fonts: Option<launcher::Fonts>,
    /// The chosen skin, uploaded as an imgui texture so the Skins page can draw the
    /// player from its own pixels instead of a stand-in.
    skin_texture: Option<imgui::TextureId>,
    /// The PNG the texture was made from, so a new choice replaces it rather than
    /// uploading the same file every frame.
    skin_source: Option<String>,
    /// The panel as it was drawn last frame. A finger inside it belongs to the panel
    /// and to nothing else, and the only place to learn that from is the window
    /// imgui has just laid out.
    panel_rect: Option<Rect>,
    /// The engine already drew the title panorama behind this overlay, and the
    /// launcher's front page is meant to show it. The platform clears this when it
    /// knows there is nothing back there — the bring-up card's black surface — so the
    /// page falls back to its own background instead of showing a void.
    panorama: bool,
    pointer: Pointer,
    /// A finger lifted off an imgui widget, waiting for the release to be sent at the
    /// end of the frame that rendered the press — see [`Overlay::paint`].
    pending_release: bool,
    /// The finger the game gets as its pointer: the first to land on bare glass, with
    /// neither a layout button nor the panel under it. A phone has no mouse, and the
    /// screens the game draws for itself are hit-tested from a cursor and a button, so
    /// something has to say which finger means that. A button's finger does not.
    game_pointer: Option<u64>,
    /// One style and one font size for the life of the process, decided from the
    /// first screen the activity gets. Rebuilding a font atlas mid-session to chase a
    /// rotation would cost a texture upload on a phone that is already rotating, and
    /// the layout scales with the screen on its own.
    dpi: f32,
    fonts_added: bool,
    show_status: bool,
    /// The diagnostic line drawn in a corner, set by whoever knows what the frame
    /// cost. The platform layer owns it because the overlay cannot know the frame
    /// rate, and a phone whose controls do not answer needs a number that says so.
    status: String,
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
            launcher: false,
            launcher_since: None,
            launcher_model: LauncherModel::default(),
            launcher_events: Vec::new(),
            launcher_fonts: None,
            skin_texture: None,
            skin_source: None,
            panel_rect: None,
            panorama: true,
            pointer: Pointer::idle(),
            pending_release: false,
            game_pointer: None,
            dpi: 1.0,
            fonts_added: false,
            show_status: true,
            status: String::new(),
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
    ///
    /// Every attach builds a new renderer, because a pipeline belongs to the device it
    /// was made on and a phone changes device more than once — the activity loses its
    /// surface when it is backgrounded, and the engine makes a second device when the
    /// loading screen hands over to the world. The style and the font atlas are the
    /// parts that must not be done twice.
    pub fn attach(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        width: u32,
        height: u32,
    ) {
        if !self.fonts_added {
            // A phone and a tablet want different text; the short edge is the one that
            // decides, because it is the axis a thumb crosses.
            self.dpi = (width.min(height) as f32 / 720.0).clamp(0.8, 2.5);
            self.imgui.style_mut().scale_all_sizes(self.dpi);
            // Geist becomes the default font (the first added), and the dark theme is
            // set here once. The built-in bitmap font is what would make every screen
            // look like the old menu, so it is never added.
            self.launcher_fonts = Some(launcher::theme(&mut self.imgui, self.dpi));
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
        self.resize(width, height);
    }

    /// The surface is gone; the pipeline, the buffers and the font texture go with
    /// it. The context and the layout stay, and no finger is holding anything.
    pub fn detach(&mut self) {
        self.renderer = None;
        // imgui outlives the renderer, so its button has to be let go here too: a
        // finger that was down when the surface went away sends no lift, and a stale
        // `MouseDown` makes the first tap after the resume look like a duplicate and
        // vanish. An up with no press is filtered by imgui, so this is always safe.
        if self.pointer.down {
            self.imgui
                .io_mut()
                .add_mouse_button_event(MouseButton::Left, false);
        }
        self.pending_release = false;
        self.pointer = Pointer::idle();
        self.game_pointer = None;
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
        self.panel || self.hud.edit || self.launcher
    }

    /// A finger went down. It goes to the layout unless the panel already has it, and
    /// to imgui when there is a window to click. The finger that took neither is the
    /// game's, and whoever is drawing a screen the player means to tap has to be able
    /// to ask which one that was.
    pub fn finger_down(&mut self, id: u64, x: f32, y: f32, width: f32, height: f32) {
        let inside_panel = self.panel_rect.is_some_and(|rect| rect.contains(x, y));
        log::debug!(
            "overlay: down {id} at {x:.0},{y:.0} interactive={} launcher={} held={:?}",
            self.interactive(),
            self.launcher,
            self.pointer.finger
        );
        // The launcher owns the whole glass, so a tap there is never a HUD press.
        let on_button =
            !inside_panel && !self.launcher && self.hud.down(id, x, y, width, height);
        // One finger at a time for imgui. A second finger inside the panel used to take
        // the pointer over from a finger that was mid-press: the new finger could not
        // click (its press was filtered as a duplicate), the first finger's lift then
        // matched nothing, and the second finger's lift released whatever the first one
        // had grabbed, at the second one's position. Holding the opacity slider and
        // tapping Save with the other thumb moved the slider to Save and swallowed the
        // tap.
        if self.interactive() && self.pointer.finger.is_none() {
            // A press that arrives before the previous lift was flushed would be
            // filtered as a duplicate and never click, so the lift goes out first.
            if self.pending_release {
                self.imgui
                    .io_mut()
                    .add_mouse_button_event(MouseButton::Left, false);
                self.pending_release = false;
            }
            self.pointer = Pointer {
                finger: Some(id),
                down: true,
            };
            // The position goes in with the press, ahead of it: imgui holds back a
            // mouse move it finds after a button change until the next frame, and a
            // press judged against the position before it is a press on whatever was
            // there instead — on a phone, on a panel that has only just opened.
            let io = self.imgui.io_mut();
            io.add_mouse_pos_event([x, y]);
            io.add_mouse_button_event(MouseButton::Left, true);
        }
        // Only while nothing of the overlay's own wants the screen: a finger that is
        // arranging buttons must not click whatever is drawn behind them.
        if !on_button && !self.interactive() && self.game_pointer.is_none() {
            self.game_pointer = Some(id);
        }
    }
    /// Whether `id` is the finger the game should read as its pointer. The overlay
    /// decides which finger that is, and keeps it across the moves and the lift, so a
    /// menu gets one click rather than one click per finger on the glass.
    pub fn finger_is_pointer(&self, id: u64) -> bool {
        self.game_pointer == Some(id)
    }

    pub fn finger_move(&mut self, id: u64, x: f32, y: f32, width: f32, height: f32) {
        self.hud.drag(id, x, y, width, height);
        if self.pointer.finger == Some(id) {
            self.imgui.io_mut().add_mouse_pos_event([x, y]);
        }
    }

    pub fn finger_up(&mut self, id: u64) {
        self.hud.up(id);
        if self.game_pointer == Some(id) {
            self.game_pointer = None;
        }
        if self.pointer.finger == Some(id) {
            self.pointer.finger = None;
            self.pointer.down = false;
            // Defer the lift to the end of the frame that rendered the press. imgui
            // reads a click from a button that was down in one rendered frame and up
            // in the next; a tap whose press and lift land between two frames would
            // otherwise collapse to no net change and register nothing.
            self.pending_release = true;
        }
    }

    /// The presses since the last call, with the overlay's own answered here: a tap on
    /// MENU opens the panel and is never a keystroke. Read this where the game reads
    /// its keyboard, not where it draws — a press that waits for a render pass is a
    /// press one frame late, and on a phone the player feels that in the first second.
    pub fn presses(&mut self) -> Vec<(Action, bool)> {
        let edges = self.hud.take_edges();
        for (_, down) in edges.iter().filter(|(action, _)| *action == Action::Menu) {
            if *down {
                self.panel = !self.panel;
                log::info!(
                    "overlay: the settings panel is {}",
                    if self.panel { "open" } else { "closed" }
                );
            }
        }
        edges
            .into_iter()
            .filter(|(action, _)| !action.is_overlay())
            .collect()
    }

    /// The buttons and the panel, painted into a pass that is already begun on
    /// whatever the platform has just drawn.
    pub fn paint(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pass: &mut wgpu::RenderPass<'_>,
        width: f32,
        height: f32,
    ) -> Result<(), Box<dyn Error>> {
        // Read before the destructure below: the opening needs `&self`, and that borrow
        // cannot be alive while every field is borrowed at once.
        let opening = self.launcher_opening();
        // One borrow of `self`, every field it needs: the renderer and the context are
        // two fields of the same struct, and imgui holds them both at once.
        let Overlay {
            imgui,
            hud,
            path,
            panel,
            launcher,
            launcher_model,
            launcher_events,
            launcher_fonts,
            skin_texture,
            panel_rect,
            panorama,
            dpi,
            show_status,
            status,
            frame_started,
            pending_release,
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
        }

        // Everything the panel can change is taken out of the layout into a local
        // first: the closure that draws a window holds the borrow imgui just gave us,
        // and `self` is not in it to be borrowed twice.
        let mut edit = hud.edit;
        let mut opacity = hud.layout.opacity;
        let mut scale = hud.layout.scale;
        let mut sensitivity = hud.layout.look_sensitivity;
        let mut drag_to_look = hud.layout.drag_to_look;
        let mut save = false;
        let mut reset = false;
        let mut close = false;
        let panel_open = *panel;
        let selected = hud.selected();
        let selected_label = selected.map(|action| action.text());
        let mut sel_size = selected
            .and_then(|action| hud.control_size(action))
            .unwrap_or(1.0);
        let mut sel_opacity = selected
            .and_then(|action| hud.control_opacity(action))
            .unwrap_or(opacity);
        // Whether the panel's per-button sliders actually moved this frame.
        let mut sel_touched = false;
        let buttons: Vec<(Action, Rect, f32)> = hud
            .buttons(width, height)
            .into_iter()
            .map(|(action, rect)| (action, rect, hud.layout.opacity_of(action)))
            .collect();
        // What is lit this frame: the buttons a finger holds while playing, and while
        // the layout is being arranged the one button a finger has picked up.
        let lit = if edit {
            hud.grabbed().into_iter().collect::<Vec<_>>()
        } else {
            hud.pressed_actions()
        };
        let mut panel_now: Option<Rect> = None;

        {
            let ui = imgui.new_frame();
            if *launcher {
                if let Some(fonts) = launcher_fonts.as_ref() {
                    launcher::draw(
                        ui,
                        launcher_model,
                        fonts,
                        *dpi,
                        width,
                        height,
                        opening,
                        *panorama,
                        *skin_texture,
                        launcher_events,
                    );
                }
            } else {
                hud_buttons(
                    ui,
                    &buttons,
                    &lit,
                    if edit { selected } else { None },
                    *dpi,
                    *show_status,
                    status.as_str(),
                    width,
                    height,
                );
                // The panel comes after the buttons, so it is drawn on top of them: a
                // slider you cannot see because a "MINE" is over it is a slider you
                // cannot find on a phone, where there is no hover to discover it with.
                if *panel {
                    panel_now = Some(settings(
                        ui,
                        status.as_str(),
                        &mut scale,
                        &mut opacity,
                        &mut sensitivity,
                        &mut drag_to_look,
                        &mut edit,
                        show_status,
                        selected_label.as_deref(),
                        &mut sel_size,
                        &mut sel_opacity,
                        &mut sel_touched,
                        &mut save,
                        &mut reset,
                        &mut close,
                        *dpi,
                    ));
                }
            }
        }

        hud.layout.scale = scale;
        hud.layout.opacity = opacity;
        hud.layout.look_sensitivity = sensitivity;
        hud.layout.drag_to_look = drag_to_look;
        if edit && panel_open && sel_touched {
            // The panel edited the selected button's own size and opacity; commit them
            // back. Only when a slider actually moved, and only while editing with the
            // panel up: a write on every frame would pin the button to whatever the
            // layout opacity was when it was selected, and restyle it while playing.
            if let Some(action) = selected {
                hud.set_control_size(action, sel_size);
                hud.set_control_opacity(action, sel_opacity);
            }
        }
        if hud.edit != edit {
            // Changing the mode under a finger that was pressing something has to let
            // it go, or the player arranges the layout with a character walking into a
            // wall for the next hour.
            hud.release_all();
        }
        hud.edit = edit;
        if reset {
            hud.layout = Layout::default();
            log::info!("overlay: the layout is the one the build ships with");
        }
        if save {
            hud.save(path);
        }
        if close {
            *panel = false;
            log::info!("overlay: the settings panel is closed from its Close button");
        }
        *panel_rect = panel_now;

        let draw_data = imgui.render();
        renderer
            .render(draw_data, queue, device, pass)
            .map_err(|error| format!("imgui: {error}"))?;
        // Send the deferred lift now, after this frame's new_frame has seen the press:
        // it queues for the next frame, so imgui reads a down in one frame and an up in
        // the next and fires the click the tap was meant to make.
        if *pending_release {
            imgui
                .io_mut()
                .add_mouse_button_event(MouseButton::Left, false);
            *pending_release = false;
        }
        Ok(())
    }

    /// Read the presses and paint, in that order: what a player pressed belongs to
    /// this frame's input, and what the frame looks like belongs to its output. A test
    /// card and an engine both want exactly this, so it is the one call the platform
    /// layer has to make.
    pub fn frame(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pass: &mut wgpu::RenderPass<'_>,
        width: f32,
        height: f32,
    ) -> Result<Vec<(Action, bool)>, Box<dyn Error>> {
        let game = self.presses();
        self.paint(device, queue, pass, width, height)?;
        Ok(game)
    }

    /// How far the fingers turned the camera since the last call, in degrees: `x` a
    /// turn to the right, `y` a pitch down. The layout's own sensitivity has already
    /// been divided out, so what the game turns by is this and nothing else.
    pub fn look(&mut self) -> [f32; 2] {
        self.hud.take_look()
    }

    /// The line in the corner. Empty turns the "Show the frame line" checkbox off
    /// as far as the screen is concerned.
    pub fn set_status(&mut self, text: impl Into<String>) {
        self.status = text.into();
    }

    pub fn hud(&self) -> &Hud {
        &self.hud
    }

    pub fn panel_open(&self) -> bool {
        self.panel
    }

    /// Show this PNG on the Skins page, decoding it into an imgui texture the first
    /// time it changes.
    ///
    /// The page draws the player out of the skin's own pixels, so it needs the image
    /// on the GPU; the overlay owns the device, and the platform only knows which file
    /// the player chose. `None` clears it, which is what the page draws as a plain
    /// figure.
    pub fn set_launcher_skin(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        path: Option<&Path>,
    ) {
        let key = path.map(|p| p.display().to_string());
        if key == self.skin_source {
            return;
        }
        self.skin_source = key;
        let Some(renderer) = self.renderer.as_mut() else {
            return;
        };
        if let Some(old) = self.skin_texture.take() {
            renderer.textures.remove(old);
        }
        let Some(path) = path else {
            return;
        };
        let Ok(file) = std::fs::File::open(path) else {
            return;
        };
        let mut decoder = png::Decoder::new(std::io::BufReader::new(file));
        // Skins are indexed, greyscale or RGBA; let the decoder hand back RGBA8
        // whichever it is, so the texture's format is one thing.
        decoder.set_transformations(png::Transformations::normalize_to_color8());
        let Ok(mut reader) = decoder.read_info() else {
            return;
        };
        let mut buffer = vec![0; reader.output_buffer_size()];
        let Ok(info) = reader.next_frame(&mut buffer) else {
            return;
        };
        // The texture is RGBA8 whatever the PNG was: expand three channels, and add
        // an opaque alpha to a greyscale or a two-channel image.
        let pixels = match info.color_type {
            png::ColorType::Rgba => buffer[..info.buffer_size()].to_vec(),
            png::ColorType::Rgb => buffer[..info.buffer_size()]
                .chunks_exact(3)
                .flat_map(|p| [p[0], p[1], p[2], 255])
                .collect(),
            png::ColorType::GrayscaleAlpha => buffer[..info.buffer_size()]
                .chunks_exact(2)
                .flat_map(|p| [p[0], p[0], p[0], p[1]])
                .collect(),
            png::ColorType::Grayscale => buffer[..info.buffer_size()]
                .iter()
                .flat_map(|g| [*g, *g, *g, 255])
                .collect(),
            png::ColorType::Indexed => return,
        };
        let texture = horizon_imgui::Texture::new(
            device,
            renderer,
            horizon_imgui::TextureConfig {
                label: Some("launcher skin"),
                size: wgpu::Extent3d {
                    width: info.width,
                    height: info.height,
                    depth_or_array_layers: 1,
                },
                // The same format the font atlas uses: the renderer samples every
                // imgui texture through one shader, and an sRGB texture there would be
                // corrected twice.
                format: Some(wgpu::TextureFormat::Rgba8Unorm),
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                ..Default::default()
            },
        );
        texture.write(queue, &pixels, info.width, info.height);
        self.skin_texture = Some(renderer.textures.insert(texture));
    }

    /// Tell the overlay whether the engine's title panorama is behind it. The
    /// launcher's front page is drawn over that panorama and must not paint it out;
    /// the bring-up card has no engine and passes `false`, so the page keeps its own
    /// background there.
    pub fn set_panorama(&mut self, on: bool) {
        self.panorama = on;
    }

    /// Show or hide the launcher's front page. Showing it lets go of any held
    /// button, so a player never comes back to a character walking into a wall.
    pub fn show_launcher(&mut self, on: bool) {
        // Idempotent on purpose. The platform reports the launcher as up on every
        // frame it is up, and the reset below throws away the deferred lift imgui
        // needs to turn a tap into a click — which made every page of the launcher
        // draw but answer to nothing. The state is cleared once, on the way in.
        if self.launcher == on {
            return;
        }
        if on {
            self.hud.release_all();
            // The panel and the arrange mode are for the game behind the HUD, and the
            // launcher hides the HUD entirely. Left set, they came back with the HUD
            // when the launcher closed — and an open panel makes `interactive()` true,
            // so no bare-glass finger could ever reach the game's own screens again.
            self.panel = false;
            self.hud.edit = false;
            self.hud.set_selected(None);
            self.panel_rect = None;
            self.pointer = Pointer::idle();
            self.pending_release = false;
            self.launcher_since = Some(Instant::now());
        }
        self.launcher = on;
    }

    /// How far the launcher's opening animation has got, 0 to 1. It holds the wordmark
    /// on the glass, then reveals the interface; `None` once the opening is spent, so
    /// the ordinary draw path costs nothing after the first second.
    ///
    /// The hold is shorter than the theme's desktop `OPENING_HOLD`: a phone is opened
    /// and closed far more often than a desktop app, and a second and a half of static
    /// wordmark is a delay there rather than a brand moment. The reveal keeps the
    /// shared length, so the two platforms still move alike once they move.
    fn launcher_opening(&self) -> Option<f32> {
        let since = self.launcher_since?;
        let elapsed = since.elapsed();
        let hold = Duration::from_millis(420);
        let reveal = horizon_theme::OPENING_REVEAL;
        if elapsed >= hold + reveal {
            return None;
        }
        Some(((elapsed.saturating_sub(hold)).as_secs_f32() / reveal.as_secs_f32()).clamp(0.0, 1.0))
    }

    pub fn launcher_active(&self) -> bool {
        self.launcher
    }

    /// The page the launcher is on, so the platform can keep its model in sync.
    pub fn launcher_page(&self) -> LauncherPage {
        self.launcher_model.page
    }

    /// What to draw on the launcher next frame. The page and whether the name field is
    /// open belong to the glass rather than the engine, so they survive the report that
    /// arrives a frame after the finger asked for them.
    pub fn set_launcher_model(&mut self, mut model: LauncherModel) {
        model.page = self.launcher_model.page;
        model.editing = self.launcher_model.editing;
        self.launcher_model = model;
    }

    /// Whether the launcher has its name field open, which on a phone is the whole
    /// question about the keyboard.
    pub fn launcher_editing(&self) -> bool {
        self.launcher && self.launcher_model.editing
    }

    /// Close the name field from the platform: the keyboard's own Enter means the same
    /// thing as the card's Save button, and the card cannot know which came first.
    pub fn end_launcher_edit(&mut self) {
        self.launcher_model.editing = false;
    }

    /// What the player asked the launcher for, since the last call.
    pub fn take_launcher_events(&mut self) -> Vec<LauncherEvent> {
        std::mem::take(&mut self.launcher_events)
    }

    /// Where the layout lives, for a message that has to name it.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// One button: its body, its edge and its legend. The body exists to aim at and the
/// legend to read, so the text keeps a floor of its own under the layout's opacity —
/// a see-through "MINE" is a button you cannot find again.
fn draw_button(
    draw: &DrawListMut<'_>,
    ui: &Ui,
    action: Action,
    rect: Rect,
    pressed: bool,
    opacity: f32,
    selected: bool,
) {
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
    if selected {
        // A gold ring, drawn outside the button so it reads as "this is the one the
        // panel is editing" rather than "this is pressed".
        let pad = 3.0;
        draw.add_rect(
            [rect.x - pad, rect.y - pad],
            [rect.x + rect.width + pad, rect.y + rect.height + pad],
            rgba(EDGE_ACTIVE, 1.0),
        )
        .rounding(rounding + pad)
        .thickness(2.0)
        .build();
    }
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

/// The HUD: one window the size of the screen that takes no input at all, so that
/// imgui is the painter and never the player. The fingers it answers to came from
/// `horizon_controls`, and it is painted back to front — the list is front to back —
/// which is why a small button on top of a big one is both visible and pressable.
fn hud_buttons(
    ui: &Ui,
    buttons: &[(Action, Rect, f32)],
    lit: &[Action],
    selected: Option<Action>,
    dpi: f32,
    show_status: bool,
    status: &str,
    width: f32,
    height: f32,
) {
    // A window with no decoration cannot be collapsed, so its contents always run;
    // the `Option` imgui hands back is only that promise, and there is nothing to do
    // with it.
    let _ = ui.window("Horizon controls")
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
            for (action, rect, opacity) in buttons.iter().rev() {
                draw_button(
                    &draw,
                    ui,
                    *action,
                    *rect,
                    lit.contains(action),
                    *opacity,
                    selected == Some(*action),
                );
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

/// The settings panel. Everything a player can reach without a file manager and
/// without this repository is behind one of these widgets.
///
/// The per-button sliders are only committed when they actually moved: `set_control_opacity`
/// stores an explicit value, and writing one every frame a button happened to be selected
/// pinned that button to the layout opacity it had at the moment of the tap and detached it
/// from the layout-wide slider from then on.
#[allow(clippy::too_many_arguments)]
fn settings(
    ui: &Ui,
    status: &str,
    scale: &mut f32,
    opacity: &mut f32,
    sensitivity: &mut f32,
    drag_to_look: &mut bool,
    edit: &mut bool,
    show_status: &mut bool,
    selected_label: Option<&str>,
    sel_size: &mut f32,
    sel_opacity: &mut f32,
    sel_touched: &mut bool,
    save: &mut bool,
    reset: &mut bool,
    close: &mut bool,
    dpi: f32,
) -> Rect {
    // Where the panel is put and what it is when it has not been laid out: a window
    // that never opened claims no pixels, so a finger cannot land inside it.
    let unmeasured = Rect {
        x: 12.0 * dpi,
        y: 12.0 * dpi,
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
            if *edit {
                ui.separator();
                match selected_label {
                    Some(label) => {
                        let line = format!("Selected: {label}");
                        ui.text(line.as_str());
                        if ui.slider("This button size", 0.03, 0.4, sel_size) {
                            *sel_touched = true;
                        }
                        if ui.slider("This button opacity", 0.08, 1.0, sel_opacity) {
                            *sel_touched = true;
                        }
                    }
                    None => {
                        ui.text("Tap a button to edit it");
                    }
                }
            }
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
            Rect {
                x,
                y,
                width,
                height,
            }
        })
        .unwrap_or(unmeasured)
}

thread_local! {
    /// The overlay of this thread, if the platform has installed one.
    static SHARED: RefCell<Option<Overlay>> = const { RefCell::new(None) };
}

/// Put an overlay on this thread, unless one is already there.
///
/// It cannot live on the renderer that draws it: the engine builds one device for the
/// loading screen and another for the world, and imgui panics when a thread is handed a
/// second context. So the overlay outlives both, and every layer that has a finger or a
/// render pass reaches it through [`with`].
pub fn install(path: &Path) {
    SHARED.with(|shared| {
        let missing = shared.borrow().is_none();
        if missing {
            *shared.borrow_mut() = Some(Overlay::load(path.to_path_buf()));
        }
    });
}

/// Reach this thread's overlay. `None` is the ordinary answer everywhere that there are
/// no touch controls to reach.
///
/// `f` must not call `with` itself: the overlay is borrowed for the length of the call.
pub fn with<T>(f: impl FnOnce(&mut Overlay) -> T) -> Option<T> {
    SHARED.with(|shared| shared.borrow_mut().as_mut().map(f))
}
