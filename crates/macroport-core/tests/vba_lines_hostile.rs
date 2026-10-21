// SPDX-License-Identifier: Apache-2.0
//! Hostile texts for the line splitter. Reading one in time linear in its size
//! shows on the clock only, not in the result: the limit is set far above what
//! a linear reading takes and far below what a quadratic one would take.

// clippy.toml keeps the clock out of the core, not out of its tests.
#![allow(clippy::disallowed_methods, clippy::disallowed_types)]

use std::time::{Duration, Instant};

use macroport_core::vba::split_lines;

/// Splits `text` and fails if that takes longer than `limit`.
fn splits_within(text: &str, limit: Duration) {
    let started = Instant::now();
    let lines = split_lines(text);
    let elapsed = started.elapsed();
    assert!(lines.is_ok(), "{:?}", lines.err());
    assert!(elapsed < limit, "took {elapsed:?}");
}

/// Each `[` looks for a `]` in the rest of its line: without the rule that a
/// search that fails is not made again, 2 MB of them take about a minute.
#[test]
fn a_line_of_opening_brackets_is_read_in_linear_time() {
    splits_within(&"[".repeat(2 << 20), Duration::from_secs(5));
}

/// Each `#` looks for its closing `#` no further than a date literal can
/// reach, each `:` cuts a statement, and only the first statement of a chain is
/// tested for a label: none of them reads the rest of a line again.
#[test]
fn lines_of_hashes_separators_and_names_are_read_in_linear_time() {
    let limit = Duration::from_secs(5);
    let never_closed = format!("#{}", "a".repeat(47)).repeat(40_000);
    let one_label = format!("{}:", "a".repeat(2 << 20));
    let one_number = format!("{} x", "1".repeat(2 << 20));
    for text in [
        "#".repeat(2 << 20),
        never_closed,
        "#1:2# ".repeat(100_000),
        "x:".repeat(100_000),
        one_label,
        one_number,
        " _\n".repeat(200_000),
    ] {
        splits_within(&text, limit);
    }
}
