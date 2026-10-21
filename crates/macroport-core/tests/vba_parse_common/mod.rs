// SPDX-License-Identifier: Apache-2.0
//! What the tests of the structural parser share.
#![allow(clippy::expect_used)]

use macroport_core::report::ProcedureKind;
use macroport_core::vba::{ParsedModule, SourceLines, parse, split_lines};

/// The logical lines and the parse of `text`.
pub fn parsed(text: &str) -> (SourceLines, ParsedModule) {
    let lines = split_lines(text).expect("a text within the line limit");
    let module = parse(&lines);
    (lines, module)
}

/// The name, the kind and the physical span of each procedure.
pub fn spans(module: &ParsedModule) -> Vec<(&str, ProcedureKind, u32, u32)> {
    module
        .procedures
        .iter()
        .map(|p| (p.name.as_str(), p.kind, p.start_line, p.end_line))
        .collect()
}
