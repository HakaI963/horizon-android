//! The phone's soft keyboard, as the game reads it.
//!
//! winit's Android backend can raise and lower the system IME, but it delivers no
//! text: a phone's keyboard commits words, not `KeyEvent`s, and winit cannot build
//! one for a crate outside itself (`KeyEvent`'s platform half is private). So the
//! platform hands the engine strings instead — everything the field holds now, and
//! whether Enter or a delete came in — and the engine says those as the edits the
//! screens' own text entry already takes.
//!
//! The platform field is not the game's field: nothing is drawn from it. It only
//! starts an IME, and its whole text is polled a frame at a time. What changed is
//! found against the last poll, so a word typed whole, a backspace, an IME's
//! autocorrection and a paste all arrive as edits.

use std::sync::{Mutex, OnceLock};

/// What a platform has to provide for the soft keyboard to reach the game.
///
/// Function pointers rather than a trait: the engine forbids `unsafe_code`, and the
/// platform half is the only place JNI may live. Installing none is a computer,
/// where the real keyboard works.
pub struct SystemHooks {
    /// Put the keyboard up, with an empty field.
    pub show: fn(),
    /// Put it away.
    pub hide: fn(),
    /// Empty the field without closing the keyboard, for a screen that moved to a
    /// different field (the next sign line).
    pub reset: fn(),
    /// Everything the platform's field holds now; empty while it is not open.
    pub text: fn() -> String,
    /// Bit 0: Enter was pressed. Bits 1..: the number of deletes on an empty field,
    /// which change no text and would otherwise be invisible.
    pub keys: fn() -> i32,
}

static SYSTEM: OnceLock<SystemHooks> = OnceLock::new();
/// The platform field's text as of the last poll. The difference from the field is
/// what the screen is told.
static MIRROR: Mutex<String> = Mutex::new(String::new());

/// Register the platform's keyboard. False when one was already registered.
pub fn install_system(hooks: SystemHooks) -> bool {
    SYSTEM.set(hooks).is_ok()
}

/// What has happened in the keyboard's field since the last poll.
pub struct Input {
    /// Characters to delete from the caret.
    pub backspaces: usize,
    /// What was typed in their place.
    pub text: String,
    pub enter: bool,
}

/// Put the keyboard up with an empty field to read from.
pub fn show() {
    clear_mirror();
    if let Some(hooks) = SYSTEM.get() {
        (hooks.show)();
    }
}

/// Put it away.
pub fn hide() {
    clear_mirror();
    if let Some(hooks) = SYSTEM.get() {
        (hooks.hide)();
    }
}

/// Empty both sides: the next word belongs to a different field, not the one the
/// keyboard was showing.
pub fn reset() {
    clear_mirror();
    if let Some(hooks) = SYSTEM.get() {
        (hooks.reset)();
    }
}

/// The edits since the last call, or `None` when nothing happened.
pub fn poll() -> Option<Input> {
    let hooks = SYSTEM.get()?;
    let now = (hooks.text)();
    let keys = (hooks.keys)();
    let enter = keys & 1 != 0;
    let deletes = (keys >> 1).max(0) as usize;
    let mut mirror = MIRROR
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let (mut backspaces, text) = if now == *mirror {
        (0, String::new())
    } else {
        diff(&mirror, &now)
    };
    // A delete that leaves the field empty has no text change to be seen in, and
    // it is how the first letter typed over a selected value is taken back.
    if backspaces == 0 && text.is_empty() && now.is_empty() {
        backspaces = deletes;
    }
    *mirror = now;
    if backspaces == 0 && text.is_empty() && !enter {
        return None;
    }
    Some(Input {
        backspaces,
        text,
        enter,
    })
}

fn clear_mirror() {
    MIRROR
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clear();
}

/// The edits that turn `old` into `new`: how many characters the end of the old
/// value loses, and what is typed in their place. A pure cursor move is no edit.
fn diff(old: &str, new: &str) -> (usize, String) {
    let old: Vec<char> = old.chars().collect();
    let new: Vec<char> = new.chars().collect();
    let mut prefix = 0;
    while prefix < old.len() && prefix < new.len() && old[prefix] == new[prefix] {
        prefix += 1;
    }
    let mut suffix = 0;
    while suffix < old.len() - prefix
        && suffix < new.len() - prefix
        && old[old.len() - 1 - suffix] == new[new.len() - 1 - suffix]
    {
        suffix += 1;
    }
    let removed = old.len() - prefix - suffix;
    let inserted = new[prefix..new.len() - suffix].iter().collect();
    (removed, inserted)
}

#[cfg(test)]
mod tests {
    use super::diff;

    #[test]
    fn typing_appends() {
        assert_eq!(diff("", "a"), (0, "a".into()));
        assert_eq!(diff("ab", "abc"), (0, "c".into()));
        assert_eq!(diff("", "hello world"), (0, "hello world".into()));
    }

    #[test]
    fn backspace_removes_the_end() {
        assert_eq!(diff("ab", "a"), (1, String::new()));
        assert_eq!(diff("abc", ""), (3, String::new()));
    }

    #[test]
    fn a_replaced_selection_is_what_changed() {
        assert_eq!(diff("hello", "y"), (5, "y".into()));
        assert_eq!(diff("abc", "axc"), (1, "x".into()));
    }

    #[test]
    fn characters_are_counted_not_bytes() {
        assert_eq!(diff("😀", ""), (1, String::new()));
        assert_eq!(diff("a😀", "a"), (1, String::new()));
    }
}
