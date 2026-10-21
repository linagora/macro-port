// SPDX-License-Identifier: Apache-2.0
//! Constant folding (SPEC M2 item 4), the only evaluation the core makes.
//!
//! The set is closed. An expression has a known value when it is a string
//! literal; the `Chr`, `ChrW` or `ChrB` of a number literal; the `StrReverse`,
//! `LCase` or `UCase` of a known value; or known values joined with `&` or
//! `+`. Everything else is unknown, and nothing is guessed about it:
//! arithmetic, `Mid`, `Replace`, `vbCrLf`, a variable, a number joined to a
//! string. Folding never runs anything and reads no state but the text of the
//! statement.
//!
//! The expression reader ([`super::expr`]) walks the expressions; this module
//! holds what it folds with.

/// The functions of the closed set that make or change a string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Folding {
    /// `Chr`: the character of an ANSI code.
    Chr,
    /// `ChrW`: the character of a UTF-16 code unit.
    ChrW,
    /// `ChrB`: the one-byte string of a byte.
    ChrB,
    /// `StrReverse`
    StrReverse,
    /// `LCase`
    LCase,
    /// `UCase`
    UCase,
}

impl Folding {
    /// The function called `name`, in any letter case and without its
    /// type-declaration character (`Chr$` is `Chr`).
    pub(super) fn named(name: &str) -> Option<Self> {
        [
            ("chr", Self::Chr),
            ("chrw", Self::ChrW),
            ("chrb", Self::ChrB),
            ("strreverse", Self::StrReverse),
            ("lcase", Self::LCase),
            ("ucase", Self::UCase),
        ]
        .into_iter()
        .find(|(spelling, _)| name.eq_ignore_ascii_case(spelling))
        .map(|(_, found)| found)
    }

    /// What the function makes of `argument`, if it is one it folds: a whole
    /// number for the `Chr` family, a string for the others.
    pub(super) fn apply(self, argument: &Value) -> Option<String> {
        match (self, argument) {
            (Self::Chr | Self::ChrW | Self::ChrB, Value::Number(code)) => {
                character(self, *code).map(String::from)
            }
            (Self::StrReverse, Value::Text(text)) => Some(text.chars().rev().collect()),
            (Self::LCase, Value::Text(text)) => Some(text.to_lowercase()),
            (Self::UCase, Value::Text(text)) => Some(text.to_uppercase()),
            _ => None,
        }
    }
}

/// The value an expression is known to have.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Value {
    /// Anything the closed set does not cover.
    Unknown,
    /// A whole number literal that fits: only `Chr` and its kin read it.
    Number(u32),
    /// A string.
    Text(String),
}

/// The value of a number literal that is a whole number, in decimal, in
/// hexadecimal (`&H41`) or in octal (`&O101`, `&101`), with the `%` or `&`
/// that may end it. `None` for a fraction, an exponent, a type character that
/// makes it a real, and a number that does not fit.
pub(super) fn parse_number(text: &str) -> Option<u32> {
    let bytes = text.as_bytes();
    let (radix, digits) = match bytes {
        [b'&', b'h' | b'H', rest @ ..] => (16, rest),
        [b'&', b'o' | b'O', rest @ ..] => (8, rest),
        [b'&', rest @ ..] => (8, rest),
        _ => (10, bytes),
    };
    let digits = match digits.split_last() {
        Some((b'%' | b'&', rest)) => rest,
        _ => digits,
    };
    u32::from_str_radix(std::str::from_utf8(digits).ok()?, radix).ok()
}

/// The character that `function` makes of `code`. `Chr` and `ChrB` take a
/// byte, which is read as the character of the same number (Latin-1): the code
/// page of the project is not known here, and the codes above 127 are not what
/// the obfuscated names are made of. `ChrW` takes a UTF-16 code unit, so a
/// surrogate is none. `None` outside what the function accepts.
fn character(function: Folding, code: u32) -> Option<char> {
    let limit = match function {
        Folding::Chr | Folding::ChrB => 255,
        Folding::ChrW => 65_535,
        _ => return None,
    };
    if code > limit {
        return None;
    }
    char::from_u32(code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_whole_number_literal_has_a_value_in_any_base() {
        for (text, expected) in [
            ("0", 0),
            ("065", 65),
            ("65%", 65),
            ("65&", 65),
            ("&H41", 65),
            ("&h41&", 65),
            ("&Hff%", 255),
            ("&O101", 65),
            ("&101", 65),
            ("4294967295", u32::MAX),
        ] {
            assert_eq!(parse_number(text), Some(expected), "{text}");
        }
    }

    #[test]
    fn a_number_that_is_not_a_whole_number_that_fits_has_no_value() {
        for text in [
            "",
            "&",
            "&H",
            "65!",
            "65#",
            "65@",
            "6.5",
            "1E3",
            "-65",
            "&H41x",
            "&O9",
            "4294967296",
            "99999999999999999999",
            "٦٥",
        ] {
            assert_eq!(parse_number(text), None, "{text:?}");
        }
    }

    #[test]
    fn a_character_is_made_of_a_code_that_the_function_accepts() {
        assert_eq!(character(Folding::Chr, 65), Some('A'));
        assert_eq!(character(Folding::Chr, 255), Some('\u{ff}'));
        assert_eq!(character(Folding::Chr, 256), None);
        assert_eq!(character(Folding::ChrB, 256), None);
        assert_eq!(character(Folding::ChrW, 8364), Some('\u{20ac}'));
        assert_eq!(character(Folding::ChrW, 65_536), None);
        // A surrogate is no character.
        assert_eq!(character(Folding::ChrW, 0xd800), None);
        assert_eq!(character(Folding::ChrW, 0xdfff), None);
    }

    #[test]
    fn a_function_of_the_set_is_found_by_name_in_any_letter_case() {
        assert_eq!(Folding::named("CHRW"), Some(Folding::ChrW));
        assert_eq!(Folding::named("strreverse"), Some(Folding::StrReverse));
        for name in ["Chr$", "Asc", "Chrx", ""] {
            assert_eq!(Folding::named(name), None, "{name}");
        }
    }
}
