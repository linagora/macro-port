// SPDX-License-Identifier: Apache-2.0
//! Constant folding (SPEC M2 item 4): the strings of a statement, with the
//! expressions of the closed set replaced by their value, and nothing else
//! evaluated. The cases are statements of one logical line.

// clippy.toml keeps the clock out of the core, not out of its tests.
#![allow(clippy::disallowed_methods, clippy::disallowed_types)]
#![allow(clippy::expect_used)]

use std::time::{Duration, Instant};

use macroport_core::vba::{Facts, MAX_NESTING, StatementReader, split_lines, tokenize};

/// What reading a long statement may take: far above one pass, far below a
/// quadratic one.
const LIMIT: Duration = Duration::from_secs(5);

/// The facts of the statement `text`.
fn facts(text: &str) -> Facts {
    StatementReader::new().read_tokens(text, &tokenize(text))
}

/// The strings of the statement `text`.
fn strings(text: &str) -> Vec<String> {
    facts(text).strings
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
        (r#"x = "a" ' "b""#, &["a"]),
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
        (r#"x = "a" & Trim("b")"#, &["a", "b"]),
        (r#"x = "a" & obj.Name("c") & "b""#, &["a", "c", "b"]),
        (r#"x = "a" & ws.Cells(1, 1) & "b""#, &["a", "b"]),
        (r#"x = "a" & b!c & "d""#, &["a", "d"]),
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
        // Comparisons bind looser than `&`, `Not` looser than a comparison.
        (r#"x = "a" & "b" Like "c" & "d""#, &["ab", "cd"]),
        (r#"x = "a" = "b""#, &["a", "b"]),
        (r#"x = Not "a" & "b""#, &["ab"]),
        // The operators group from the left: nothing joins to what comes
        // after an operand that is not known.
        (
            r#"x = s & "WScr" & "ipt.Sh" & "ell""#,
            &["WScr", "ipt.Sh", "ell"],
        ),
    ]);
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
        ("& + ) ( , ,", &[]),
        ("x = (", &[]),
        ("x = (\"a\"", &["a"]),
        ("x = )\"a\"", &["a"]),
        (r#"f("a", , "b", ,)"#, &["a", "b"]),
        (r#"f(ByVal "a" As Long, "b")"#, &["a", "b"]),
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
fn nesting_beyond_the_limit_reads_the_rest_flat_and_flags_the_statement() {
    let nested = |groups: usize| {
        let (open, close) = ("(".repeat(groups), ")".repeat(groups));
        facts(&format!(r#"{open}"a" & "b"{close} & "c""#))
    };
    // The statement and the right operand of `&` each take a level.
    let fits = nested(MAX_NESTING.saturating_sub(2));
    assert_eq!(
        (fits.strings, fits.too_deep),
        (vec!["abc".to_string()], false)
    );
    let beyond = nested(MAX_NESTING.saturating_sub(1));
    assert_eq!(beyond.strings, ["a", "b", "c"]);
    assert!(beyond.too_deep);
}

#[test]
fn deep_nesting_of_every_shape_keeps_every_string_without_overflowing_the_stack() {
    let deep = |open: &str, close: &str| {
        let depth = 100_000;
        format!(
            r#"x = {}"a"{} & "b""#,
            open.repeat(depth),
            close.repeat(depth)
        )
    };
    let shapes = [
        deep("(", ")"),
        deep("-", ""),
        deep("Not ", ""),
        deep("f(", ")"),
        deep("a.b(", ")"),
    ];
    for (shape, text) in shapes.iter().enumerate() {
        let read = facts(text);
        assert!(read.too_deep, "shape {shape}");
        assert!(read.strings.contains(&"b".to_string()), "shape {shape}");
    }
}

/// The strings of the statement `text`, which must be read within [`LIMIT`].
fn strings_in_time(text: &str) -> Vec<String> {
    let started = Instant::now();
    let read = strings(text);
    let elapsed = started.elapsed();
    assert!(elapsed < LIMIT, "took {elapsed:?}");
    read
}

#[test]
fn a_long_statement_is_read_in_linear_time() {
    // 600,000 tokens, under the cap of the lexer, in two shapes: every join
    // folds, or none does.
    let folded = strings_in_time(&format!("x = {}\"a\"", "\"b\" & ".repeat(300_000)));
    assert_eq!(
        folded.iter().map(String::len).collect::<Vec<_>>(),
        [300_001]
    );
    let apart = strings_in_time(&format!("x = {}\"a\"", "y & \"b\" & ".repeat(100_000)));
    assert_eq!(apart.len(), 100_001);
}
