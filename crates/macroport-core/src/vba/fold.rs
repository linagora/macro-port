// SPDX-License-Identifier: Apache-2.0
//! Constant folding (SPEC M2 item 4), the only evaluation the core makes.
//!
//! The set is closed. An expression has a known value when it is a string
//! literal; the `Chr`, `ChrW` or `ChrB` of a number literal; the `StrReverse`,
//! `LCase` or `UCase` of a known value; a reference to a module-level `Const`
//! that has one; or known values joined with `&` or `+`. Everything else is
//! unknown, and nothing is guessed about it: arithmetic, `Mid`, `Replace`,
//! `vbCrLf`, a variable, a number joined to a string. Folding never runs
//! anything and reads no state but the text of the statement and the
//! constants of the module.
//!
//! The expression reader ([`super::expr`]) walks the expressions; this module
//! holds what it folds with.

use std::collections::BTreeMap;

use super::expr::declare_consts;
use super::lexer::tokenize;
use super::{LogicalKind, LogicalLine};

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
    /// number for the `Chr` family, a string for the others. A string it
    /// builds out of a longer one is paid for out of `budget`.
    pub(super) fn apply(self, argument: &Value, budget: &mut Budget) -> Option<String> {
        match (self, argument) {
            (Self::Chr | Self::ChrW | Self::ChrB, Value::Number(code)) => {
                character(self, *code).map(String::from)
            }
            (Self::StrReverse, Value::Text(text)) => budget
                .take(text.len())
                .then(|| text.chars().rev().collect()),
            (Self::LCase, Value::Text(text)) => {
                budget.take(text.len()).then(|| text.to_lowercase())
            }
            (Self::UCase, Value::Text(text)) => {
                budget.take(text.len()).then(|| text.to_uppercase())
            }
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

/// How many bytes of string one statement may copy or build, so that the
/// text of a statement bounds what folding makes of it: four times its length
/// and a little more. A literal is never held back (it is in the text
/// already), but what is made out of a constant or of a string function is: a
/// constant that a statement of a few bytes repeats into megabytes, or that
/// other constants double along a chain of declarations, stays unresolved
/// instead.
#[derive(Debug, Clone, Copy)]
pub(super) struct Budget(usize);

impl Budget {
    /// The budget of a statement whose text is `text`.
    pub(super) fn for_text(text: &str) -> Self {
        Self(text.len().saturating_mul(4).saturating_add(256))
    }

    /// Takes `bytes` out of the budget. False, and nothing taken, when fewer
    /// are left.
    pub(super) fn take(&mut self, bytes: usize) -> bool {
        match self.0.checked_sub(bytes) {
            Some(left) => {
                self.0 = left;
                true
            }
            None => false,
        }
    }
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

/// The `Const` strings of a module, by name: what a reference to a name folds
/// to. Only the module-level declarations are read (the caller gives them one
/// by one, in order), and a constant whose value is not a known string is not
/// recorded: it folds to nothing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConstTable {
    /// The values by name, in lowercase.
    values: BTreeMap<String, String>,
}

impl ConstTable {
    /// Reads `line`, a logical line of the module outside any procedure (the
    /// caller gives these, in order, before it reads the procedures), and
    /// records the constants it declares whose value folds to a string. A
    /// constant can refer to those declared before it, in this statement or
    /// an earlier one; a name that is declared twice keeps its first value.
    /// Any other line is left alone, and so is a compile directive
    /// (`#Const`), which no reference to a name reads.
    pub fn declare(&mut self, line: &LogicalLine) {
        if line.kind == LogicalKind::Code {
            declare_consts(&line.text, &tokenize(&line.text), self);
        }
    }

    /// The value of the constant called `name`, in any letter case and with
    /// or without a type-declaration character or brackets (`P`, `p$`, `[P]`).
    pub fn get(&self, name: &str) -> Option<&str> {
        let unwrapped = name
            .strip_prefix('[')
            .and_then(|rest| rest.strip_suffix(']'));
        let plain = unwrapped.unwrap_or_else(|| {
            name.strip_suffix(['$', '%', '&', '!', '#', '@'])
                .unwrap_or(name)
        });
        self.lookup(plain)
    }

    /// The value of the constant called `name`, which is as `Token::name`
    /// gives it.
    pub(super) fn lookup(&self, name: &str) -> Option<&str> {
        if self.values.is_empty() {
            // The usual case: nothing to compare the name with.
            return None;
        }
        self.values.get(&name.to_lowercase()).map(String::as_str)
    }

    /// Records `value` for `name` (as `Token::name` gives it), unless the name
    /// has a value already.
    pub(super) fn insert(&mut self, name: &str, value: String) {
        self.values.entry(name.to_lowercase()).or_insert(value);
    }

    /// How many constants are recorded.
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Whether no constant is recorded.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
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
