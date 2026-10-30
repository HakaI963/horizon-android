//! The two key vocabularies a layout file can be written in.
//!
//! Layout files carry key codes, and which codes depends on the file's age: the
//! oldest ones use the desktop's GLFW codes, newer ones Android's own. A file says
//! which with its `version`, and the two ranges barely overlap, so this reads the
//! version when it is there and the numbers when it is not.
//!
//! Everything ends up as an Android code, because that is what the rest of the pad
//! speaks and what the engine is asked for.

/// GLFW key codes that a layout file may carry, and their Android equivalents.
const GLFW_TO_ANDROID: &[(i32, i32)] = &[
    (32, 62),   // space
    (39, 75),   // apostrophe
    (44, 55),   // comma
    (45, 69),   // minus
    (46, 56),   // period
    (47, 76),   // slash
    (48, 7),    // 0
    (49, 8),    // 1
    (50, 9),    // 2
    (51, 10),   // 3
    (52, 11),   // 4
    (53, 12),   // 5
    (54, 13),   // 6
    (55, 14),   // 7
    (56, 15),   // 8
    (57, 16),   // 9
    (59, 74),   // semicolon
    (61, 70),   // equal
    (65, 29),   // A
    (66, 30),   // B
    (67, 31),   // C
    (68, 32),   // D
    (69, 33),   // E
    (70, 34),   // F
    (71, 35),   // G
    (72, 36),   // H
    (73, 37),   // I
    (74, 38),   // J
    (75, 39),   // K
    (76, 40),   // L
    (77, 41),   // M
    (78, 42),   // N
    (79, 43),   // O
    (80, 44),   // P
    (81, 45),   // Q
    (82, 46),   // R
    (83, 47),   // S
    (84, 48),   // T
    (85, 49),   // U
    (86, 50),   // V
    (87, 51),   // W
    (88, 52),   // X
    (89, 53),   // Y
    (90, 54),   // Z
    (91, 71),   // left bracket
    (92, 73),   // backslash
    (93, 72),   // right bracket
    (96, 68),   // grave accent
    (256, 111), // escape
    (257, 66),  // enter
    (258, 61),  // tab
    (259, 67),  // backspace
    (260, 124), // insert
    (261, 112), // delete
    (262, 22),  // right
    (263, 21),  // left
    (264, 20),  // down
    (265, 19),  // up
    (266, 92),  // page up
    (267, 93),  // page down
    (268, 122), // home
    (269, 123), // end
    (280, 115), // caps lock
    (281, 116), // scroll lock
    (282, 143), // num lock
    (283, 120), // print screen
    (284, 121), // pause
    (320, 144), // keypad 0
    (321, 145), // keypad 1
    (322, 146), // keypad 2
    (323, 147), // keypad 3
    (324, 148), // keypad 4
    (325, 149), // keypad 5
    (326, 150), // keypad 6
    (327, 151), // keypad 7
    (328, 152), // keypad 8
    (329, 153), // keypad 9
    (340, 59),  // left shift
    (341, 113), // left control
    (342, 57),  // left alt
    (344, 60),  // right shift
    (345, 114), // right control
    (346, 58),  // right alt
];

/// F1..F12 are contiguous in Android's table but not in the list above, so the three
/// that would have been written as arithmetic are spelled out here.
pub fn android_f(n: i32) -> i32 {
    131 + (n - 1)
}

/// Whether a layout file's key codes are GLFW's rather than Android's.
///
/// The version is the honest answer when a file has one. Without it, the numbers
/// decide: Android's own table stops below 300, and GLFW's special keys start above
/// 255, so a file carrying any of those cannot be Android.
pub fn is_glfw(version: Option<i32>, codes: impl Iterator<Item = i32>) -> bool {
    match version {
        // Version 9 is the one that switched to Android codes; everything before it
        // carried GLFW's.
        Some(version) => version < 9,
        None => codes.into_iter().any(|code| code > 255),
    }
}

/// An Android key code for a code from whichever vocabulary the file used.
pub fn to_android(code: i32, glfw: bool) -> i32 {
    if !glfw || code <= 0 {
        return code;
    }
    // The function keys are contiguous on both sides and are the one family worth
    // computing rather than tabling.
    if (290..=301).contains(&code) {
        return android_f(code - 289);
    }
    GLFW_TO_ANDROID
        .iter()
        .find(|(glfw, _)| *glfw == code)
        .map(|(_, android)| *android)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_pad_is_glfw() {
        assert!(is_glfw(Some(4), std::iter::empty()));
        assert!(is_glfw(None, [340, 87].into_iter()));
    }

    #[test]
    fn a_modern_file_is_android() {
        assert!(!is_glfw(Some(9), std::iter::empty()));
    }

    #[test]
    fn the_keys_a_pad_actually_uses_convert() {
        assert_eq!(to_android(87, true), 51); // W
        assert_eq!(to_android(65, true), 29); // A
        assert_eq!(to_android(83, true), 47); // S
        assert_eq!(to_android(68, true), 32); // D
        assert_eq!(to_android(69, true), 33); // E
        assert_eq!(to_android(32, true), 62); // space
        assert_eq!(to_android(340, true), 59); // left shift
        assert_eq!(to_android(84, true), 48); // T
        assert_eq!(to_android(258, true), 61); // tab
        assert_eq!(to_android(292, true), 133); // F3
        assert_eq!(to_android(294, true), 135); // F5
        assert_eq!(to_android(51, false), 51); // already Android
    }
}
