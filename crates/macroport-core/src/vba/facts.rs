// SPDX-License-Identifier: Apache-2.0
//! The facts of a statement (SPEC M2 items 3 and 4): what the rule engine
//! reads of a line of code, in a form that needs no knowledge of VBA.
//!
//! [`StatementReader`] reads one statement at a time and keeps what it needs
//! from one to the next. Nothing in a statement is evaluated, constant folding
//! apart.

use super::expr::Reader;
use super::lexer::{Token, tokenize};
use super::{LogicalKind, LogicalLine};

/// What a statement holds. The facts are small in proportion to the
/// statement, but a caller that keeps those of many statements bounds the
/// total, as it bounds the lines themselves.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Facts {
    /// The strings of the statement, in source order, with the doubled quotes
    /// of a literal read as one. A literal that belongs to an expression of
    /// the closed set of constant folding (`"WScr" & "ipt.Sh" & "ell"`) is
    /// replaced by the value of the whole expression, which is listed once.
    /// A string literal that is not closed is not listed.
    pub strings: Vec<String>,
    /// The statement nests expressions deeper than [`MAX_NESTING`]. The reader
    /// read the rest of it flat from there: all its strings are listed, as the
    /// source spells them, with none folded. A caller can count the statement
    /// among those that did not parse.
    ///
    /// [`MAX_NESTING`]: super::MAX_NESTING
    pub too_deep: bool,
}

/// Reads the statements of a module one after the other.
#[derive(Debug, Default)]
pub struct StatementReader {}

impl StatementReader {
    /// A reader for the statements of one module.
    pub fn new() -> Self {
        Self::default()
    }

    /// The facts of a logical line. A line that is not code (a label, a
    /// comment) holds none.
    pub fn read(&mut self, line: &LogicalLine) -> Facts {
        if line.kind != LogicalKind::Code {
            return Facts::default();
        }
        self.read_tokens(&line.text, &tokenize(&line.text))
    }

    /// The facts of the statement `tokens`, which were cut from `text`.
    pub fn read_tokens(&mut self, text: &str, tokens: &[Token]) -> Facts {
        Reader::new(text, tokens).read()
    }
}
