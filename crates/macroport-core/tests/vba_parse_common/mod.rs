// SPDX-License-Identifier: Apache-2.0
//! What the tests of the structural parser share.

// clippy.toml excuses `expect` in the `#[test]` functions only, not in helpers.
#![allow(clippy::expect_used)]

use macroport_core::report::ProcedureKind;
use macroport_core::vba::{ParsedModule, Recovery, RecoveryKind, SourceLines, parse, split_lines};

/// The logical lines of `text`.
pub fn lines_of(text: &str) -> SourceLines {
    split_lines(text).expect("a text within the line limit")
}

/// What the parser makes of `text`.
pub fn module_of(text: &str) -> ParsedModule {
    parse(&lines_of(text))
}

/// The name, the kind and the physical span of each procedure.
pub fn spans(module: &ParsedModule) -> Vec<(&str, ProcedureKind, u32, u32)> {
    module
        .procedures
        .iter()
        .map(|p| (p.name.as_str(), p.kind, p.start_line, p.end_line))
        .collect()
}

/// What the parser records for a statement that cannot be read, found on the
/// physical line `line`.
// Each test file compiles its own copy of this module, and some of them never
// look at the statements that cannot be read.
#[allow(dead_code)]
pub fn unreadable(line: u32) -> Recovery {
    Recovery {
        line,
        kind: RecoveryKind::Unparseable,
    }
}
