// SPDX-License-Identifier: Apache-2.0
//! The rules of the line splitter (SPEC M2 item 1), one test each.

// clippy.toml excuses `expect` in the `#[test]` functions only, not in helpers.
#![allow(clippy::expect_used)]

use macroport_core::report::Status;
use macroport_core::vba::{
    LogicalKind, LogicalLine, MAX_LINES, SkipKind, SourceLines, split_lines,
};

fn lines(source: &str) -> SourceLines {
    split_lines(source).expect("a text within the limit")
}

/// The skipped runs as `(first, last, kind)`.
fn skipped(source: &str) -> Vec<(u32, u32, SkipKind)> {
    lines(source)
        .skipped
        .iter()
        .map(|run| (run.first, run.last, run.kind))
        .collect()
}

/// One string per logical line: its physical lines (`3`, or `3-4` when it
/// spans several), a `:` when it follows a separator, `label` or `comment` for
/// those kinds, its text, and ` // comment` with the comment's line when that
/// differs.
fn shape(source: &str) -> Vec<String> {
    lines(source).logical.iter().map(describe).collect()
}

fn describe(line: &LogicalLine) -> String {
    let span = if line.end_line == line.line {
        line.line.to_string()
    } else {
        format!("{}-{}", line.line, line.end_line)
    };
    let separator = if line.after_separator { ":" } else { "" };
    let kind = match line.kind {
        LogicalKind::Code => "",
        LogicalKind::Label => "label ",
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
    format!("{span}{separator} {kind}{}{comment}", line.text)
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
    let error = split_lines(&over).expect_err("a line too many");
    assert_eq!(
        (error.status, error.code),
        (Status::TooLarge, "vba.too_many_lines")
    );
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
    assert_eq!(
        skipped("\u{feff}version 1.0 class\nbegin\nend\nOption Explicit"),
        [(1, 3, SkipKind::ExportHeader)]
    );
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
fn a_continuation_mark_ends_the_line_and_follows_whitespace() {
    assert_eq!(shape("foo_\nbar"), ["1 foo_", "2 bar"]);
    // Alone at the start of a line, or followed by anything, `_` is text.
    assert_eq!(shape("x = 1 + _\n_\ny"), ["1-2 x = 1 + _", "3 y"]);
    assert_eq!(shape("x = 1 + _ \t \ny"), ["1 x = 1 + _", "2 y"]);
}

#[test]
fn a_line_of_only_a_continuation_mark_holds_no_text_and_no_token() {
    assert_eq!(shape(" _\nx = 1"), ["2 x = 1"]);
    // `Rem` is still the first word of its statement.
    assert_eq!(shape(" _\nRem note"), ["2 comment  // note"]);
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

// Separators

#[test]
fn a_colon_splits_statements_on_the_same_line() {
    assert_eq!(
        shape("a = 1: b = 2 :c = 3"),
        ["1 a = 1", "1: b = 2", "1: c = 3"]
    );
}

#[test]
fn a_colon_inside_a_string_a_bracket_or_a_comment_does_not_split() {
    assert_eq!(
        shape("MsgBox \"a:b\": x = 1"),
        ["1 MsgBox \"a:b\"", "1: x = 1"]
    );
    assert_eq!(shape("[a:b].Value = 1"), ["1 [a:b].Value = 1"]);
    assert_eq!(shape("x = 1 ' a: b"), ["1 x = 1 // a: b"]);
}

#[test]
fn a_colon_inside_a_time_literal_does_not_split() {
    assert_eq!(
        shape("t = #12:30:00 PM#: x = 1"),
        ["1 t = #12:30:00 PM#", "1: x = 1"]
    );
}

#[test]
fn a_hash_after_a_name_or_before_a_file_number_is_not_a_date() {
    assert_eq!(shape("x# = 1: y = 2"), ["1 x# = 1", "1: y = 2"]);
    assert_eq!(shape("x#1:2#"), ["1 x#1", "1: 2#"]);
    assert_eq!(
        shape("Print #1, 2: Close #1"),
        ["1 Print #1, 2", "1: Close #1"]
    );
    // A closing parenthesis or bracket ends a name as well.
    assert_eq!(shape("a(1)#1:2#"), ["1 a(1)#1", "1: 2#"]);
    assert_eq!(shape("[a]#1:2#"), ["1 [a]#1", "1: 2#"]);
}

#[test]
fn a_colon_that_begins_a_named_argument_does_not_split() {
    assert_eq!(
        shape("Foo Type:=8, Default:=\"'\""),
        ["1 Foo Type:=8, Default:=\"'\""]
    );
}

#[test]
fn empty_statements_are_dropped() {
    assert_eq!(shape("a = 1::b = 2"), ["1 a = 1", "1: b = 2"]);
    assert_eq!(shape(": a = 1 :"), ["1 a = 1"]);
    assert!(shape(":::").is_empty());
}

#[test]
fn a_statement_split_over_a_continuation_keeps_its_own_lines() {
    assert_eq!(shape("a = 1: b = _\n 2"), ["1 a = 1", "1-2: b = 2"]);
    assert_eq!(shape("a = _\n 1: b = 2"), ["1-2 a = 1", "2: b = 2"]);
}

#[test]
fn a_statement_after_then_and_a_colon_follows_a_separator() {
    assert_eq!(shape("If x Then: y = 1"), ["1 If x Then", "1: y = 1"]);
    assert_eq!(shape("If x Then\n y = 1"), ["1 If x Then", "2 y = 1"]);
}

#[test]
fn a_statement_after_a_separator_knows_the_physical_line_of_each_offset() {
    let split = lines("a = 1: b = _\n 2: c = _\n 3");
    let line = |index: usize| split.logical.get(index).expect("a logical line");
    let b = line(1);
    assert_eq!((b.text.as_str(), b.line, b.end_line), ("b = 2", 1, 2));
    assert_eq!((b.line_at(0), b.line_at(4)), (1, 2));
    let c = line(2);
    assert_eq!((c.text.as_str(), c.line, c.end_line), ("c = 3", 2, 3));
    assert_eq!((c.line_at(0), c.line_at(4)), (2, 3));
}

#[test]
fn a_colon_that_leads_a_chain_has_nothing_to_follow() {
    // A colon at the start of a line does not make a block If single-line.
    assert_eq!(shape("If x Then\n: y = 1"), ["1 If x Then", "2 y = 1"]);
    assert_eq!(shape(":: y = 1: z = 2"), ["1 y = 1", "1: z = 2"]);
    assert_eq!(shape(": Foo: x = 1"), ["1 Foo", "1: x = 1"]);
    // The colon that ends the `If` line does, even over a continuation.
    assert_eq!(shape("If x Then: _\n y = 1"), ["1 If x Then", "2: y = 1"]);
}

#[test]
fn a_text_of_more_statements_than_the_limit_is_refused() {
    let at_limit = "x:".repeat(MAX_LINES as usize);
    assert_eq!(lines(&at_limit).logical.len(), MAX_LINES as usize);
    let code = |text: &str| split_lines(text).err().map(|error| error.code);
    assert_eq!(code(&format!("{at_limit}x")), Some("vba.too_many_lines"));
    // Separators with nothing between them count as well.
    let colons = ":".repeat(MAX_LINES as usize + 1);
    assert_eq!(code(&colons), Some("vba.too_many_lines"));
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
    // A string that never closes holds the rest of its line.
    assert_eq!(shape("x = \"a ' b"), ["1 x = \"a ' b"]);
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
    assert_eq!(shape("x = 1: Rem note"), ["1 x = 1 // note"]);
    assert_eq!(shape("Done: Rem note"), ["1 label Done // note"]);
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
fn a_comment_belongs_to_the_last_statement_of_the_line() {
    assert_eq!(shape("a = 1: b = 2 ' c"), ["1 a = 1", "1: b = 2 // c"]);
    assert_eq!(shape("a = 1: ' c"), ["1 a = 1 // c"]);
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

// Labels

#[test]
fn a_name_and_a_colon_at_the_start_of_a_statement_is_a_label() {
    assert_eq!(shape("ErrorHandler:"), ["1 label ErrorHandler"]);
    assert_eq!(
        shape("    Cleanup:\n    Exit Sub"),
        ["1 label Cleanup", "2 Exit Sub"]
    );
    assert_eq!(shape("Done: ' cleanup"), ["1 label Done // cleanup"]);
}

#[test]
fn a_statement_may_follow_its_label_on_the_same_line() {
    assert_eq!(shape("Done: Exit Sub"), ["1 label Done", "1: Exit Sub"]);
    assert_eq!(shape("Foo: Bar"), ["1 label Foo", "1: Bar"]);
}

#[test]
fn the_colon_of_a_label_follows_its_name() {
    // `Foo :` is a call to Foo, then a separator.
    assert_eq!(shape("Foo : Bar"), ["1 Foo", "1: Bar"]);
}

#[test]
fn a_reserved_word_before_a_colon_is_not_a_label() {
    assert_eq!(shape("Else: x = 1"), ["1 Else", "1: x = 1"]);
    assert_eq!(shape("DoEvents: x = 1"), ["1 DoEvents", "1: x = 1"]);
    assert_eq!(shape("Randomize: x = 1"), ["1 Randomize", "1: x = 1"]);
    assert_eq!(shape("Error: x = 1"), ["1 Error", "1: x = 1"]);
    assert_eq!(shape("time: x = 1"), ["1 time", "1: x = 1"]);
    assert_eq!(shape("  Do: x = 1: Loop"), ["1 Do", "1: x = 1", "1: Loop"]);
}

#[test]
fn only_a_lone_name_before_the_colon_is_a_label() {
    assert_eq!(shape("Case 1: x = 1"), ["1 Case 1", "1: x = 1"]);
    assert_eq!(shape("x = y: z = 1"), ["1 x = y", "1: z = 1"]);
    assert_eq!(shape("Foo"), ["1 Foo"]);
    assert_eq!(shape("Foo ' not a label:"), ["1 Foo // not a label:"]);
    assert_eq!(shape("Foo.Bar: x = 1"), ["1 Foo.Bar", "1: x = 1"]);
}

#[test]
fn a_label_is_only_the_first_statement_of_a_chain() {
    assert_eq!(
        shape("x = 1: Foo: y = 2"),
        ["1 x = 1", "1: Foo", "1: y = 2"]
    );
    assert_eq!(shape("x = a + _\nfoo: bar"), ["1-2 x = a + foo", "2: bar"]);
}

#[test]
fn a_line_number_is_a_label_with_or_without_a_colon() {
    assert_eq!(shape("10 x = 1"), ["1 label 10", "1: x = 1"]);
    assert_eq!(shape("20:\n30"), ["1 label 20", "2 label 30"]);
    assert_eq!(shape("40: y = 2"), ["1 label 40", "1: y = 2"]);
}

#[test]
fn a_comment_may_follow_a_line_number() {
    assert_eq!(shape("10 Rem note"), ["1 label 10 // note"]);
    assert_eq!(shape("  10   rem note"), ["1 label 10 // note"]);
    assert_eq!(shape("10: Rem note"), ["1 label 10 // note"]);
    assert_eq!(shape("10 x = 1 ' note"), ["1 label 10", "1: x = 1 // note"]);
    // The mark of a continuation is no token, and neither is the number.
    assert_eq!(shape("10 _\nRem note"), ["1-2 label 10 // note @2"]);
    assert_eq!(shape(" _\n10 Rem note"), ["2 label 10 // note"]);
    // Only the number that opens a chain is a line number.
    assert_eq!(shape("x = 1: 10 Rem note"), ["1 x = 1", "1: 10 Rem note"]);
    assert_eq!(shape("x = 1 + _\n10 Rem note"), ["1-2 x = 1 + 10 Rem note"]);
    assert_eq!(
        shape("x = 1: _\n10 Rem note"),
        ["1 x = 1", "2: 10 Rem note"]
    );
}

#[test]
fn a_statement_glued_to_a_line_number_stays_a_statement() {
    // Hypothesis, not checked in the VBE: the lexical grammar of MS-VBAL reads
    // `10x` as the number 10 and a name. Reading it that way is the side that
    // shows the statement to the parser.
    assert_eq!(shape("10x = 1"), ["1 label 10", "1: x = 1"]);
    assert_eq!(
        shape("10Shell \"calc\""),
        ["1 label 10", "1: Shell \"calc\""]
    );
    assert_eq!(shape("10Rem note"), ["1 label 10 // note"]);
    assert_eq!(
        shape("10x = 1: y = 2"),
        ["1 label 10", "1: x = 1", "1: y = 2"]
    );
}

#[test]
fn a_number_that_something_else_continues_is_not_a_line_number() {
    let kinds = |text: &str| -> Vec<LogicalKind> {
        lines(text).logical.iter().map(|line| line.kind).collect()
    };
    for text in ["10.5 + x", "10% = 1", "10# + 1", "10& = 1"] {
        assert_eq!(kinds(text), [LogicalKind::Code], "{text:?}");
    }
    for text in ["10", "007", "10 "] {
        assert_eq!(kinds(text), [LogicalKind::Label], "{text:?}");
    }
}

// Corpus header

fn provenance(keys: &str) -> String {
    keys.split(' ').map(|key| format!("' {key}: x\n")).collect()
}

#[test]
fn the_corpus_header_is_skipped_and_still_counted() {
    let keys = provenance("Source Author License Retrieved Family Synthetic");
    let text = format!("{keys}'\nOption Explicit\n");
    assert_eq!(skipped(&text), [(1, 7, SkipKind::CorpusHeader)]);
    assert_eq!(shape(&text), ["8 Option Explicit"]);
    assert_eq!(lines(&text).physical_lines, 8);
}

#[test]
fn the_corpus_header_needs_its_closing_quote_line() {
    let text = format!("{}Option Explicit\n", provenance("Source Author"));
    assert!(skipped(&text).is_empty());
    assert_eq!(
        shape(&text),
        [
            "1 comment  // Source: x",
            "2 comment  // Author: x",
            "3 Option Explicit"
        ]
    );
    assert!(skipped("'\nOption Explicit\n").is_empty());
    // The closing line is a single quote and nothing else.
    for closing in [" '", "' ", "''", "' x"] {
        let text = format!("' Source: x\n{closing}\nOption Explicit\n");
        assert!(skipped(&text).is_empty(), "{closing:?}");
    }
    // A text that ends inside the provenance lines has no header.
    assert!(skipped("' Source: x\n").is_empty());
    assert!(skipped("' Source: x\n' Author: y").is_empty());
}

#[test]
fn the_corpus_header_only_matches_its_own_keys_at_the_top() {
    assert!(skipped("' Source: x\n' Note: y\n'\nOption Explicit\n").is_empty());
    assert!(skipped("' source: x\n'\nOption Explicit\n").is_empty());
    assert!(skipped("' Sourcex: x\n'\nOption Explicit\n").is_empty());
    assert!(skipped("'Source: x\n'\nOption Explicit\n").is_empty());
    let later = format!("Option Explicit\n{}'\n", provenance("Source"));
    assert!(skipped(&later).is_empty());
}

// Export headers

#[test]
fn a_class_export_header_is_skipped() {
    let lf = "VERSION 1.0 CLASS\nBEGIN\n  MultiUse = -1  'True\nEND\n\
              Attribute VB_Name = \"Class1\"\nAttribute VB_Base = \"0{FCFB3D2A}\"\n\
              Option Explicit\n";
    // Excel ends the lines of its exports with CR LF.
    let crlf = lf.replace('\n', "\r\n");
    for text in [lf, crlf.as_str()] {
        assert_eq!(
            skipped(text),
            [(1, 4, SkipKind::ExportHeader), (5, 6, SkipKind::Attribute)]
        );
        assert_eq!(shape(text), ["7 Option Explicit"]);
        let split = lines(text);
        assert_eq!(split.physical_lines, 7);
        let base = split.attribute("vb_base").expect("VB_Base");
        assert_eq!((base.line, base.value.as_str()), (6, "\"0{FCFB3D2A}\""));
    }
}

#[test]
fn a_form_designer_block_is_skipped_with_its_nested_blocks() {
    let text = "VERSION 5.00\n\
                Begin {C62A69F0-16DC-11CE-9E98-00AA00574A4F} UserForm1\n\
                \x20  Caption         =   \"Form\"\n\
                \x20  Begin MSForms.CommandButton Button1\n\
                \x20     Caption         =   \"OK\"\n\
                \x20  End\n\
                End\n\
                Attribute VB_Name = \"UserForm1\"\n\
                Private Sub Button1_Click()\n\
                End Sub\n";
    assert_eq!(
        skipped(text),
        [(1, 7, SkipKind::ExportHeader), (8, 8, SkipKind::Attribute)]
    );
    assert_eq!(shape(text), ["9 Private Sub Button1_Click()", "10 End Sub"]);
}

#[test]
fn a_begin_block_that_never_closes_hides_nothing_but_its_first_line() {
    assert_eq!(
        skipped("BEGIN\nx = 1\ny = 2\n"),
        [(1, 1, SkipKind::ExportHeader)]
    );
    assert_eq!(shape("BEGIN\nx = 1\ny = 2\n"), ["2 x = 1", "3 y = 2"]);
    let nested = "Begin A\nBegin B\nEnd\nx = 1\n";
    assert_eq!(skipped(nested), [(1, 1, SkipKind::ExportHeader)]);
    assert_eq!(shape(nested), ["2 Begin B", "3 End", "4 x = 1"]);
}

#[test]
fn begin_and_version_are_only_headers_before_the_first_code() {
    let text = "Option Explicit\nVERSION 5.00\nBegin A\nEnd\nBegin = 1\n";
    assert!(skipped(text).is_empty());
    assert_eq!(shape(text).len(), 5);
    assert!(skipped("Begin = 1\nEnd\n").is_empty());
    // Blank lines and comments come before the header without ending its zone.
    assert_eq!(
        skipped("' note\n\nVERSION 1.0 CLASS\nBEGIN\nEND\n"),
        [(3, 5, SkipKind::ExportHeader)]
    );
}

#[test]
fn a_version_or_begin_line_with_more_on_it_is_code() {
    for text in [
        "VERSION 5.00: Shell \"x\"",
        "VERSION 5",
        "VERSION 1.0.0",
        "VERSION .5",
        "VERSION 1.0 CLASS 2",
        "VERSION 1.0 FOO",
        "Begin: x = 1\nEnd",
        "Begin(1)\nEnd",
        "Begin Sub Foo()\nEnd",
        "Begin Shell \"calc\"\nEnd",
        "Begin A B C\nEnd",
    ] {
        let split = lines(text);
        assert!(split.skipped.is_empty(), "{text:?}");
        let first = split.logical.first().map(|line| line.line);
        assert_eq!(first, Some(1), "{text:?}");
    }
}

#[test]
fn a_begin_line_opens_a_block_with_a_type_and_a_name_at_most() {
    for text in [
        "BEGIN\nEND",
        "begin\nend",
        "Begin A\nEnd",
        "Begin MSForms.CommandButton Button1\nEnd",
        "Begin {C62A69F0-16DC-11CE-9E98-00AA00574A4F} Formulaire_Employés  'c\nEnd",
    ] {
        assert_eq!(skipped(text), [(1, 2, SkipKind::ExportHeader)], "{text:?}");
    }
}

#[test]
fn only_designer_lines_can_sit_in_a_begin_block() {
    // A statement shows that the block is not one: it never closes, so only
    // its first line is skipped, and the `End` after the statement is code.
    let text = "Begin A\nSub Auto_Open()\nShell \"calc\"\nEnd\nEnd Sub\n";
    assert_eq!(skipped(text), [(1, 1, SkipKind::ExportHeader)]);
    assert_eq!(
        shape(text),
        [
            "2 Sub Auto_Open()",
            "3 Shell \"calc\"",
            "4 End",
            "5 End Sub"
        ]
    );
    // `End Sub` does not close a block, and a property called `Ending` neither.
    assert_eq!(
        skipped("Begin A\nEnd Sub\nEnd\n"),
        [(1, 1, SkipKind::ExportHeader)]
    );
    let properties = "Begin A\nEnding = 1\n\n  Name = \"x\"  'c\nEnd\n";
    assert_eq!(skipped(properties), [(1, 5, SkipKind::ExportHeader)]);
    // The properties of a VB6 designer and comments belong to the block too.
    let vb6 =
        "Begin A\n BeginProperty Font\n  Name = \"Arial\"\n EndProperty\n ' note\n Left = 1\nEnd\n";
    assert_eq!(skipped(vb6), [(1, 7, SkipKind::ExportHeader)]);
}

#[test]
fn the_lines_of_a_begin_block_count_against_the_limit() {
    let block = |inside: u32| format!("Begin A\n{}End\n", "x = 1\n".repeat(inside as usize));
    assert_eq!(lines(&block(MAX_LINES - 2)).physical_lines, MAX_LINES);
    let error = split_lines(&block(MAX_LINES - 1)).expect_err("a line too many");
    assert_eq!(error.code, "vba.too_many_lines");
}

#[test]
fn a_reference_to_a_control_library_is_part_of_the_export_header() {
    let text = "VERSION 5.00\n\
                Object = \"{831FDD16-0C5C-11D2-A9FC-0000F8754DA1}#2.2#0\"; \"MSCOMCTL.OCX\"\n\
                Begin {C62A69F0-16DC-11CE-9E98-00AA00574A4F} F\n\
                \x20  Caption = \"F\"\n\
                End\n\
                Attribute VB_Name = \"F\"\n\
                Option Explicit\n";
    assert_eq!(
        skipped(text),
        [(1, 5, SkipKind::ExportHeader), (6, 6, SkipKind::Attribute)]
    );
    assert_eq!(shape(text), ["7 Option Explicit"]);
}

#[test]
fn an_object_line_is_a_header_only_before_the_first_code_and_with_two_strings() {
    for text in [
        "Option Explicit\nObject = \"x\"; \"y\"\n",
        "Object = \"x\"\n",
        "Object = \"x\"; \"y\"; \"z\"\n",
        "Object = \"x\"; Shell \"y\"\n",
        "Object = \"x\"; \"y\": z = 1\n",
        "Object = \"x\"; \"y\" z\n",
        "Object = 1; 2\n",
        "Objects = \"x\"; \"y\"\n",
    ] {
        assert!(skipped(text).is_empty(), "{text:?}");
    }
}

// Attributes

#[test]
fn attribute_lines_are_skipped_wherever_a_statement_starts() {
    let text = "Attribute VB_Name = \"M\"\nSub A()\n\
                Attribute A.VB_Description = \"d\"\nEnd Sub\n\
                Public X As Long\nAttribute X.VB_VarDescription = \"v\"\n";
    assert_eq!(
        skipped(text),
        [
            (1, 1, SkipKind::Attribute),
            (3, 3, SkipKind::Attribute),
            (6, 6, SkipKind::Attribute)
        ]
    );
    assert_eq!(
        shape(text),
        ["2 Sub A()", "4 End Sub", "5 Public X As Long"]
    );
    let names: Vec<_> = lines(text)
        .attributes
        .iter()
        .map(|attribute| (attribute.line, attribute.name.clone()))
        .collect();
    assert_eq!(
        names,
        [
            (1, "VB_Name".to_owned()),
            (3, "A.VB_Description".to_owned()),
            (6, "X.VB_VarDescription".to_owned())
        ]
    );
}

#[test]
fn an_attribute_name_may_have_several_parts_and_the_value_is_not_interpreted() {
    let split = lines("  attribute Foo.VB_ProcData.VB_Invoke_Func  =  \"T\\n14\"  \n");
    let attribute = split.attributes.first().expect("an attribute");
    assert_eq!(attribute.name, "Foo.VB_ProcData.VB_Invoke_Func");
    assert_eq!(attribute.value, "\"T\\n14\"");
}

#[test]
fn what_is_not_an_attribute_line_stays_code() {
    for text in [
        "Attribute VB_Name",
        "Attribute = 1",
        "Attributes X = 1",
        "AttributeX = 1",
        "' Attribute y = 2",
        "x = 1 + _\nAttribute y = 2",
    ] {
        let split = lines(text);
        assert!(split.attributes.is_empty(), "{text:?}");
        assert_eq!(split.logical.len(), 1, "{text:?}");
    }
}

#[test]
fn the_value_of_an_attribute_is_a_list_of_literals_that_ends_before_a_comment() {
    for (text, value) in [
        ("Attribute VB_Name = \"M\"", "\"M\""),
        ("Attribute VB_Exposed = False", "False"),
        ("Attribute VB_UserMemId = -4", "-4"),
        ("Attribute VB_Flags = &H10", "&H10"),
        (
            "Attribute VB_Ext_KEY = \"Top\" ,  \"Yes\"",
            "\"Top\" ,  \"Yes\"",
        ),
        (
            "Attribute VB_Description = \"say \"\"hi\"\", a:b\"",
            "\"say \"\"hi\"\", a:b\"",
        ),
        ("Attribute X = 1 ' a: b", "1"),
    ] {
        let split = lines(text);
        let attribute = split.attributes.first().expect("an attribute");
        assert_eq!(attribute.value, value, "{text:?}");
    }
}

#[test]
fn a_statement_after_an_attribute_value_is_read_as_code() {
    let text = "Attribute VB_Name = \"M\": Shell \"calc\"";
    assert!(lines(text).attributes.is_empty());
    assert_eq!(
        shape(text),
        ["1 Attribute VB_Name = \"M\"", "1: Shell \"calc\""]
    );
}

#[test]
fn an_attribute_line_that_holds_more_than_literals_stays_code() {
    for text in [
        "Attribute VB_Name =",
        "Attribute VB_Name == \"M\"",
        "Attribute VB_Name = Shell(\"calc\")",
        "Attribute VB_Name = \"M\": Shell \"calc\"",
        "Attribute VB_Name = \"M\" Shell \"calc\"",
        "Attribute VB_Name = \"M\",",
        "Attribute VB_Name = \"M\" _",
        "Attribute VB_Name = \"M",
        "Attribute VB_Name = [M]",
        "Attribute VB_Name = M",
    ] {
        let split = lines(text);
        assert!(split.attributes.is_empty(), "{text:?}");
        assert!(split.skipped.is_empty(), "{text:?}");
        let first = split.logical.first().map(|line| line.line);
        assert_eq!(first, Some(1), "{text:?}");
    }
}

#[test]
fn attribute_finds_the_first_of_a_repeated_name_in_any_letter_case() {
    let split = lines("Attribute VB_Name = \"A\"\nAttribute vb_name = \"B\"\n");
    assert_eq!(split.attributes.len(), 2);
    let found = split.attribute("VB_NAME").map(|a| a.value.as_str());
    assert_eq!(found, Some("\"A\""));
    assert!(split.attribute("VB_Base").is_none());
}

#[test]
fn skipped_runs_of_the_same_kind_merge_when_they_touch() {
    assert_eq!(
        skipped("Attribute A = 1\nAttribute B = 2\n\nAttribute C = 3\n"),
        [(1, 2, SkipKind::Attribute), (4, 4, SkipKind::Attribute)]
    );
}
