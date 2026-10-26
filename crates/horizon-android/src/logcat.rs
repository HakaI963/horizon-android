//! logcat, so `log::info!` from the engine and Rust panics reach `adb logcat`
//! instead of a stderr nobody reads on a phone.

use std::ffi::{CStr, CString, c_char, c_int};

const VERBOSE: c_int = 2;
const DEBUG: c_int = 3;
const INFO: c_int = 4;
const WARN: c_int = 5;
const ERROR: c_int = 6;

unsafe extern "C" {
    #[link_name = "__android_log_write"]
    fn write(prio: c_int, tag: *const c_char, text: *const c_char) -> c_int;
}

fn log(prio: c_int, tag: &CStr, text: &str) {
    // A single embedded NUL would truncate the message, and backtraces are long:
    // logcat cuts a line at about 4 KB anyway, so split on newlines.
    for line in text.lines() {
        if line.is_empty() {
            continue;
        }
        if let Ok(text) = CString::new(line) {
            unsafe { write(prio, tag.as_ptr(), text.as_ptr()) };
        }
    }
}

struct Logcat;

/// The shader compiler logs one line per WGSL instruction and wgpu one per
/// resource. On a phone that fills the ring buffer in a second and buries the
/// engine's own lines, which is the reason the log is being read at all. Their
/// warnings and errors still come through. `ureq` and `rustls` are the same
/// problem while the launcher fetches sounds: a dozen lines per object, for
/// thousands of objects.
fn chatty(target: &str) -> bool {
    target.starts_with("naga")
        || target.starts_with("wgpu")
        || target.starts_with("gpu_allocator")
        || target.starts_with("ureq")
        || target.starts_with("rustls")
}

impl log::Log for Logcat {
    fn enabled(&self, meta: &log::Metadata<'_>) -> bool {
        // `Level` orders itself with the noisiest last, so "not above Info" is
        // "still important".
        !chatty(meta.target()) || meta.level() <= log::Level::Info
    }
    fn log(&self, record: &log::Record<'_>) {
        // The `log!` macro only consults the static max level, never `enabled`,
        // so the filter has to be checked here as well.
        if !self.enabled(record.metadata()) {
            return;
        }
        let prio = match record.level() {
            log::Level::Trace => VERBOSE,
            log::Level::Debug => DEBUG,
            log::Level::Info => INFO,
            log::Level::Warn => WARN,
            log::Level::Error => ERROR,
        };
        let Ok(tag) = CString::new(record.target()) else {
            return;
        };
        log(prio, &tag, &format!("{}: {}", record.level(), record.args()));
    }
    fn flush(&self) {}
}

static LOGCAT: Logcat = Logcat;

/// Sends `log` records and panics to logcat under one tag, and keeps the panic
/// message: an Android process that dies with a Rust panic leaves nothing behind
/// but an empty screen otherwise.
pub fn init() {
    // `&'static`, so no `alloc` feature is needed for the logger itself; calling
    // twice (the activity can be recreated) keeps the first logger.
    let _ = log::set_logger(&LOGCAT);
    log::set_max_level(log::LevelFilter::Debug);
    std::panic::set_hook(Box::new(|info| {
        let tag = CString::new("horizon.panic").expect("static tag");
        let mut report = info.to_string();
        if let Some(location) = info.location() {
            report.push_str(&format!(" at {location}"));
        }
        log(ERROR, &tag, &report);
    }));
}
