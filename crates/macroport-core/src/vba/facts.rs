// SPDX-License-Identifier: Apache-2.0
//! The facts of a statement (SPEC M2 items 3 and 4): what the rule engine
//! reads of a line of code, in a form that needs no knowledge of VBA.
//!
//! [`StatementReader`] reads one statement at a time and keeps what it needs
//! from one to the next. Nothing in a statement is evaluated, constant folding
//! apart.

use super::expr::Reader;
use super::fold::ConstTable;
use super::lexer::{Token, tokenize};
use super::{LogicalKind, LogicalLine};

/// What a statement holds.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Facts {
    /// The strings of the statement, in source order, with the doubled quotes
    /// of a literal read as one. A literal that belongs to an expression of
    /// the closed set of constant folding (`"WScr" & "ipt.Sh" & "ell"`,
    /// `Chr(110) & "ote"`, `UCase(Prefix) & "x"` with a `Const Prefix`) is
    /// replaced by the value of the whole expression, which is listed once.
    /// The name of a constant on its own adds nothing: its declaration lists
    /// the string. A string literal that is not closed is not listed.
    pub strings: Vec<String>,
}

/// Reads the statements of a module one after the other.
#[derive(Debug)]
pub struct StatementReader<'c> {
    /// The constants of the module, which a name in an expression can stand for.
    consts: &'c ConstTable,
}

impl<'c> StatementReader<'c> {
    /// A reader for the statements of a module whose constants are `consts`.
    pub fn new(consts: &'c ConstTable) -> Self {
        Self { consts }
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
        Facts {
            strings: Reader::new(text, tokens, self.consts).read(),
        }
    }
}
