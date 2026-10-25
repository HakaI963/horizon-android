//! Stage A of the port: prove the platform layer on real hardware before the
//! engine is wired in.
//!
//! It opens the Vulkan surface through winit's `NativeActivity` window, clears
//! the swapchain every frame and reports what the touchscreen sends. On a phone
//! the things that can only be found on a phone are the swapchain format, the
//! surface going stale when the activity is backgrounded, the resolution
//! changing when the device rotates, and touch arriving at all — so this stage
//! watches exactly those four.

use std::error::Error;
use std::sync::Arc;
use std::time::Instant;

use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{Touch, TouchPhase, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::platform::android::EventLoopBuilderExtAndroid;
use winit::platform::android::activity::AndroidApp;
use winit::window::{Window, WindowId};

/// The device and the swapchain it presents to.
struct Screen {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
}

pub struct BringUp {
    app: AndroidApp,
    window: Option<Arc<Window>>,
    screen: Option<Screen>,
    frames: u64,
    touches: u64,
    fingers: u32,
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
    config.alpha_mode = wgpu::CompositeAlphaMode::Opaque;
    config.present_mode = wgpu::PresentMode::Fifo;
    log::info!(
        "bring-up: configuring {}x{} {:?} {:?}, scale {}, content {:?}, sdk {}",
        config.width,
        config.height,
        config.format,
        config.present_mode,
        window.scale_factor(),
        app.content_rect(),
        app.config().sdk_version()
    );
    surface.configure(&device, &config);
    Ok(Screen {
        surface,
        device,
        queue,
        config,
    })
}

impl Screen {
    /// A slow hue cycle: proof that frames reach the display and keep reaching it.
    fn clear_color(frame: u64, fingers: u32) -> wgpu::Color {
        let phase = (frame as f64 / 90.0).sin();
        wgpu::Color {
            r: (0.25 + 0.2 * phase) * 255.0,
            g: (0.35 + 0.15 * (1.0 - phase) + 0.1 * fingers as f64) * 255.0,
            b: (0.6 + 0.2 * phase) * 255.0,
            a: 255.0,
        }
    }

    fn draw(&mut self, frame: u64, fingers: u32) -> Result<(), Box<dyn Error>> {
        let acquired = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Outdated => {
                self.surface.configure(&self.device, &self.config);
                return Ok(());
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
                load: wgpu::LoadOp::Clear(Self::clear_color(frame, fingers)),
                store: wgpu::StoreOp::Store,
            },
        })];
        // Nothing to bind: the clear is the whole point of this stage.
        drop(
            encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Horizon bring-up clear"),
                color_attachments: &attachments,
                depth_stencil_attachment: None,
                ..Default::default()
            }),
        );
        self.queue.submit([encoder.finish()]);
        // No `pre_present_notify()`: that paces winit's Wayland frame callbacks,
        // and Android's ANativeWindow queue paces FIFO by itself.
        self.queue.present(acquired);
        Ok(())
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
            }
            WindowEvent::Touch(Touch {
                phase,
                id,
                location,
                force,
                ..
            }) => {
                match phase {
                    TouchPhase::Started => {
                        self.touches += 1;
                        self.fingers += 1;
                        log::info!(
                            "bring-up: touch {id} down at {:.0}x{:.0} force {:?}",
                            location.x,
                            location.y,
                            force
                        );
                    }
                    TouchPhase::Moved => {}
                    TouchPhase::Ended | TouchPhase::Cancelled => {
                        self.fingers = self.fingers.saturating_sub(1);
                        log::info!("bring-up: touch {id} up ({} fingers left)", self.fingers);
                    }
                }
            }
            WindowEvent::RedrawRequested => {
                let Some(screen) = self.screen.as_mut() else {
                    return;
                };
                self.frames += 1;
                if let Err(error) = screen.draw(self.frames, self.fingers) {
                    log::error!("bring-up: frame failed: {error}");
                    event_loop.exit();
                    return;
                }
                if self.frames == 1 {
                    log::info!(
                        "bring-up: first frame after {:?}, sdk {}",
                        self.started.elapsed(),
                        self.app.config().sdk_version()
                    );
                } else if self.frames % 300 == 0 {
                    log::info!(
                        "bring-up: {} frames, {} touch(es) seen",
                        self.frames,
                        self.touches
                    );
                }
            }
            WindowEvent::CloseRequested => event_loop.exit(),
            _ => {}
        }
    }

    fn suspended(&mut self, _: &ActiveEventLoop) {
        log::info!("bring-up: activity suspended");
    }

    fn exiting(&mut self, _: &ActiveEventLoop) {
        log::info!("bring-up: activity exiting");
    }
}

pub fn run(app: &AndroidApp) -> Result<(), Box<dyn Error>> {
    let mut builder = EventLoop::builder();
    builder.with_android_app(app.clone());
    let event_loop = builder.build()?;
    log::info!("bring-up: running the event loop");
    event_loop.run_app(&mut BringUp {
        app: app.clone(),
        window: None,
        screen: None,
        frames: 0,
        touches: 0,
        fingers: 0,
        started: Instant::now(),
    })?;
    log::info!("bring-up: the event loop ended");
    Ok(())
}
