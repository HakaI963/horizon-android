//! The card that runs when there is no game to run.
//!
//! A phone without a prepared resource pack has no world to show, and a black window
//! tells nobody anything, so the adapter draws this instead. It opens the Vulkan surface
//! through winit's `NativeActivity` window, presents a test card every frame and reports
//! what the touchscreen sends, and it carries the same on-screen controls the game uses,
//! which is the only way to tell a dead button from a dead engine.
//!
//! On a phone the things that can only be found on a phone are the swapchain format,
//! the surface going stale when the activity is backgrounded, the resolution changing
//! when the device rotates, and touch arriving at all — so the card watches exactly
//! those four.

use std::error::Error;
use std::sync::Arc;
use std::time::Instant;

use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{Touch, TouchPhase, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::platform::android::activity::AndroidApp;
use winit::window::{Window, WindowId};

use horizon_controls::Action;
use horizon_overlay::{LauncherEvent, LauncherModel, LauncherRow, Overlay};

use crate::frame_dump::FrameDump;
use crate::installer;
use crate::paths;

/// A block of the test card, placed in the top-left pixel coordinates the touch
/// events report. Its color is the palette's entry for `slot`. A zero width or
/// height skips it.
struct Block {
    slot: usize,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

/// The card is one oversized triangle per block, clipped to the block's scissor
/// rect: no index data, no uniforms, and nothing written per frame.
const CARD_SHADER: &str = r#"
struct Out {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) index: u32, @location(0) color: vec4<f32>) -> Out {
    // (-1,-1), (3,-1), (-1,3): one triangle covering the framebuffer several
    // times over, so the scissor rect alone decides which pixels get written.
    let x = select(-1.0, 3.0, index == 1u);
    let y = select(-1.0, 3.0, index == 2u);
    var out: Out;
    out.position = vec4<f32>(x, y, 0.0, 1.0);
    out.color = color;
    return out;
}

@fragment
fn fs_main(in: Out) -> @location(0) vec4<f32> {
    return in.color;
}
"#;

/// Four corners, the sweep stripe, up to five fingers and the finger itself.
const MAX_BLOCKS: usize = 11;
/// A block is its own three vertices, so drawing it means binding a different
/// slice. Rewriting one bound buffer between the draws of a single submission is
/// not something Vulkan can order, and this way there is nothing to order.
const BLOCK_BYTES: u64 = 3 * 16;

/// One color per slot. The palette never changes, only which blocks are drawn
/// and where, so it goes to the GPU once.
const PALETTE: [[f32; 4]; MAX_BLOCKS] = [
    [1.0, 0.0, 0.0, 1.0], // top left: red
    [0.0, 1.0, 0.0, 1.0], // top right: green
    [0.0, 0.0, 1.0, 1.0], // bottom left: blue
    [1.0, 1.0, 0.0, 1.0], // bottom right: yellow
    [1.0, 0.0, 1.0, 1.0], // the sweep: magenta
    [0.0, 1.0, 1.0, 1.0], // one per finger: cyan
    [0.0, 1.0, 1.0, 1.0],
    [0.0, 1.0, 1.0, 1.0],
    [0.0, 1.0, 1.0, 1.0],
    [0.0, 1.0, 1.0, 1.0],
    [1.0, 1.0, 1.0, 1.0], // where the finger is: white
];

const ATTRIBUTES: [wgpu::VertexAttribute; 1] = wgpu::vertex_attr_array![0 => Float32x4];

/// The card's pipeline, compiled by naga into the driver's SPIR-V on the phone.
struct Card {
    pipeline: wgpu::RenderPipeline,
    palette: wgpu::Buffer,
}

impl Card {
    fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Horizon bring-up card"),
            source: wgpu::ShaderSource::Wgsl(CARD_SHADER.into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Horizon bring-up card"),
            bind_group_layouts: &[],
            ..Default::default()
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Horizon bring-up card"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: 16,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &ATTRIBUTES,
                })],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let mut bytes = Vec::with_capacity(MAX_BLOCKS * BLOCK_BYTES as usize);
        for color in PALETTE {
            for _ in 0..3 {
                for channel in color {
                    bytes.extend_from_slice(&channel.to_le_bytes());
                }
            }
        }
        let palette = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Horizon bring-up card palette"),
            size: bytes.len() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&palette, 0, &bytes);
        Self { pipeline, palette }
    }
}

/// The device and the swapchain it presents to.
struct Screen {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    card: Card,
}

pub struct BringUp {
    app: AndroidApp,
    /// Why the card is on the screen instead of the game, in the corner of it. A phone
    /// has no terminal, and a missing resource pack looks exactly like a dead driver
    /// unless the picture says which it is.
    notice: String,
    /// A retry of the install is running on a worker thread. The card keeps drawing
    /// and keeps taking taps while it does; the Java dialog is the progress surface.
    installing: bool,
    install_job: Option<installer::InstallJob>,
    /// The install state and the player name, read once rather than on every frame.
    /// Both are files, and a card that draws at the phone's refresh rate has no
    /// business stat-ing the pack sixty times a second.
    cached_installed: Option<bool>,
    cached_account: Option<String>,
    window: Option<Arc<Window>>,
    screen: Option<Screen>,
    dump: Arc<FrameDump>,
    frames: u64,
    touches: u64,
    fingers: u32,
    /// Where the last touch landed. It stays after the finger lifts: injection
    /// can be seconds behind the call that asks for it, so a shell that taps and
    /// then asks for a frame needs the marker to still be there, and whether a
    /// finger is down right now is what the cyan blocks are for.
    touch: Option<(f64, f64)>,
    started: Instant,
}

/// The swapchain format Horizon uses on desktop: a non-sRGB 8-bit target, because
/// vanilla blends in gamma space and the game writes gamma-encoded pixels itself.
fn surface_format(
    formats: &[wgpu::TextureFormat],
    default: wgpu::TextureFormat,
) -> wgpu::TextureFormat {
    [
        wgpu::TextureFormat::Bgra8Unorm,
        wgpu::TextureFormat::Rgba8Unorm,
    ]
    .into_iter()
    .find(|format| formats.contains(format))
    .or_else(|| formats.iter().copied().find(|format| !format.is_srgb()))
    .unwrap_or(default)
}

/// The alpha mode to ask for. Desktop compositors list `Opaque` and prefer it —
/// the game is edge to edge, so its alpha is nobody else's business. Android's
/// Vulkan driver lists only `Inherit`, and asking for anything else is a
/// validation error, not a preference.
fn alpha_mode(modes: &[wgpu::CompositeAlphaMode]) -> wgpu::CompositeAlphaMode {
    modes
        .iter()
        .copied()
        .find(|mode| *mode == wgpu::CompositeAlphaMode::Opaque)
        .or_else(|| modes.first().copied())
        .unwrap_or_default()
}

fn open_screen(window: &Arc<Window>, app: &AndroidApp) -> Result<Screen, Box<dyn Error>> {
    let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
    descriptor.backends = wgpu::Backends::VULKAN;
    let instance = wgpu::Instance::new(descriptor);
    let surface = instance.create_surface(window.clone())?;
    log::info!("bring-up: created the Vulkan surface");
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: Some(&surface),
        force_fallback_adapter: false,
        ..Default::default()
    }))
    .map_err(|error| format!("no Vulkan adapter on this device: {error}"))?;
    let info = adapter.get_info();
    log::info!(
        "bring-up: adapter {} ({:?}) vendor {} driver {:?}",
        info.name,
        info.device_type,
        info.vendor,
        info.driver
    );
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("Horizon bring-up device"),
        required_features: wgpu::Features::empty(),
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .map_err(|error| format!("request_device: {error}"))?;
    let PhysicalSize { width, height } = window.inner_size();
    let capabilities = surface.get_capabilities(&adapter);
    let mut config = surface
        .get_default_config(&adapter, width.max(1), height.max(1))
        .ok_or("the surface has no supported configuration")?;
    config.format = surface_format(&capabilities.formats, config.format);
    config.view_formats = vec![config.format.remove_srgb_suffix()];
    config.alpha_mode = alpha_mode(&capabilities.alpha_modes);
    config.present_mode = wgpu::PresentMode::Fifo;
    // The frame server reads the presented image back; a driver that will not
    // let it is reported rather than forced.
    if capabilities.usages.contains(wgpu::TextureUsages::COPY_SRC) {
        config.usage |= wgpu::TextureUsages::COPY_SRC;
    } else {
        log::warn!("bring-up: this swapchain cannot be copied out");
    }
    log::info!(
        "bring-up: configuring {}x{} {:?} {:?} {:?}, scale {}, content {:?}, sdk {}",
        config.width,
        config.height,
        config.format,
        config.alpha_mode,
        config.present_mode,
        window.scale_factor(),
        app.content_rect(),
        app.config().sdk_version()
    );
    surface.configure(&device, &config);
    // The pipeline target has to be the attachment view's own format, and the
    // view is made from the swapchain texture without a descriptor.
    let card = Card::new(&device, &queue, config.format);
    log::info!("bring-up: the card pipeline compiled");
    Ok(Screen {
        surface,
        device,
        queue,
        config,
        card,
    })
}

impl Screen {
    /// The background stays dark and still, so that the block colors read the
    /// same in every frame dump and the sweep stripe is what proves frames keep
    /// arriving.
    ///
    /// `wgpu::Color` is 0.0 to 1.0 (`Color::WHITE` is `1.0`, not `255.0`), so
    /// scaling these by 255 clamps every channel to white and the card below it
    /// is the only thing left to look at.
    fn background(fingers: u32) -> wgpu::Color {
        wgpu::Color {
            r: 0.04,
            g: 0.05 + 0.03 * fingers.min(4) as f64,
            b: 0.07,
            a: 1.0,
        }
    }

    /// Which way is up, is the loop still running, where the finger is and how
    /// many there are, each as a block of solid color. Every one of these is
    /// something the phone can get wrong that a flat clear cannot show: the
    /// display can be rotated while the framebuffer is not, the driver can
    /// present one frame and then stop, and touch can arrive in a coordinate
    /// space that is not the framebuffer's.
    fn blocks(&self, frame: u64, fingers: u32, touch: Option<(f64, f64)>) -> Vec<Block> {
        let (width, height) = (self.config.width, self.config.height);
        let shortest = width.min(height);
        // Never longer than the axis it is measured against: the corner blocks
        // are placed by subtracting this from a dimension.
        let side = (shortest / 8).clamp(16, 192).min(shortest.max(1));
        let mut blocks = vec![
            Block {
                slot: 0,
                x: 0,
                y: 0,
                width: side,
                height: side,
            },
            Block {
                slot: 1,
                x: width - side,
                y: 0,
                width: side,
                height: side,
            },
            Block {
                slot: 2,
                x: 0,
                y: height - side,
                width: side,
                height: side,
            },
            Block {
                slot: 3,
                x: width - side,
                y: height - side,
                width: side,
                height: side,
            },
            // Two seconds per crossing at 120 fps.
            Block {
                slot: 4,
                x: (frame % 240) as u32 * width / 240,
                y: 0,
                width: (side / 2).max(8),
                height,
            },
        ];
        for finger in 0..fingers.min(5) {
            blocks.push(Block {
                slot: 5 + finger as usize,
                x: width / 2 + finger * side,
                y: height - side,
                width: side / 2,
                height: side / 2,
            });
        }
        if let Some((x, y)) = touch {
            let marker = side.max(32);
            blocks.push(Block {
                slot: MAX_BLOCKS - 1,
                x: (x as u32).saturating_sub(marker / 2),
                y: (y as u32).saturating_sub(marker / 2),
                width: marker,
                height: marker,
            });
        }
        blocks
    }

    /// One block: bind its three palette vertices, place the scissor rect, draw.
    fn fill(&self, pass: &mut wgpu::RenderPass<'_>, block: &Block) {
        // Scissor coordinates are pixels from the top left, the same space touch
        // reports, and Vulkan wants the rect inside the attachment.
        let x = block.x.min(self.config.width);
        let y = block.y.min(self.config.height);
        let width = block.width.min(self.config.width - x);
        let height = block.height.min(self.config.height - y);
        if width == 0 || height == 0 {
            return;
        }
        let start = block.slot as u64 * BLOCK_BYTES;
        pass.set_vertex_buffer(0, self.card.palette.slice(start..start + BLOCK_BYTES));
        pass.set_scissor_rect(x, y, width, height);
        pass.draw(0..3, 0..1);
    }

    fn draw(
        &mut self,
        overlay: &mut Overlay,
        status: &str,
        frame: u64,
        fingers: u32,
        touch: Option<(f64, f64)>,
        dump: &FrameDump,
    ) -> Result<Vec<(Action, bool)>, Box<dyn Error>> {
        let acquired = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return Ok(Vec::new());
            }
            wgpu::CurrentSurfaceTexture::Outdated => {
                self.surface.configure(&self.device, &self.config);
                return Ok(Vec::new());
            }
            wgpu::CurrentSurfaceTexture::Lost => return Err("the surface was lost".into()),
            wgpu::CurrentSurfaceTexture::Validation => {
                return Err("the surface failed validation".into());
            }
        };
        let view = acquired
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        let attachments = [Some(wgpu::RenderPassColorAttachment {
            view: &view,
            resolve_target: None,
            depth_slice: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(Self::background(fingers)),
                store: wgpu::StoreOp::Store,
            },
        })];
        let edges;
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Horizon bring-up card"),
                color_attachments: &attachments,
                depth_stencil_attachment: None,
                ..Default::default()
            });
            pass.set_pipeline(&self.card.pipeline);
            for block in self.blocks(frame, fingers, touch) {
                self.fill(&mut pass, &block);
            }
            // The controls go over the card, in the same pass and after it: the card is
            // what says the platform works, and it is worth looking at only until the
            // HUD is on top of it.
            overlay.set_status(status);
            edges = overlay.frame(
                &self.device,
                &self.queue,
                &mut pass,
                self.config.width as f32,
                self.config.height as f32,
            )?;
        }
        self.queue.submit([encoder.finish()]);
        // Read before `present`: after it, the driver owns the image again and
        // there is nothing left to copy. Does nothing unless someone asked.
        dump.capture(
            &self.device,
            &self.queue,
            &acquired.texture,
            self.config.width,
            self.config.height,
        );
        // No `pre_present_notify()`: that paces winit's Wayland frame callbacks,
        // and Android's ANativeWindow queue paces FIFO by itself.
        self.queue.present(acquired);
        Ok(edges)
    }

    fn resize(&mut self, width: u32, height: u32) {
        if width == 0
            || height == 0
            || (width, height) == (self.config.width, self.config.height)
        {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
        log::info!("bring-up: resized to {width}x{height}");
    }
}

impl BringUp {
    /// The launcher over the card. Nothing of the engine is running here to be asked,
    /// so the model is what this half can see for itself: whether the pack landed, what
    /// went wrong if it did not, and the one action that helps — install it.
    fn sync_launcher(&mut self, event_loop: &ActiveEventLoop) {
        // A worker's result lands before the model is built, so the frame that reports
        // it is the frame after it finished.
        self.poll_install();
        let installing = self.installing;
        let notice = self.notice.clone();
        let version = horizon_core::VANILLA_VERSION.to_string();
        // The install state and the player name are files, and this runs on every
        // frame the card is up. Both are re-read only when something could have
        // changed them: an install finishing, or a name being saved from the card.
        if self.cached_installed.is_none() {
            self.cached_installed = Some(installer::installed(&paths::resources_dir(&self.app)));
        }
        let installed = self.cached_installed.unwrap_or(false);
        if self.cached_account.is_none() {
            self.cached_account = Some(
                std::env::current_dir()
                    .map(|dir| horizon_client::offline_name::load_or_create(&dir))
                    .unwrap_or_else(|_| "Player".to_string()),
            );
        }
        let account = self
            .cached_account
            .clone()
            .unwrap_or_else(|| "Player".to_string());
        let status = if installing {
            "Installing Minecraft Java. Keep this open.".to_string()
        } else if !notice.is_empty() {
            notice
        } else if installed {
            "Installed. Close Horizon and open it again to play.".to_string()
        } else {
            String::new()
        };
        // The sound pack fills in behind the card on its own thread; this is the only
        // place its counter can be read.
        let sounds = horizon_client::sound_progress::snapshot();
        let rows = vec![LauncherRow {
            id: 0,
            title: format!("Minecraft Java {version}"),
            subtitle: if installed {
                "The pack this build was made for".to_string()
            } else {
                "Fetched from Mojang and unpacked with the seed this APK carries".to_string()
            },
            badge: if installed { "installed" } else { "install" }.to_string(),
            // A row that is installed has nothing left to do here: the card cannot
            // start the engine it does not own, so the row stops claiming to.
            enabled: !installing && !installed,
            ..Default::default()
        }];
        let facts = vec![
            ("Minecraft".to_string(), version.clone()),
            ("Engine".to_string(), "Horizon · Rust".to_string()),
            ("Platform".to_string(), "Android · native".to_string()),
        ];
        let _ = horizon_overlay::with(|overlay| {
            overlay.show_launcher(true);
            overlay.set_launcher_model(LauncherModel {
                instance: "Default".to_string(),
                version,
                rows,
                status: match (&sounds, status.is_empty()) {
                    (Some((line, _)), true) => line.clone(),
                    _ => status,
                },
                progress: sounds.as_ref().map(|(_, fraction)| *fraction),
                account,
                facts,
                // The front button is Play only when there is something to play; the
                // card has no engine to hand the screen to, so after a successful
                // install it offers the restart that would reach one.
                playable: false,
                front: installed.then(|| "Close".to_string()),
                ..Default::default()
            });
        });
        let events = horizon_overlay::with(|overlay| overlay.take_launcher_events())
            .unwrap_or_default();
        for event in events {
            match event {
                LauncherEvent::Install
                | LauncherEvent::Play
                | LauncherEvent::PlayRow(_)
                | LauncherEvent::NewInstance
                    if !installing && !installed =>
                {
                    self.install_now();
                }
                LauncherEvent::Quit => event_loop.exit(),
                LauncherEvent::Restart if installed => {
                    // The pack is on disk and this loop is the only one the activity
                    // will ever give out, so the engine cannot be started from here.
                    // Ending the process is what makes the next open a clean one.
                    log::info!("bring-up: the pack is ready; closing so the next open plays it");
                    event_loop.exit();
                }
                _ => {}
            }
        }
    }

    /// Run the install again, on a worker thread. It used to run here, inside
    /// `RedrawRequested`, which froze the card for the whole download and unpack —
    /// Android's definition of an ANR. Now the card keeps drawing, the Java dialog
    /// keeps showing progress, and `poll_install` picks the result up next frame.
    fn install_now(&mut self) {
        if self.install_job.is_some() {
            return;
        }
        self.notice.clear();
        log::info!("bring-up: installing Minecraft Java again");
        match installer::spawn_install(&self.app) {
            Some(job) => {
                self.install_job = Some(job);
                self.installing = true;
            }
            // No thread to run it on: say so instead of showing an install that will
            // never finish.
            None => {
                self.notice = "Minecraft install failed: no thread to download with".to_string();
            }
        }
    }

    /// Take the worker's result, once it has one.
    fn poll_install(&mut self) {
        let Some(job) = self.install_job.as_mut() else {
            return;
        };
        let Some(result) = job.poll() else {
            return;
        };
        self.install_job = None;
        self.installing = false;
        // The install just changed the answer, so the cached one is stale.
        self.cached_installed = None;
        self.notice = match result {
            Ok(()) => "Installed. Close Horizon and open it again to play.".to_string(),
            Err(error) => {
                // The Java dialog is the only surface during an install, and it is not
                // cancelable: a failed retry that left it up trapped the player behind a
                // spinner with no way back to the card.
                installer::fail_dialog(&error);
                format!("Minecraft install failed: {error}")
            }
        };
    }
}

impl ApplicationHandler for BringUp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let window = match event_loop.create_window(Window::default_attributes().with_title("Horizon"))
        {
            Ok(window) => Arc::new(window),
            Err(error) => {
                log::error!("bring-up: no window: {error}");
                event_loop.exit();
                return;
            }
        };
        log::info!(
            "bring-up: window {}x{} at scale {}",
            window.inner_size().width,
            window.inner_size().height,
            window.scale_factor()
        );
        match open_screen(&window, &self.app) {
            Ok(screen) => {
                horizon_overlay::with(|overlay| {
                    overlay.attach(
                        &screen.device,
                        &screen.queue,
                        screen.config.format,
                        screen.config.width,
                        screen.config.height,
                    );
                    // The launcher, not the touch layout: there is no world behind this
                    // card to walk, and the one thing worth offering is the install.
                    overlay.show_launcher(true);
                });
                self.screen = Some(screen);
                self.window = Some(window);
                self.started = Instant::now();
                event_loop.set_control_flow(ControlFlow::Poll);
            }
            Err(error) => {
                log::error!("bring-up: no renderer: {error}");
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        if self.window.as_ref().is_none_or(|w| w.id() != id) {
            return;
        }
        match event {
            WindowEvent::Resized(size) => {
                if let Some(screen) = self.screen.as_mut() {
                    screen.resize(size.width, size.height);
                }
                horizon_overlay::with(|overlay| overlay.resize(size.width, size.height));
            }
            WindowEvent::Touch(Touch {
                phase,
                id,
                location,
                force,
                ..
            }) => {
                let (width, height) = self
                    .screen
                    .as_ref()
                    .map(|screen| (screen.config.width, screen.config.height))
                    .unwrap_or((1, 1));
                // winit already reports a touch in the window's physical pixels — the
                // same space the buttons, the menus and a frame dump are in — so it is
                // used as it arrives. Scaling it by the display density here moved every
                // press to twice the distance the finger went, which is why the Install
                // button could not be tapped where it was drawn.
                let (x, y) = (location.x as f32, location.y as f32);
                match phase {
                    TouchPhase::Started => {
                        self.touches += 1;
                        self.fingers += 1;
                        self.touch = Some((f64::from(x), f64::from(y)));
                        log::info!(
                            "bring-up: touch {id} down at {x:.0}x{y:.0} force {:?}",
                            force
                        );
                        horizon_overlay::with(|overlay| {
                            overlay.finger_down(id, x, y, width as f32, height as f32);
                        });
                    }
                    TouchPhase::Moved => {
                        self.touch = Some((f64::from(x), f64::from(y)));
                        horizon_overlay::with(|overlay| {
                            overlay.finger_move(id, x, y, width as f32, height as f32);
                        });
                    }
                    TouchPhase::Ended | TouchPhase::Cancelled => {
                        self.fingers = self.fingers.saturating_sub(1);
                        log::info!("bring-up: touch {id} up ({} fingers left)", self.fingers);
                        horizon_overlay::with(|overlay| overlay.finger_up(id));
                    }
                }
            }
            WindowEvent::RedrawRequested => {
                self.sync_launcher(event_loop);
                let Some(screen) = self.screen.as_mut() else {
                    return;
                };
                self.frames += 1;
                let elapsed = self.started.elapsed();
                let fps = self.frames as f64 / elapsed.as_secs_f64().max(0.001);
                // One line the frame dump can show and logcat does not need: the tools
                // on a phone read the picture.
                let (fingers, touches) = (self.fingers, self.touches);
                let pressed = horizon_overlay::with(|overlay| {
                    overlay.hud().pressed_actions().len()
                })
                .unwrap_or(0);
                let line = format!(
                    "{fps:.0} fps, {fingers} fingers, {touches} touches, {pressed} pressed"
                );
                let status = if self.notice.is_empty() {
                    line
                } else {
                    format!("{} — {line}", self.notice)
                };
                let (frames, touch, dump) = (self.frames, self.touch, self.dump.clone());
                let edges = match horizon_overlay::with(|overlay| {
                    screen.draw(overlay, &status, frames, fingers, touch, &dump)
                }) {
                    Some(Ok(edges)) => edges,
                    Some(Err(error)) => {
                        log::error!("bring-up: frame failed: {error}");
                        event_loop.exit();
                        return;
                    }
                    // The card can still run without the controls; the reason it would
                    // is that no overlay was ever installed, which is a bug in `run`.
                    None => Vec::new(),
                };
                // No game runs behind this card, so what a press did is written down
                // and thrown away: the line is the proof that a tap reached a button.
                for (action, down) in edges {
                    log::info!("bring-up: {} {}", action.text(), if down { "down" } else { "up" });
                }
                if self.frames == 1 {
                    log::info!(
                        "bring-up: first frame after {:?}, sdk {}",
                        self.started.elapsed(),
                        self.app.config().sdk_version()
                    );
                } else if self.frames % 120 == 0 {
                    log::info!(
                        "bring-up: {} frames in {elapsed:?} ({fps:.0} fps), {} touch(es) seen, {status}",
                        self.frames,
                        self.touches,
                    );
                }
            }
            WindowEvent::CloseRequested => event_loop.exit(),
            _ => {}
        }
    }

    /// Android's backend delivers `RedrawRequested` only when something asked for
    /// it: `ControlFlow::Poll` makes the loop wake up immediately, but it does not
    /// request a redraw by itself. Without this the app draws one frame and then
    /// sits in a loop that has nothing to do.
    fn about_to_wait(&mut self, _: &ActiveEventLoop) {
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
    }

    /// The activity is backgrounded: the platform has destroyed the window's
    /// surface, and every Vulkan object made from it is now invalid. Both go
    /// before this callback returns, and `resumed` makes new ones. Keeping them
    /// alive is the classic "black screen after Alt-Tab" on Android.
    fn suspended(&mut self, event_loop: &ActiveEventLoop) {
        horizon_overlay::with(|overlay| overlay.detach());
        self.screen = None;
        self.window = None;
        // `Poll` here spins the loop at full tilt drawing nothing, on a thread the
        // phone expected to be idle. `resumed` asks for Poll back with the new window.
        event_loop.set_control_flow(ControlFlow::Wait);
        log::info!("bring-up: activity suspended, the surface is gone");
    }

    fn exiting(&mut self, _: &ActiveEventLoop) {
        log::info!("bring-up: activity exiting");
    }
}

/// Draw the card on the loop the activity handed over.
///
/// The loop comes in rather than being built here: Android registers winit's handler on
/// the activity the first time a loop is made and refuses a second one, so whoever
/// builds it decides what runs. `notice` says why that is the card and not the game.
pub fn run(
    app: &AndroidApp,
    event_loop: EventLoop<()>,
    notice: &str,
) -> Result<(), Box<dyn Error>> {
    log::info!("bring-up: running the event loop");
    // The on-screen controls are what the player has instead of a keyboard from the
    // first frame onward, so the card carries them: the same thread-global overlay the
    // game would have used, and the only way to tell a dead button from a dead engine.
    horizon_overlay::install(&paths::controls_path(app));
    event_loop.run_app(&mut BringUp {
        app: app.clone(),
        notice: notice.to_owned(),
        installing: false,
        install_job: None,
        cached_installed: None,
        cached_account: None,
        window: None,
        screen: None,
        dump: FrameDump::start(Some(paths::import_dir(app).join("frame.png"))),
        frames: 0,
        touches: 0,
        fingers: 0,
        touch: None,
        started: Instant::now(),
    })?;
    log::info!("bring-up: the event loop ended");
    Ok(())
}
