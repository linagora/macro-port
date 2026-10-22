// SPDX-License-Identifier: Apache-2.0
//! Tests of the `regex`, `string` and `comment` matchers, which read the text
//! of the lines. The regexes of these tests are case-sensitive unless they say
//! `(?i)`: the rules loader makes the real ones case-insensitive.

use crate::engine::testing::{
    comments, line, lines_hit, lines_hit_with_text, module, places, procedure, project, project_of,
    regexes, snippets, strings,
};
use crate::engine::{CodeLine, Evaluator, ProjectInput};
use crate::rules::{Matcher, ModuleType, TextMode};

/// What `matcher` quotes in a procedure whose body is `body`.
fn quoted(body: &[CodeLine], matcher: &Matcher) -> Vec<String> {
    let project = project_of(body);
    snippets(&Evaluator::new(&project).procedure_hits(0, 0, matcher, TextMode::Code))
}

#[test]
fn regex_reads_the_code_without_strings_and_comments_unless_raw() {
    let body = [
        line(2, r#"Open "C:\out.txt" For Output As #1"#),
        line(3, r#"x = "Open a For Output""#),
        line(4, "y = 1 ' Open it For Output"),
    ];
    let matcher = regexes(&[r"\bOpen\s+.+\s+For\s+Output\b"]);
    assert_eq!(lines_hit(&body, &matcher), [2]);
    let raw = lines_hit_with_text(&body, &matcher, TextMode::Raw);
    assert_eq!(raw, [2, 3, 4]);
}

#[test]
fn regex_quotes_the_statement_and_not_the_code_it_matched() {
    let body = [line(2, r#"Open "C:\out.txt" For Output As #1"#)];
    let matcher = regexes(&[r"\bOpen\s+.+\s+For\s+Output\b"]);
    assert_eq!(
        quoted(&body, &matcher),
        [r#"Open "C:\out.txt" For Output As #1"#]
    );
}

#[test]
fn regex_gives_one_hit_per_line_however_often_it_matches() {
    let body = [line(2, "Call Foo.Show(Foo.Show)"), line(3, "Bar.Hide")];
    let matcher = regexes(&[r"\bShow\b", r"\bFoo\b"]);
    assert_eq!(lines_hit(&body, &matcher), [2]);
}

#[test]
fn regex_matches_any_of_its_patterns() {
    let body = [line(2, "Foo.Show"), line(3, "Bar.Hide"), line(4, "x = 1")];
    let matcher = regexes(&[r"\bShow\b", r"\bHide\b"]);
    assert_eq!(lines_hit(&body, &matcher), [2, 3]);
}

/// A module whose declarations hold a `Declare`, a credential and a comment,
/// and whose one procedure holds none of them.
fn project_with_declarations() -> ProjectInput {
    let declare = r#"Private Declare PtrSafe Function Sleep Lib "kernel32" (ByVal ms As Long)"#;
    let main = procedure("Main", 6, vec![line(7, "Sleep 100")]);
    project(vec![
        module("Module1", ModuleType::Standard, vec![main]).with_declarations(vec![
            line(2, declare),
            line(3, r#"Const Key = "password=abc123""#),
            line(4, "' Recorded macro"),
        ]),
    ])
}

#[test]
fn the_text_matchers_read_the_declarations_of_a_module_and_a_procedure_does_not() {
    let project = project_with_declarations();
    let evaluator = Evaluator::new(&project);
    let text = TextMode::Code;
    // Each matcher finds one of the declarations.
    let declaration = r"^\s*(Public\s+|Private\s+)?Declare\s+(PtrSafe\s+)?(Function|Sub)\b";
    let credential = r#"(password|pwd)\s*=\s*[^;"\s]{3,}"#;
    let found = [
        (regexes(&[declaration]), 2),
        (strings(&[credential]), 3),
        (comments(&["(?i)recorded"]), 4),
    ];
    for (matcher, number) in found {
        let hits = evaluator.module_hits(0, &matcher, text);
        assert_eq!(places(&hits), [(0, number)]);
        let hits = evaluator.project_hits(&matcher, text);
        assert_eq!(places(&hits), [(0, number)]);
        assert!(evaluator.procedure_hits(0, 0, &matcher, text).is_empty());
    }
}

#[test]
fn regex_reads_the_text_key_at_every_scope() {
    let main = procedure("Main", 6, vec![line(7, r#"Log "password=none""#)]);
    let declarations = vec![line(3, r#"Const Key = "password=abc123""#)];
    let project = project(vec![
        module("Module1", ModuleType::Standard, vec![main]).with_declarations(declarations),
    ]);
    let evaluator = Evaluator::new(&project);
    // The quote is in the raw text only: the code has `""` for the literal.
    let matcher = regexes(&[r#""password"#]);
    let raw = TextMode::Raw;
    let hits = evaluator.procedure_hits(0, 0, &matcher, raw);
    assert_eq!(places(&hits), [(0, 7)]);
    let hits = evaluator.module_hits(0, &matcher, raw);
    assert_eq!(places(&hits), [(0, 3), (0, 7)]);
    let hits = evaluator.project_hits(&matcher, raw);
    assert_eq!(places(&hits), [(0, 3), (0, 7)]);
    let code = TextMode::Code;
    assert!(evaluator.procedure_hits(0, 0, &matcher, code).is_empty());
    assert!(evaluator.module_hits(0, &matcher, code).is_empty());
    assert!(evaluator.project_hits(&matcher, code).is_empty());
}

#[test]
fn string_reads_the_contents_of_each_literal() {
    let body = [
        line(2, r#"msg = "select 1""#),
        line(3, r#"msg = "Not a select""#),
        line(4, "Select = 1"),
        line(5, r#"Log "select 1", "select 2""#),
    ];
    // Code that is not in a literal is not read, and a literal is one hit.
    let matcher = strings(&["(?i)^select"]);
    assert_eq!(lines_hit(&body, &matcher), [2, 5, 5]);
}

#[test]
fn string_quotes_the_statement_and_not_the_literal() {
    let body = [line(2, r#"Log "select 1", "select 2""#)];
    let matcher = strings(&["^select"]);
    let statement = r#"Log "select 1", "select 2""#;
    assert_eq!(quoted(&body, &matcher), [statement, statement]);
}

#[test]
fn string_reads_the_folded_value() {
    let sql = r#"sql = "SELECT * FROM t" & " WHERE a = 1""#;
    let matcher = strings(&[r"(?i)^select\b.*\bwhere\b"]);
    let folded = [line(2, sql).with_strings(&["SELECT * FROM t WHERE a = 1"])];
    assert_eq!(lines_hit(&folded, &matcher), [2]);
    // The pieces alone do not match.
    assert!(lines_hit(&[line(2, sql)], &matcher).is_empty());
}

#[test]
fn string_matches_any_of_its_patterns() {
    let body = [
        line(2, r#"a = "http://x""#),
        line(3, r#"b = "ftp://y""#),
        line(4, r#"c = "file""#),
    ];
    // Each pattern matches a different literal.
    assert_eq!(lines_hit(&body, &strings(&["^http", "^ftp"])), [2, 3]);
    // A literal that both patterns match is one hit.
    assert_eq!(lines_hit(&body, &strings(&["^h", "tp"])), [2, 3]);
}

#[test]
fn comment_reads_the_text_after_the_marker() {
    let body = [
        line(2, "' Macro recorded 12/05/2019 by Jo"),
        line(3, "x = 1 ' keyboard shortcut: Ctrl+q"),
        line(4, "y = 2"),
        line(5, "' unrelated"),
        line(6, "Rem Macro recorded by Jo"),
    ];
    // Anchored, so that the marker must not be part of the text; each pattern
    // matches different comments.
    let matcher = comments(&[r"(?i)^\s*macro recorded", r"(?i)^\s*keyboard shortcut"]);
    assert_eq!(lines_hit(&body, &matcher), [2, 3, 6]);
    // A hit quotes the whole statement, comment included.
    assert_eq!(
        quoted(&body, &matcher),
        [
            "' Macro recorded 12/05/2019 by Jo",
            "x = 1 ' keyboard shortcut: Ctrl+q",
            "Rem Macro recorded by Jo"
        ]
    );
}

#[test]
fn comment_never_reads_the_code_or_the_literals() {
    let body = [
        line(2, r#"z = "macro recorded""#),
        line(3, r#"w = "it's a macro recorded""#),
        line(4, "x = 1 ' macro recorded"),
    ];
    assert_eq!(lines_hit(&body, &comments(&["macro recorded"])), [4]);
}
