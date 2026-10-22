// SPDX-License-Identifier: Apache-2.0
//! The VBA front end (SPEC M2): the text of a module becomes logical lines,
//! which the lexer cuts into tokens and the parser reads into procedures and
//! declarations. It only reads text. Nothing from an analysed file is ever
//! executed or evaluated, constant folding apart.

mod aggregate;
mod callgraph;
mod dependencies;
mod expr;
mod facts;
mod fold;
mod lexer;
mod lines;
mod metrics;
mod parse;
mod scan;
mod udf;

use crate::report::Status;
use crate::rules::ModuleType;

pub use aggregate::{file_metrics, module_metrics};
pub use callgraph::{CallGraph, ModuleId, ProcedureLinks};
pub(crate) use dependencies::is_external_progid;
pub use expr::MAX_NESTING;
pub use facts::{CallSite, Facts, MAX_WITH_BLOCKS, MemberUse, StatementReader};
pub use fold::ConstTable;
pub use lexer::{Keyword, MAX_TOKENS, Token, TokenKind, tokenize};
pub use lines::{
    Attribute, Comment, LogicalKind, LogicalLine, MAX_LINES, SkipKind, SkippedLines, SourceLines,
    split_lines,
};
pub use metrics::{ProcedureMeasure, measure_procedure};
pub use parse::{
    Declaration, DeclarationKind, Directive, MAX_RECOVERIES, OptionKind, ParsedModule, Procedure,
    Recovery, RecoveryKind, parse,
};
pub use udf::is_udf_candidate;

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
