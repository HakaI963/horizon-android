//! The little arithmetic language a layout's position is written in.
//!
//! A button in a layout file does not have a position, it has a formula:
//! `${margin} * 3 + ${width} * 2`, `${bottom} - ${margin}`. The point is that the
//! same file describes the same pad on a phone and on a tablet: a button anchored
//! to the right edge stays on the right edge, and one placed after another follows
//! it when the other grows.
//!
//! The language is the one the format has always used — numbers, `+ - * /`,
//! parentheses, and `${name}` for a value supplied by the caller. Nothing else is
//! needed and nothing else is accepted: an unknown name is a layout that cannot be
//! placed, and saying so is better than drawing a button at zero.

use std::collections::BTreeMap;

/// A value the caller supplies for a `${name}` in a formula.
pub type Values = BTreeMap<String, f32>;

/// Evaluate `formula` against `values`.
///
/// Returns `None` for a formula that does not parse, names something absent, or
/// divides by zero: all three are a layout the caller cannot place, and a button at
/// the origin is worse than no button.
pub fn eval(formula: &str, values: &Values) -> Option<f32> {
    let expanded = expand(formula, values)?;
    let mut parser = Parser {
        text: expanded.as_bytes(),
        at: 0,
    };
    let value = parser.sum()?;
    parser.space();
    if parser.at != parser.text.len() {
        return None;
    }
    value.is_finite().then_some(value)
}

/// Replace every `${name}` with its value, in decimal.
///
/// The substitution is textual because that is what the format means: the formula
/// is arithmetic over the numbers, not over a value tree, and `${width} * 2` has to
/// become a multiplication rather than a doubled string.
fn expand(formula: &str, values: &Values) -> Option<String> {
    let mut out = String::with_capacity(formula.len() + 32);
    let mut rest = formula;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let end = after.find('}')?;
        let name = &after[..end];
        let value = values.get(name)?;
        // `{:?}`-free formatting: a value that came out non-finite would make the
        // formula unparsable, which is the answer we want anyway.
        out.push_str(&format!("{value}"));
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Some(out)
}

struct Parser<'a> {
    text: &'a [u8],
    at: usize,
}

impl Parser<'_> {
    fn space(&mut self) {
        while self.at < self.text.len() && self.text[self.at].is_ascii_whitespace() {
            self.at += 1;
        }
    }

    /// `+` and `-`, left to right.
    fn sum(&mut self) -> Option<f32> {
        let mut value = self.product()?;
        loop {
            self.space();
            match self.text.get(self.at) {
                Some(b'+') => {
                    self.at += 1;
                    value += self.product()?;
                }
                Some(b'-') => {
                    self.at += 1;
                    value -= self.product()?;
                }
                _ => return Some(value),
            }
        }
    }

    /// `*` and `/`, left to right.
    fn product(&mut self) -> Option<f32> {
        let mut value = self.unary()?;
        loop {
            self.space();
            match self.text.get(self.at) {
                Some(b'*') => {
                    self.at += 1;
                    value *= self.unary()?;
                }
                Some(b'/') => {
                    self.at += 1;
                    let divisor = self.unary()?;
                    if divisor == 0.0 {
                        return None;
                    }
                    value /= divisor;
                }
                _ => return Some(value),
            }
        }
    }

    /// A leading sign, then a number or a parenthesised sum.
    fn unary(&mut self) -> Option<f32> {
        self.space();
        match self.text.get(self.at) {
            Some(b'-') => {
                self.at += 1;
                Some(-self.unary()?)
            }
            Some(b'+') => {
                self.at += 1;
                self.unary()
            }
            _ => self.atom(),
        }
    }

    fn atom(&mut self) -> Option<f32> {
        self.space();
        if self.text.get(self.at) == Some(&b'(') {
            self.at += 1;
            let value = self.sum()?;
            self.space();
            if self.text.get(self.at) != Some(&b')') {
                return None;
            }
            self.at += 1;
            return Some(value);
        }
        let start = self.at;
        while self.at < self.text.len()
            && (self.text[self.at].is_ascii_digit()
                || self.text[self.at] == b'.'
                || self.text[self.at] == b'e'
                || self.text[self.at] == b'E')
        {
            self.at += 1;
        }
        if self.at == start {
            return None;
        }
        std::str::from_utf8(&self.text[start..self.at])
            .ok()?
            .parse()
            .ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values() -> Values {
        let mut values = Values::new();
        values.insert("margin".into(), 2.0);
        values.insert("width".into(), 50.0);
        values.insert("height".into(), 50.0);
        values.insert("screen_width".into(), 1000.0);
        values.insert("screen_height".into(), 500.0);
        values.insert("right".into(), 950.0);
        values.insert("bottom".into(), 450.0);
        values
    }

    #[test]
    fn arithmetic_and_names() {
        let v = values();
        assert_eq!(eval("${margin}", &v), Some(2.0));
        assert_eq!(eval("${margin} * 3 + ${width} * 2", &v), Some(106.0));
        assert_eq!(eval("${bottom} - ${margin}", &v), Some(448.0));
        assert_eq!(eval("${screen_height} - ${margin} * 3 - ${height} * 3", &v), Some(344.0));
        assert_eq!(eval("(${margin} + 1) * 2", &v), Some(6.0));
    }

    #[test]
    fn a_name_nobody_supplied_is_not_a_position() {
        let v = values();
        assert_eq!(eval("${nope} + 1", &v), None);
        assert_eq!(eval("1 / 0", &v), None);
        assert_eq!(eval("1 +", &v), None);
        assert_eq!(eval("", &v), None);
    }

    #[test]
    fn fractions_of_the_screen_parse() {
        let v = values();
        assert_eq!(eval("0.5 * ${screen_width}", &v), Some(500.0));
        assert_eq!(eval("0.7 * ${screen_width} - ${width}", &v), Some(650.0));
    }
}
