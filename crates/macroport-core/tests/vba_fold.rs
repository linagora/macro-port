// SPDX-License-Identifier: Apache-2.0
//! Constant folding (SPEC M2 item 4): the strings of a statement, with the
//! expressions of the closed set replaced by their value, and nothing else
//! evaluated. The cases are statements of one logical line.

#![allow(clippy::expect_used)]

use macroport_core::vba::{StatementReader, split_lines, tokenize};

/// The strings of the statement `text`.
fn strings(text: &str) -> Vec<String> {
    StatementReader::new()
        .read_tokens(text, &tokenize(text))
        .strings
}

/// Checks that each statement gives the strings next to it.
fn check(cases: &[(&str, &[&str])]) {
    for (text, expected) in cases {
        assert_eq!(strings(text), *expected, "{text}");
    }
}

#[test]
fn a_string_literal_is_listed_with_its_doubled_quotes_read_as_one() {
    check(&[
        (r#"x = "abc""#, &["abc"]),
        (r#"x = "say ""hi"" now""#, &[r#"say "hi" now"#]),
        (r#"x = """#, &[""]),
        (r#"MsgBox "a", vbOKOnly, "b""#, &["a", "b"]),
        ("x = 1 + 2", &[]),
    ]);
}

#[test]
fn literals_joined_with_and_or_plus_are_folded_into_one_string() {
    check(&[
        (r#"x = "WScr" & "ipt.Sh" & "ell""#, &["WScript.Shell"]),
        (r#"x = "WScr" + "ipt.Sh" + "ell""#, &["WScript.Shell"]),
        (r#"x = "a" & "b" + "c""#, &["abc"]),
        (r#"x = ("a" & "b") & ("c" & "d")"#, &["abcd"]),
        (r#"x = (("a"))"#, &["a"]),
        (r#"CreateObject("a" & "b"), "c""#, &["ab", "c"]),
    ]);
}

#[test]
fn an_expression_outside_the_closed_set_stays_unresolved() {
    // Each of these folds only what the closed set covers, and leaves the
    // other literals apart.
    check(&[
        (r#"x = "a" & y & "b""#, &["a", "b"]),
        (r#"x = "a" & Left("bcd", 2)"#, &["a", "bcd"]),
        (r#"x = Replace("abc", "b", "d")"#, &["abc", "b", "d"]),
        (r#"x = "a" & 1"#, &["a"]),
        (r#"x = "1" + 2"#, &["1"]),
        (r#"x = "a" & vbCrLf & "b""#, &["a", "b"]),
        (r#"x = Mid("abc", 2)"#, &["abc"]),
        (r#"x = String(3, "a")"#, &["a"]),
        (r#"x = "a" & Trim("b")"#, &["a", "b"]),
        (r#"x = "a" & CStr(5) & "b""#, &["a", "b"]),
        (r#"x = "a" & obj.Name & "b""#, &["a", "b"]),
        (r#"x = "a" & obj.Name("c") & "b""#, &["a", "c", "b"]),
        (r#"x = "a" & ws.Cells(1, 1) & "b""#, &["a", "b"]),
        (r#"x = "a" & b!c & "d""#, &["a", "d"]),
        (r#"x = "a" = "b""#, &["a", "b"]),
        (r#"x = "a" & "b" Like "c" & "d""#, &["ab", "cd"]),
        (r#"x = Not "a" & "b""#, &["ab"]),
        (r#"x = -"a" & "b""#, &["a", "b"]),
    ]);
}

#[test]
fn the_operators_group_as_in_vba() {
    check(&[
        // `+` binds tighter than `&`, and `*` tighter than both.
        (r#"x = "a" & "b" + y"#, &["a", "b"]),
        (r#"x = "a" + "b" & y"#, &["ab"]),
        (r#"x = "a" & "b" * 2"#, &["a", "b"]),
        // The operators group from the left: nothing joins to what comes
        // after an operand that is not known.
        (
            r#"x = s & "WScr" & "ipt.Sh" & "ell""#,
            &["WScr", "ipt.Sh", "ell"],
        ),
    ]);
}

#[test]
fn a_line_that_is_not_code_holds_no_string() {
    let mut reader = StatementReader::new();
    let lines = split_lines("' x = \"a\"\nDone:\nx = \"b\"\n")
        .expect("lines")
        .logical;
    let read: Vec<_> = lines.iter().map(|line| reader.read(line).strings).collect();
    assert_eq!(read, [vec![], vec![], vec!["b".to_string()]]);
}

#[test]
fn a_string_that_is_not_closed_is_no_literal() {
    check(&[
        (r#"x = "abc"#, &[]),
        (r#"x = "a" & "bc"#, &["a"]),
        (r#"x = "abc"""#, &[]),
    ]);
}

#[test]
fn what_is_not_a_string_expression_is_left_alone() {
    check(&[
        ("x = #1/2/2000#", &[]),
        ("x = 10 & 20", &[]),
        ("If a = b Then c = d", &[]),
        ("For i = 1 To 10 Step 2", &[]),
        ("", &[]),
        ("'", &[]),
        ("& + ) ( , ,", &[]),
        ("x = (", &[]),
        ("x = (\"a\"", &["a"]),
        ("x = )\"a\"", &["a"]),
        (r#"f("a", , "b", ,)"#, &["a", "b"]),
        (r#"f(ByVal "a" As Long, "b")"#, &["a", "b"]),
    ]);
}

#[test]
fn nesting_beyond_the_depth_limit_gives_up_without_overflowing_the_stack() {
    let deep = |open: &str, close: &str, inner: &str| {
        format!(
            "x = {}{inner}{}",
            open.repeat(100_000),
            close.repeat(100_000)
        )
    };
    for text in [
        deep("(", ")", r#""a""#),
        deep("-", "", "1"),
        deep("Not ", "", "a"),
        deep("f(", ")", r#""a""#),
        deep("a.b(", ")", r#""a""#),
    ] {
        // Only that it returns: the strings that were read before giving up
        // are not specified.
        let _ = strings(&text);
    }
    // A reasonable nesting still folds.
    let ok = format!("x = {}\"a\"{}", "(".repeat(20), ")".repeat(20));
    assert_eq!(strings(&ok), ["a"]);
}

#[test]
fn a_statement_with_many_operands_is_read_without_quadratic_work() {
    // 600,000 tokens, under the cap of the lexer.
    let text = format!("x = {}\"a\"", "\"b\" & ".repeat(300_000));
    let folded = strings(&text);
    assert_eq!(folded.len(), 1);
    assert_eq!(folded.first().map(String::len), Some(300_001));
}
