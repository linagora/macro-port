// SPDX-License-Identifier: Apache-2.0
//! What the tests of the structural parser share.

// clippy.toml excuses `expect` in the `#[test]` functions only, not in helpers.
#![allow(clippy::expect_used)]

use macroport_core::report::ProcedureKind;
use macroport_core::vba::{ParsedModule, SourceLines, parse, split_lines};

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
