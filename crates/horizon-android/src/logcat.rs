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

impl log::Log for Logcat {
    fn enabled(&self, _: &log::Metadata<'_>) -> bool {
        true
    }
    fn log(&self, record: &log::Record<'_>) {
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

/// Sends `log` records and panics to logcat under one tag, and keeps the panic
/// message: an Android process that dies with a Rust panic leaves nothing behind
/// but an empty screen otherwise.
pub fn init() {
    let _ = log::set_boxed_logger(Box::new(Logcat));
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
