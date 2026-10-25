//! A way for a shell to see what the phone is showing.
//!
//! Screen capture of a `NativeActivity` comes back as a blank rectangle: the
//! surface the GPU draws into is its own hardware layer, and the frame rate the
//! swapchain reports proves it is on screen without giving a picture of it. So
//! the app is asked directly. A loopback socket takes the frame it just drew,
//! copies it into a buffer, encodes it and sends it back.
//!
//! Nothing is read back unless someone asked, so the cost is a flag check per
//! frame while nobody is looking.

use std::io::{BufRead, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

/// wgpu requires the stride of a texture copy destination to be a multiple of this.
const ROW_ALIGNMENT: u64 = 256;

/// The port the frame server listens on, loopback only.
///
/// A second Horizon clone left running holds the port, so the server walks
/// forward from here until it finds one it can bind — hence the "try 8123..8130"
/// in what it logs.
pub const PORT: u16 = 8123;

/// How many ports forward the server will go before giving up.
const PORT_TRIES: u16 = 8;

pub struct FrameDump {
    /// Set by a waiter, cleared by the frame that answers it.
    asked: AtomicBool,
    /// The newest frame nobody has taken yet.
    png: Mutex<Option<Vec<u8>>>,
    /// Where a copy is dropped for tools that cannot open a socket at all.
    path: Option<PathBuf>,
}

impl FrameDump {
    fn new(path: Option<PathBuf>) -> Self {
        Self {
            asked: AtomicBool::new(false),
            png: Mutex::new(None),
            path,
        }
    }

    /// The frame server, once for the life of the process.
    ///
    /// A second call returns the same dump: the activity is destroyed and
    /// recreated on rotation and on the way out of a suspend, and a listener
    /// cannot be rebound to the port each time.
    pub fn start(path: Option<PathBuf>) -> Arc<FrameDump> {
        static DUMP: OnceLock<Arc<FrameDump>> = OnceLock::new();
        static LISTENER: AtomicBool = AtomicBool::new(false);
        let dump = DUMP.get_or_init(|| Arc::new(FrameDump::new(path))).clone();
        if LISTENER.swap(true, Ordering::SeqCst) {
            return dump;
        }
        let server = dump.clone();
        if std::thread::Builder::new()
            .name("horizon-framedump".into())
            .spawn(move || serve(server))
            .is_err()
        {
            LISTENER.store(false, Ordering::SeqCst);
            log::error!("frame dump: no thread, so nothing to connect to");
        }
        dump
    }

    /// Copy the frame that is about to be presented out of the swapchain.
    ///
    /// Called on the render thread with the texture still acquired: after
    /// `present` the driver owns the image again and there is nothing to read.
    pub fn capture(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        texture: &wgpu::Texture,
        width: u32,
        height: u32,
    ) {
        if !self.asked.swap(false, Ordering::SeqCst) {
            return;
        }
        match self.read_back(device, queue, texture, width, height) {
            Ok(png) => {
                if let Some(path) = &self.path
                    && let Err(error) = write_atomically(path, &png)
                {
                    log::warn!("frame dump: cannot write {}: {error}", path.display());
                }
                *self.png.lock().expect("frame dump lock") = Some(png);
            }
            Err(error) => log::error!("frame dump: {error}"),
        }
    }

    fn read_back(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        texture: &wgpu::Texture,
        width: u32,
        height: u32,
    ) -> Result<Vec<u8>, String> {
        if width == 0 || height == 0 {
            return Err("the surface has no size".into());
        }
        if !texture.usage().contains(wgpu::TextureUsages::COPY_SRC) {
            return Err("this swapchain cannot be copied out (no COPY_SRC)".into());
        }
        let row_bytes = u64::from(width) * 4;
        let padded = row_bytes.div_ceil(ROW_ALIGNMENT) * ROW_ALIGNMENT;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("frame dump"),
            size: padded * u64::from(height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("frame dump"),
        });
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded as u32),
                    rows_per_image: None,
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        queue.submit([encoder.finish()]);
        let (sender, receiver) = std::sync::mpsc::channel();
        buffer.slice(..).map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|error| error.to_string())?;
        receiver
            .recv()
            .map_err(|error| error.to_string())?
            .map_err(|error| error.to_string())?;
        let mapped = buffer
            .slice(..)
            .get_mapped_range()
            .map_err(|error| error.to_string())?;
        let mut rgba = Vec::with_capacity((row_bytes * u64::from(height)) as usize);
        for row in mapped.chunks(padded as usize) {
            rgba.extend_from_slice(&row[..row_bytes as usize]);
        }
        drop(mapped);
        buffer.unmap();
        encode_png(width, height, &rgba)
    }

    /// Ask for the next frame, and wait for the render thread to draw one.
    fn fetch(&self) -> Option<Vec<u8>> {
        for _ in 0..400 {
            self.asked.store(true, Ordering::SeqCst);
            if let Some(png) = self.png.lock().expect("frame dump lock").take() {
                return Some(png);
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        None
    }
}

fn write_atomically(path: &std::path::Path, png: &[u8]) -> std::io::Result<()> {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "frame.png".to_owned());
    let dir = path.parent().unwrap_or_else(|| std::path::Path::new("."));
    // The app's `import` directory only exists once something has been imported
    // into it, and a frame is asked for long before that.
    std::fs::create_dir_all(dir)?;
    std::fs::write(&dir.join(format!("{name}.tmp")), png)?;
    std::fs::rename(&dir.join(format!("{name}.tmp")), path)
}

fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    // The same three calls the engine's own screenshot writer makes.
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|error| error.to_string())?;
    writer
        .write_image_data(rgba)
        .map_err(|error| error.to_string())?;
    writer.finish().map_err(|error| error.to_string())?;
    Ok(out)
}

fn serve(dump: Arc<FrameDump>) {
    let mut listener = None;
    for offset in 0..PORT_TRIES {
        let port = PORT + offset;
        match TcpListener::bind(("127.0.0.1", port)) {
            Ok(bound) => {
                listener = Some((bound, port));
                break;
            }
            // Another clone answers on this port. Nothing to do but move on.
            Err(error) => log::warn!("frame dump: {port} is taken: {error}"),
        }
    }
    let Some((listener, port)) = listener else {
        log::error!("frame dump: no port free in {PORT}..{}", PORT + PORT_TRIES);
        return;
    };
    log::info!("frame dump: http://127.0.0.1:{port}/frame.png");
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let dump = dump.clone();
                std::thread::spawn(move || answer(stream, &dump));
            }
            Err(error) => log::warn!("frame dump: accept failed: {error}"),
        }
    }
}

fn answer(stream: TcpStream, dump: &FrameDump) {
    let mut request = String::new();
    let mut reader = std::io::BufReader::new(stream);
    // The request line is read for its length: a client that connects and says
    // nothing is a probe, and there is no point waiting a frame for it.
    let asked = match reader.read_line(&mut request) {
        Ok(bytes) => bytes > 0,
        Err(error) => {
            log::warn!("frame dump: {error}");
            false
        }
    };
    let path = request.split_whitespace().nth(1).unwrap_or("/");
    // `write_all` and `flush` both take `&mut self`.
    let mut stream = reader.into_inner();
    let (status, headers, body): (&str, &str, Vec<u8>) = if !asked {
        ("400 Bad Request", "Content-Type: text/plain\r\n", Vec::new())
    } else if !path.starts_with("/frame") {
        (
            "404 Not Found",
            "Content-Type: text/plain\r\n",
            b"try /frame.png".to_vec(),
        )
    } else {
        match dump.fetch() {
            Some(png) => (
                "200 OK",
                "Content-Type: image/png\r\n",
                png,
            ),
            None => (
                "504 Gateway Timeout",
                "Content-Type: text/plain\r\n",
                b"the render loop never answered".to_vec(),
            ),
        }
    };
    let head = format!(
        "HTTP/1.0 {status}\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream
        .write_all(head.as_bytes())
        .and_then(|()| stream.write_all(&body))
        .and_then(|()| stream.flush());
}
