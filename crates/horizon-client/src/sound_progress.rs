//! How far the sound pack has got, for a launcher to draw.
//!
//! The sounds fill in on a background thread that must not touch the overlay: the
//! overlay is a thread-local belonging to the render thread, and a worker reaching
//! into it is a borrow of something another thread owns. So the worker writes three
//! plain atomics and whoever draws reads them, which is the same shape as the
//! world-loading `Progress` and the frame-dump counters.

use std::sync::atomic::{AtomicU32, AtomicU8, Ordering};

const IDLE: u8 = 0;
const RUNNING: u8 = 1;
const COMPLETE: u8 = 2;
const FAILED: u8 = 3;

static DONE: AtomicU32 = AtomicU32::new(0);
static TOTAL: AtomicU32 = AtomicU32::new(0);
static STATE: AtomicU8 = AtomicU8::new(IDLE);

/// The worker found how many sound objects the version has, before fetching any.
pub fn begin(total: u32) {
    TOTAL.store(total, Ordering::Relaxed);
    DONE.store(0, Ordering::Relaxed);
    STATE.store(RUNNING, Ordering::Relaxed);
}

/// One more file is on disk (or was already there).
pub fn step() {
    let done = DONE.fetch_add(1, Ordering::Relaxed) + 1;
    let total = TOTAL.load(Ordering::Relaxed);
    // A file counted past the total would make the bar read over 100%; the total is
    // read from the same index the loop walks, so this is a guard, not a policy.
    if total > 0 && done >= total {
        STATE.store(COMPLETE, Ordering::Relaxed);
    }
}

/// The pass ended without every file: it retries on the next launch, so the bar
/// stops rather than pretending to be finished.
pub fn stop() {
    if STATE.load(Ordering::Relaxed) == RUNNING {
        STATE.store(FAILED, Ordering::Relaxed);
    }
}

/// Every sound the version has is on disk.
pub fn complete() {
    STATE.store(COMPLETE, Ordering::Relaxed);
}

/// The line to draw, and the fraction behind it, or `None` when there is nothing to
/// say: no pass has run, or the pack was already complete before this launch.
pub fn snapshot() -> Option<(String, f32)> {
    let state = STATE.load(Ordering::Relaxed);
    let (done, total) = (
        DONE.load(Ordering::Relaxed),
        TOTAL.load(Ordering::Relaxed),
    );
    match state {
        RUNNING if total > 0 => {
            let fraction = (done as f32 / total as f32).clamp(0.0, 1.0);
            Some((
                format!(
                    "Sounds {done} of {total} ({:.0}%)",
                    fraction * 100.0
                ),
                fraction,
            ))
        }
        RUNNING => Some(("Fetching Minecraft's sounds".to_string(), 0.0)),
        // A finished or abandoned pass leaves the status line to the launcher: only a
        // pass in flight has something worth reading every frame.
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The atomics are process-wide, so the tests that touch them run in one test:
    /// two of them in parallel would read each other's counters.
    #[test]
    fn a_pass_reports_its_fraction_and_ends_complete() {
        assert!(snapshot().is_none(), "nothing has run yet");
        begin(4);
        let (line, fraction) = snapshot().expect("a running pass has a line");
        assert!(line.contains("0 of 4"), "{line}");
        assert_eq!(fraction, 0.0);
        for _ in 0..2 {
            step();
        }
        let (line, fraction) = snapshot().expect("still running");
        assert!(line.contains("2 of 4"), "{line}");
        assert!((fraction - 0.5).abs() < f32::EPSILON);
        step();
        step();
        assert!(snapshot().is_none(), "a complete pack has nothing to say");
    }

    #[test]
    fn a_pass_that_gives_up_stops_saying_anything() {
        begin(10);
        step();
        stop();
        assert!(snapshot().is_none());
    }
}
