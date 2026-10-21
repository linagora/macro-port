// SPDX-License-Identifier: Apache-2.0
//! Constant folding (SPEC M2 item 4), the only evaluation the core makes.
//!
//! The set is closed. An expression has a known value when it is a string
//! literal; the `Chr` or `ChrW` of a number literal; the `StrReverse`, `LCase`
//! or `UCase` of a known value; a reference to a module-level `Const` that has
//! one; or known values joined with `&` or `+`. Everything else is unknown,
//! and nothing is guessed about it: arithmetic, `Mid`, `Replace`, `vbCrLf`, a
//! variable, a number joined to a string, and `ChrB`, which makes a one-byte
//! string that only another byte completes into a character. Folding never
//! runs anything and reads no state but the text of the statement and the
//! constants of the module.
//!
//! The expression reader ([`super::expr`]) walks the expressions; this module
//! holds what it folds with.

use std::collections::BTreeMap;

use super::expr::declare_consts;
use super::lexer::tokenize;
use super::{LogicalKind, LogicalLine};

/// How many bytes the names and the strings of the constants of a module may
/// take together. The budget of a statement bounds what it builds, but a
/// module has many statements; real constants take a few kilobytes.
const MAX_CONST_BYTES: usize = 8 * 1024 * 1024;

/// The highest code of `Chr` that stands for the same character on every
/// machine: ASCII. The other codes of a byte are those of the code page.
const ASCII_MAX: u32 = 127;

/// The highest code of `ChrW`: a UTF-16 code unit.
const UTF16_MAX: u32 = 65_535;

/// How many bytes of string a statement may build out of others, beyond four
/// times its own text: enough for a short statement to join a few constants.
const BUDGET_FLOOR: usize = 1024;

/// The functions of the closed set that make or change a string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Folding {
    /// `Chr`: the character of an ASCII code.
    Chr,
    /// `ChrW`: the character of a UTF-16 code unit.
    ChrW,
    /// `StrReverse`
    StrReverse,
    /// `LCase`
    LCase,
    /// `UCase`
    UCase,
}

impl Folding {
    /// The function called `name`, in any letter case. The reader passes the
    /// name without its type-declaration character (`Chr$` is `Chr`).
    pub(super) fn named(name: &str) -> Option<Self> {
        [
            ("chr", Self::Chr),
            ("chrw", Self::ChrW),
            ("strreverse", Self::StrReverse),
            ("lcase", Self::LCase),
            ("ucase", Self::UCase),
        ]
        .into_iter()
        .find(|(spelling, _)| name.eq_ignore_ascii_case(spelling))
        .map(|(_, found)| found)
    }

    /// What the function makes of `argument`, if it is one it folds: a whole
    /// number for `Chr` and `ChrW`, a string for the others. A string it makes
    /// out of another is paid for out of `budget`, by the length of the other.
    pub(super) fn apply(self, argument: &Value, budget: &mut Budget) -> Option<String> {
        match (self, argument) {
            (Self::Chr, Value::Number(code)) => character(*code, ASCII_MAX).map(String::from),
            (Self::ChrW, Value::Number(code)) => character(*code, UTF16_MAX).map(String::from),
            (Self::StrReverse, Value::Text(text)) => budget
                .take(text.len())
                .then(|| text.chars().rev().collect()),
            (Self::LCase, Value::Text(text)) => budget
                .take(text.len())
                .then(|| map_case(text, char::to_lowercase)),
            (Self::UCase, Value::Text(text)) => budget
                .take(text.len())
                .then(|| map_case(text, char::to_uppercase)),
            _ => None,
        }
    }
}

/// The value an expression is known to have.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Value {
    /// Anything the closed set does not cover.
    Unknown,
    /// A whole number literal that fits: only `Chr` and `ChrW` read it.
    Number(u32),
    /// A string.
    Text(String),
}

/// How many bytes of string one statement may build out of other strings, so
/// that the text of a statement bounds what folding makes of it: four times
/// its length and [`BUDGET_FLOOR`]. A literal is never held back (it is in the
/// text already), but what a string function makes of a long string, or what
/// a name copies of a constant, is: a statement that nests `UCase` around a
/// literal of megabytes stays unresolved after a few levels instead of
/// copying it at each one, and so does a constant that a statement of a few
/// bytes repeats into megabytes, or that other constants double along a chain
/// of declarations.
#[derive(Debug, Clone, Copy)]
pub(super) struct Budget(usize);

impl Budget {
    /// The budget of a statement whose text is `text`.
    pub(super) fn for_text(text: &str) -> Self {
        Self(text.len().saturating_mul(4).saturating_add(BUDGET_FLOOR))
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
    // `from_str_radix` would take a sign.
    if !digits.iter().all(u8::is_ascii_alphanumeric) {
        return None;
    }
    u32::from_str_radix(std::str::from_utf8(digits).ok()?, radix).ok()
}

/// The character of `code`, if it is not above `max`. A surrogate is none.
fn character(code: u32, max: u32) -> Option<char> {
    if code > max {
        return None;
    }
    char::from_u32(code)
}

/// `text` with each character replaced by `case` of it, when that is a single
/// character: the full Unicode mapping lengthens `ß` to `SS`, and a string
/// that grows would escape the budget. Here `ß` stays.
fn map_case<I: Iterator<Item = char>>(text: &str, case: impl Fn(char) -> I) -> String {
    text.chars()
        .map(|original| {
            let mut mapped = case(original);
            match (mapped.next(), mapped.next()) {
                (Some(only), None) => only,
                _ => original,
            }
        })
        .collect()
}

/// The `Const` strings of a module, by name: what a reference to a name folds
/// to. Only the module-level declarations are read, one by one. A constant
/// whose value is not a known string is kept as a name with no value, and so
/// is one that the module declares twice with different values (the branches
/// of an `#If`): a reference to either folds to nothing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConstTable {
    /// The value by name, in lowercase.
    values: BTreeMap<String, Option<String>>,
    /// How many bytes the names and the values take.
    bytes: usize,
}

impl ConstTable {
    /// Reads `line`, a logical line of the module outside any procedure (the
    /// caller gives these, in order, before it reads the statements that use
    /// the constants), and records the constants it declares. A constant can
    /// refer to those declared before it, in this statement or an earlier
    /// one. Any other line is left alone, and so is a compile directive
    /// (`#Const`), which no reference to a name reads.
    pub fn declare(&mut self, line: &LogicalLine) {
        if line.kind == LogicalKind::Code {
            declare_consts(&line.text, &tokenize(&line.text), self);
        }
    }

    /// The string of the constant called `name`, as `Token::name` spells it
    /// (without brackets or type character) in any letter case, if it has
    /// one.
    pub fn get(&self, name: &str) -> Option<&str> {
        if self.values.is_empty() {
            // The usual case: nothing to compare the name with.
            return None;
        }
        self.values.get(&name.to_lowercase())?.as_deref()
    }

    /// Records the declaration of `name`, spelled as `Token::name` does, with
    /// `value`, which is `None` when it is not a known string. A name that is
    /// declared again keeps its value only if it is the same. Past the cap on
    /// the bytes of the table, a new name is not recorded.
    pub(super) fn insert(&mut self, name: &str, value: Option<String>) {
        let name = name.to_lowercase();
        if let Some(known) = self.values.get_mut(&name) {
            if *known != value {
                let freed = known.as_ref().map_or(0, String::len);
                self.bytes = self.bytes.saturating_sub(freed);
                *known = None;
            }
            return;
        }
        let cost = name
            .len()
            .saturating_add(value.as_ref().map_or(0, String::len));
        if self.bytes.saturating_add(cost) <= MAX_CONST_BYTES {
            self.bytes = self.bytes.saturating_add(cost);
            self.values.insert(name, value);
        }
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
            "+65",
            "&H+41",
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
        assert_eq!(character(65, ASCII_MAX), Some('A'));
        assert_eq!(character(0, ASCII_MAX), Some('\0'));
        assert_eq!(character(127, ASCII_MAX), Some('\u{7f}'));
        assert_eq!(character(128, ASCII_MAX), None);
        assert_eq!(character(8364, UTF16_MAX), Some('\u{20ac}'));
        assert_eq!(character(65_536, UTF16_MAX), None);
        // A surrogate is no character.
        assert_eq!(character(0xd800, UTF16_MAX), None);
        assert_eq!(character(0xdfff, UTF16_MAX), None);
    }

    #[test]
    fn the_case_changes_one_character_at_a_time_and_never_lengthens_a_string() {
        let mut budget = Budget::for_text("");
        let mut apply = |function: Folding, text: &str| {
            function.apply(&Value::Text(text.to_string()), &mut budget)
        };
        assert_eq!(apply(Folding::UCase, "abc ß é"), Some("ABC ß É".into()));
        assert_eq!(apply(Folding::LCase, "ABC İ Σ"), Some("abc İ σ".into()));
        assert_eq!(apply(Folding::StrReverse, "aé€"), Some("€éa".into()));
    }

    #[test]
    fn a_function_of_the_set_is_found_by_name_in_any_letter_case() {
        assert_eq!(Folding::named("CHRW"), Some(Folding::ChrW));
        assert_eq!(Folding::named("strreverse"), Some(Folding::StrReverse));
        // `ChrB` is not in the set, and the reader strips the `$` before it
        // asks.
        for name in ["ChrB", "Chr$", "Asc", "Chrx", ""] {
            assert_eq!(Folding::named(name), None, "{name}");
        }
    }

    #[test]
    fn a_budget_refuses_what_is_left_short_of_and_keeps_what_it_has() {
        let mut budget = Budget::for_text("ab");
        assert!(budget.take(BUDGET_FLOOR + 8));
        assert!(!budget.take(1));
        assert!(Budget::for_text("ab").take(BUDGET_FLOOR + 8));
        assert!(!Budget::for_text("ab").take(BUDGET_FLOOR + 9));
    }

    #[test]
    fn a_name_declared_again_keeps_its_value_only_if_it_is_the_same() {
        let mut table = ConstTable::default();
        for (name, value) in [("A", Some("x")), ("a", Some("x")), ("B", Some("x"))] {
            table.insert(name, value.map(String::from));
        }
        for (name, value) in [("b", Some("y")), ("C", Some("x")), ("C", None), ("D", None)] {
            table.insert(name, value.map(String::from));
        }
        table.insert("d", Some("x".to_string()));
        let found = ["A", "B", "C", "D", "E"].map(|name| table.get(name));
        assert_eq!(found, [Some("x"), None, None, None, None]);
    }

    #[test]
    fn a_table_records_no_new_name_past_its_cap() {
        let mut table = ConstTable::default();
        // The name takes three bytes, so the table is full.
        let size = MAX_CONST_BYTES.saturating_sub(3);
        table.insert("big", Some("x".repeat(size)));
        table.insert("more", Some("y".to_string()));
        assert_eq!(table.get("more"), None);
        assert_eq!(table.get("BIG").map(str::len), Some(size));
        // A name that is recorded can still become ambiguous, and frees its bytes.
        table.insert("big", None);
        table.insert("more", Some("y".to_string()));
        assert_eq!(table.get("more"), Some("y"));
    }
}
