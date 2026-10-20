// SPDX-License-Identifier: Apache-2.0
//! The VBA front end (SPEC M2): the text of a module becomes logical lines,
//! which the lexer cuts into tokens and the parser reads into procedures. It
//! only reads text. Nothing from an analysed file is ever executed or
//! evaluated, constant folding apart.

mod lines;
mod scan;

pub use lines::{
    Attribute, Comment, LogicalKind, LogicalLine, MAX_LINES, SkipKind, SkippedLines, SourceLines,
    TooManyLines, split_lines,
};
