// SPDX-License-Identifier: Apache-2.0
//! The rules of the line splitter (SPEC M2 item 1), one test each.

// clippy.toml excuses `expect` in the `#[test]` functions only, not in helpers.
#![allow(clippy::expect_used)]

use macroport_core::vba::{
    LogicalKind, LogicalLine, MAX_LINES, SourceLines, TooManyLines, split_lines,
};

fn lines(source: &str) -> SourceLines {
    split_lines(source).expect("a text within the limit")
}

/// One string per logical line: its physical lines (`3`, or `3-4` when it
/// spans several), `comment` for a comment alone, its text, and ` // comment`
/// with the comment's line when that differs.
fn shape(source: &str) -> Vec<String> {
    lines(source).logical.iter().map(describe).collect()
}

fn describe(line: &LogicalLine) -> String {
    let span = if line.end_line == line.line {
        line.line.to_string()
    } else {
        format!("{}-{}", line.line, line.end_line)
    };
    let kind = match line.kind {
        LogicalKind::Code => "",
        LogicalKind::CommentOnly => "comment ",
    };
    let comment = line.comment.as_ref().map_or(String::new(), |comment| {
        let at = if comment.line == line.line {
            String::new()
        } else {
            format!(" @{}", comment.line)
        };
        format!(" // {}{at}", comment.text)
    });
    format!("{span} {kind}{}{comment}", line.text)
}

// Physical lines

#[test]
fn every_line_terminator_ends_a_physical_line() {
    let text = "a\r\nb\rc\nd\u{2028}e\u{2029}f";
    assert_eq!(lines(text).physical_lines, 6);
    assert_eq!(shape(text), ["1 a", "2 b", "3 c", "4 d", "5 e", "6 f"]);
}

#[test]
fn a_final_terminator_opens_no_line() {
    for (text, count) in [
        ("", 0),
        ("\n", 1),
        ("a", 1),
        ("a\n", 1),
        ("a\n\n", 2),
        ("\r\n\r\n", 2),
    ] {
        assert_eq!(lines(text).physical_lines, count, "{text:?}");
    }
}

#[test]
fn a_text_of_more_lines_than_the_limit_is_refused() {
    let at_limit = "a\n".repeat(MAX_LINES as usize);
    assert_eq!(lines(&at_limit).physical_lines, MAX_LINES);
    let over = format!("{at_limit}a");
    assert_eq!(split_lines(&over).err(), Some(TooManyLines));
}

#[test]
fn blank_lines_count_but_hold_nothing_and_the_others_are_trimmed() {
    assert_eq!(
        shape("\n  \t\n x = 1 \t\n\n\u{19}y = 2\n"),
        ["3 x = 1", "5 y = 2"]
    );
}

#[test]
fn a_byte_order_mark_is_not_part_of_the_first_line() {
    assert_eq!(shape("\u{feff}x = 1"), ["1 x = 1"]);
}

// Continuations

#[test]
fn a_continuation_joins_the_next_line_with_one_space() {
    assert_eq!(shape("x = 1 + _\n        2"), ["1-2 x = 1 + 2"]);
    assert_eq!(shape("x = 1 +   _\n2\ny = 3"), ["1-2 x = 1 + 2", "3 y = 3"]);
}

#[test]
fn a_line_knows_the_physical_line_of_each_offset() {
    let split = lines("a = b + _\n    c + _\n\td\nz = 1");
    let line = split.logical.first().expect("a logical line");
    assert_eq!(line.text, "a = b + c + d");
    assert_eq!((line.line, line.end_line, split.logical.len()), (1, 3, 2));
    let at = |needle: &str| line.line_at(line.text.find(needle).expect("in the text"));
    assert_eq!((at("a"), at("b"), at("c"), at("d")), (1, 1, 2, 3));
}

#[test]
fn a_continuation_mark_follows_whitespace_and_may_be_followed_by_some() {
    assert_eq!(shape("foo_\nbar"), ["1 foo_", "2 bar"]);
    assert_eq!(shape("x = 1 + _\n_\n2"), ["1-3 x = 1 + 2"]);
    assert_eq!(shape("x = 1 + _ \t \n2"), ["1-2 x = 1 + 2"]);
}

#[test]
fn an_underscore_inside_an_unclosed_string_is_not_a_continuation() {
    assert_eq!(shape("x = \"a _\ny = 2"), ["1 x = \"a _", "2 y = 2"]);
    assert_eq!(shape("x = \"a _\" & _\n\"b\""), ["1-2 x = \"a _\" & \"b\""]);
}

#[test]
fn a_continuation_with_nothing_to_join_ends_the_statement() {
    assert_eq!(shape("x = 1 + _"), ["1 x = 1 +"]);
    assert_eq!(shape("x = 1 + _\n"), ["1 x = 1 +"]);
    assert_eq!(shape("x = 1 + _\n\ny = 2"), ["1-2 x = 1 +", "3 y = 2"]);
}

// Comments

#[test]
fn a_quote_starts_a_comment_that_is_kept_apart() {
    assert_eq!(shape("x = 1 ' set x"), ["1 x = 1 // set x"]);
    assert_eq!(shape("' note\n'"), ["1 comment  // note", "2 comment  // "]);
}

#[test]
fn a_quote_inside_a_string_or_a_bracketed_name_is_not_a_comment() {
    assert_eq!(
        shape("x = \"it's\" & \"a ' b\" ' real"),
        ["1 x = \"it's\" & \"a ' b\" // real"]
    );
    assert_eq!(
        shape("x = \"say \"\"hi\"\" ' no\" ' yes"),
        ["1 x = \"say \"\"hi\"\" ' no\" // yes"]
    );
    assert_eq!(
        shape("[a'b].Value = 1 ' real"),
        ["1 [a'b].Value = 1 // real"]
    );
}

#[test]
fn rem_at_the_start_of_a_statement_starts_a_comment() {
    assert_eq!(shape("Rem note"), ["1 comment  // note"]);
    assert_eq!(shape("  rem"), ["1 comment  // "]);
    assert_eq!(shape("REM: note"), ["1 comment  // : note"]);
}

#[test]
fn rem_elsewhere_or_as_part_of_a_name_is_code() {
    assert_eq!(shape("Remove = 3"), ["1 Remove = 3"]);
    assert_eq!(shape("Rem_x = 3"), ["1 Rem_x = 3"]);
    assert_eq!(shape("x = 1 + Rem"), ["1 x = 1 + Rem"]);
    assert_eq!(shape("x = a + _\nRem + b"), ["1-2 x = a + Rem + b"]);
}

#[test]
fn a_comment_after_a_continued_statement_keeps_its_own_line() {
    assert_eq!(shape("a = 1 + _\n 2 ' c"), ["1-2 a = 1 + 2 // c @2"]);
}

#[test]
fn a_comment_ending_with_a_continuation_mark_continues_on_the_next_line() {
    assert_eq!(
        shape("' macro _\nrecorded\nx = 1"),
        ["1-2 comment  // macro recorded", "3 x = 1"]
    );
    assert_eq!(
        shape("x = 1 ' a _\n' b _\nc\ny = 2"),
        ["1-3 x = 1 // a ' b c", "4 y = 2"]
    );
}
