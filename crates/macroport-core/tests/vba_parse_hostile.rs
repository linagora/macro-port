// SPDX-License-Identifier: Apache-2.0
//! The structural parser on hostile text (CLAUDE.md: every input is hostile).
//! The work must grow with the number of lines and no more: a procedure with
//! no end must not send the parser looking for one in the lines that follow,
//! header after header. Linear time shows on the clock only, so the limit is
//! set far above what a single pass takes and far below what a second look at
//! the lines read so far, at each line, would take on modules this long.

// clippy.toml keeps the clock out of the core, not out of its tests.
#![allow(clippy::disallowed_methods, clippy::disallowed_types)]
// clippy.toml excuses `expect` in the `#[test]` functions only, not in helpers.
#![allow(clippy::expect_used)]

use std::time::{Duration, Instant};

use macroport_core::vba::{MAX_RECOVERIES, MAX_TOKENS, ParsedModule, parse, split_lines};

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
    // The text to repeat, the procedures read from it and the recoveries of the
    // module.
    let cases = [
        // No procedure ends: each header closes the one before it.
        ("Sub a()", LINES, 0),
        ("Property Get a()", LINES, 0),
        // A header that names nothing is a header all the same.
        ("Sub", LINES, 0),
        // Nothing opens, so each end is stray, and only the first are kept.
        ("End Sub", 0, MAX_RECOVERIES),
        ("End Property", 0, MAX_RECOVERIES),
        // Not headers and not ends.
        ("x = 1", 0, 0),
        ("' comment", 0, 0),
        ("Label:", 0, 0),
        ("Public Static", 0, 0),
        ("Property", 0, 0),
        ("End", 0, 0),
        // Directives with nothing to match: every `#If` stays open.
        ("#If X Then", 0, 0),
        ("#Else", 0, 0),
        ("#End If", 0, 0),
        // Four lines, `LINES` times, no `#End If`: the second header of each
        // unit is the other form of the first, and the next unit opens a
        // procedure of its own.
        ("#If X Then\nSub a()\n#Else\nSub a()", LINES, 0),
    ];
    for (line, procedures, recoveries) in cases {
        let module = parse_in_time(&format!("{line}\n").repeat(LINES));
        assert_eq!(module.procedures.len(), procedures, "{line:?}");
        assert_eq!(module.recoveries.len(), recoveries, "{line:?}");
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
