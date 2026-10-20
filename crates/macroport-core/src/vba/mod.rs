// SPDX-License-Identifier: Apache-2.0
//! The VBA front end (SPEC M2): the text of a module becomes logical lines,
//! which the lexer cuts into tokens and the parser reads into procedures. It
//! only reads text. Nothing from an analysed file is ever executed or
//! evaluated, constant folding apart.

mod lexer;
mod lines;
mod scan;

use crate::report::Status;
use crate::rules::ModuleType;

pub use lexer::{Keyword, Lexer, Token, TokenKind, tokenize};
pub use lines::{
    Attribute, Comment, LogicalKind, LogicalLine, MAX_LINES, SkipKind, SkippedLines, SourceLines,
    split_lines,
};

/// The source of one module: what the extraction layer, or a text file,
/// hands to the front end.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleSource {
    /// The name of the module.
    pub name: String,
    /// Its type.
    pub module_type: ModuleType,
    /// Its source, as the file presents it.
    pub text: String,
}

/// Why a module could not be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{code}: {message}")]
pub struct VbaError {
    /// The status the file gets in the report.
    pub status: Status,
    /// Stable code, lowercase and dotted as `<layer>.<problem>`, such as
    /// `vba.too_many_lines`.
    pub code: &'static str,
    /// What went wrong.
    pub message: String,
}

impl VbaError {
    /// An error giving the file `status`.
    pub fn new(status: Status, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
        }
    }
}
