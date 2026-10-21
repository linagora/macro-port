// SPDX-License-Identifier: Apache-2.0
//! The structural parser on hostile text (CLAUDE.md: every input is hostile).
//! The work must grow with the number of lines and no more: a procedure with
//! no end, or a `Type` with none, must not send the parser looking for one in
//! the lines that follow, header after header. Linear time shows on the clock
//! only, so the limit is set far above what a single pass takes and far below
//! what a second look at the lines read so far, at each line, would take on
//! modules this long.

// clippy.toml keeps the clock out of the core, not out of its tests.
#![allow(clippy::disallowed_methods, clippy::disallowed_types)]
// clippy.toml excuses `expect` in the `#[test]` functions only, not in helpers.
#![allow(clippy::expect_used)]

use std::time::{Duration, Instant};

use macroport_core::vba::{
    MAX_RECOVERIES, MAX_TOKENS, ParsedModule, Recovery, RecoveryKind, parse, split_lines,
};

/// The lines of the hostile modules: far more than a real module has.
const LINES: usize = 50_000;

/// What reading one module may take.
const LIMIT: Duration = Duration::from_secs(5);

/// Reads `text` and fails if the parser takes longer than [`LIMIT`] over it.
fn parse_in_time(text: &str) -> ParsedModule {
    let lines = split_lines(text).expect("a text within the line limit");
    let started = Instant::now();
    let module = parse(&lines);
    let elapsed = started.elapsed();
    let head: String = text.chars().take(40).collect();
    assert!(elapsed < LIMIT, "{head:?} took {elapsed:?}");
    module
}

#[test]
fn a_module_of_one_repeated_line_is_read_in_linear_time() {
    // The text to repeat, the procedures and the declarations read from it, and
    // the recoveries of the module.
    let cases = [
        // No procedure ends: each header closes the one before it.
        ("Sub a()", LINES, 0, 0),
        ("Property Get a()", LINES, 0, 0),
        // A header that names nothing is a header all the same.
        ("Sub", LINES, 0, 0),
        // Nothing opens, so each end is stray, and only the first are kept.
        ("End Sub", 0, 0, MAX_RECOVERIES),
        ("End Property", 0, 0, MAX_RECOVERIES),
        // Not headers and not ends.
        ("x = 1", 0, 0, 0),
        ("' comment", 0, 0, 0),
        ("Label:", 0, 0, 0),
        ("Public Static", 0, 0, 0),
        ("Property", 0, 0, 0),
        ("End", 0, 0, 0),
        ("End Type", 0, 0, 0),
        // A statement that cannot be read: outside any procedure only the
        // first recoveries are kept, and inside one it is the procedure's, so
        // the module has none.
        ("x = (", 0, 0, MAX_RECOVERIES),
        ("Sub a()\nx = (", LINES, 0, 0),
        // Declarations, one for each line. Directives with nothing to match
        // are declarations too, and every `#If` stays open.
        ("Option Explicit", 0, LINES, 0),
        ("Dim a", 0, LINES, 0),
        ("Public Declare Function F Lib \"x\" ()", 0, LINES, 0),
        ("#If X Then", 0, LINES, 0),
        ("#Else", 0, LINES, 0),
        ("#End If", 0, LINES, 0),
        // Four lines, `LINES` times, no `#End If`: the second header of each
        // unit is the other form of the first, and the next unit opens a
        // procedure of its own. Only the first `#If` stands outside one.
        ("#If X Then\nSub a()\n#Else\nSub a()", LINES, 1, 0),
        // No `End` closes the block, and each `Type` or `Enum` ends the one
        // before it: only the first recoveries are kept.
        ("Type T", 0, LINES, MAX_RECOVERIES),
        ("Enum E", 0, LINES, MAX_RECOVERIES),
        // Two forms of the opener in each unit, then the next unit: the `#If`
        // is a member of the block before it, and its `Type` ends that block.
        // The first `#If` stands outside any.
        (
            "#If X Then\nType T\n#Else\nType T",
            0,
            LINES + 1,
            MAX_RECOVERIES,
        ),
        // The header of a procedure ends the block in the same way.
        ("Type T\nSub a()\nEnd Sub", LINES, LINES, MAX_RECOVERIES),
    ];
    for (line, procedures, declarations, recoveries) in cases {
        let module = parse_in_time(&format!("{line}\n").repeat(LINES));
        assert_eq!(module.procedures.len(), procedures, "{line:?}");
        assert_eq!(module.declarations.len(), declarations, "{line:?}");
        assert_eq!(module.recoveries.len(), recoveries, "{line:?}");
    }
}

#[test]
fn the_members_of_a_block_with_no_end_are_read_in_linear_time() {
    // One `Type` or `Enum` takes every line that cannot end it, directives
    // included, whose `#If` blocks stay open.
    for opener in ["Type T", "Enum E"] {
        let members = "x As Long\n#If X Then\n".repeat(LINES / 2);
        let module = parse_in_time(&format!("{opener}\n{members}"));
        assert_eq!(module.declarations.len(), 1, "{opener:?}");
        assert!(module.procedures.is_empty(), "{opener:?}");
        assert_eq!(module.recoveries.len(), 1, "{opener:?}");
    }
}

#[test]
fn a_member_of_the_most_tokens_a_line_can_hold_is_read_in_linear_time() {
    // The subscripts of an array member are read up to the parenthesis that
    // closes them, and none of these lines has one. Each is a `Type` of its
    // own, and ends the one before it.
    for piece in ["(", "(x)", ")", "x,"] {
        let text = format!("Type T\nType({}\nEnd Type\n", piece.repeat(MAX_TOKENS));
        let module = parse_in_time(&text);
        assert_eq!(module.declarations.len(), 2, "{piece:?}");
        // The first `Type` has no end, and the second cannot be read.
        assert_eq!(module.recoveries.len(), 2, "{piece:?}");
    }
}

#[test]
fn a_header_of_the_most_tokens_a_line_can_hold_is_counted_in_linear_time() {
    // What follows the opening parenthesis, and the counts it can give. The
    // tokens past the limit make a last one, which counts as an item.
    let cases = [
        ("x,", 1..=MAX_TOKENS),
        ("(", 1..=1),
        (")", 0..=0),
        (",", 1..=1),
    ];
    for (piece, expected) in cases {
        let module = parse_in_time(&format!("Sub a({}", piece.repeat(MAX_TOKENS)));
        let [procedure] = module.procedures.as_slice() else {
            panic!("{piece:?}: {module:?}");
        };
        assert!(
            expected.contains(&procedure.parameter_count),
            "{piece:?}: {}",
            procedure.parameter_count
        );
    }
}

/// What the parser records for a statement that cannot be read, on the line
/// `line`.
fn unreadable(line: u32) -> Recovery {
    Recovery {
        line,
        kind: RecoveryKind::Unparseable,
    }
}

#[test]
fn a_procedure_of_statements_that_cannot_be_read_keeps_the_first_recoveries() {
    let body = "x = (\n".repeat(LINES);
    let module = parse_in_time(&format!("Sub a()\n{body}End Sub\n"));
    let [procedure] = module.procedures.as_slice() else {
        panic!("{module:?}");
    };
    // The statements start on the line 2, and each is on a line of its own.
    let first: Vec<_> = (2u32..).take(MAX_RECOVERIES).map(unreadable).collect();
    assert_eq!(procedure.recoveries, first);
    assert!(module.recoveries.is_empty());
}

#[test]
fn a_statement_of_the_most_tokens_a_line_can_hold_is_checked_in_linear_time() {
    // What follows `x = `, repeated: each statement fails a check, or is cut
    // by the lexer, and is one recovery of the procedure whatever its length.
    let pieces = ["(", ")", "+ 1 ", "To "];
    for piece in pieces {
        let statement = format!("x = {}", piece.repeat(MAX_TOKENS));
        let module = parse_in_time(&format!("Sub a()\n{statement}\nEnd Sub\n"));
        let [procedure] = module.procedures.as_slice() else {
            panic!("{piece:?}: {module:?}");
        };
        assert_eq!(procedure.recoveries, [unreadable(2)], "{piece:?}");
    }
}
