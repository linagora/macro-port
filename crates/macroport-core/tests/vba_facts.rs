// SPDX-License-Identifier: Apache-2.0
//! The members of a statement (SPEC M2 item 3, SPEC section 7). The cases give
//! them as the names in order, apart by spaces, with `Name:=` for a named
//! argument.
#![allow(clippy::expect_used)]
// clippy.toml keeps the clock out of the core, not out of its tests.
#![allow(clippy::disallowed_methods, clippy::disallowed_types)]

use std::time::{Duration, Instant};

use macroport_core::vba::{ConstTable, Facts, StatementReader, split_lines, tokenize};

/// What reading a long statement may take: far above one pass, far below a
/// quadratic one.
const LIMIT: Duration = Duration::from_secs(5);

/// The facts of the statement `text`, with no constant.
fn facts(text: &str) -> Facts {
    StatementReader::new(&ConstTable::default()).read_tokens(text, &tokenize(text))
}

/// The facts of the statement `text`, which must be read within [`LIMIT`].
fn facts_in_time(text: &str) -> Facts {
    let started = Instant::now();
    let read = facts(text);
    let elapsed = started.elapsed();
    assert!(elapsed < LIMIT, "took {elapsed:?}");
    read
}

/// The members of a set of facts, written as the cases do.
fn members_of(facts: &Facts) -> String {
    let names = facts
        .members
        .iter()
        .map(|member| match member.named_argument {
            true => format!("{}:=", member.name),
            false => member.name.clone(),
        });
    names.collect::<Vec<_>>().join(" ")
}

/// Checks that each statement gives the members next to it.
fn check_members(cases: &[(&str, &str)]) {
    for (text, expected) in cases {
        assert_eq!(members_of(&facts(text)), *expected, "{text}");
    }
}

#[test]
fn every_name_of_a_statement_is_a_member_in_source_order() {
    check_members(&[
        (r#"x = Range("A1").Font.Size"#, "x Range Font Size"),
        ("rs!Name = ws.Cells(1, 2).Value", "rs Name ws Cells Value"),
        ("s$ = Left$(t$, 2)", "s Left t"),
        ("[Sheet 1].Select", "Sheet 1 Select"),
        (r#"x = "Font.Bold""#, "x"),
    ]);
}

#[test]
fn a_keyword_or_a_literal_is_no_member() {
    check_members(&[
        ("If a Then b = 1 Else c = 2", "a b c"),
        ("For Each c In rng", "c rng"),
        ("x = True And Not False", "x"),
        ("Set o = Nothing: y = Null", "o y"),
        ("Exit Sub", ""),
    ]);
}

#[test]
fn a_type_name_after_as_or_new_is_a_member() {
    check_members(&[
        ("Dim m As MailItem", "m MailItem"),
        (
            "Dim d As New Scripting.Dictionary",
            "d Scripting Dictionary",
        ),
        ("Set o = New Collection", "o Collection"),
        ("Function F(ByVal a As Long) As Boolean", "F a Long Boolean"),
        ("Dim x As", "x"),
        (r#"Const S As String = "s", T = S"#, "S String T S"),
    ]);
}

#[test]
fn a_name_before_colon_equals_is_a_named_argument() {
    check_members(&[
        (
            r#"r.Replace What:="a", With:=b"#,
            "r Replace What:= With:= b",
        ),
        ("Foo(a:=1, b:=2)", "Foo a:= b:="),
        (".Interior.Pattern = xlSolid", "Interior Pattern xlSolid"),
    ]);
    // The name of an argument is not part of its value.
    assert_eq!(facts("x = Chr(CharCode:=65)").strings, ["A"]);
}

#[test]
fn the_label_of_a_jump_and_an_on_statement_hold_no_member() {
    check_members(&[
        ("GoTo Done", ""),
        ("Resume Retry", ""),
        ("On Error GoTo Handler", ""),
        ("On Error Resume Next", ""),
        ("On n GoTo A, B", ""),
        ("If x Then GoTo Done", "x"),
        // An `On` statement ends at the `Else` of a single-line `If`.
        ("If c Then On Error Resume Next Else y = z", "c y z"),
    ]);
}

#[test]
fn the_name_of_a_constant_is_a_member_and_a_line_that_is_not_code_holds_no_fact() {
    let mut consts = ConstTable::default();
    let lines = split_lines("Const P = \"x\"\n' y = Shell(1)\nDone:\nz = P + Q\n")
        .expect("lines")
        .logical;
    lines.iter().for_each(|line| consts.declare(line));
    let mut reader = StatementReader::new(&consts);
    let read: Vec<_> = lines.iter().map(|line| reader.read(line)).collect();
    let members: Vec<_> = read.iter().map(members_of).collect();
    assert_eq!(members, ["P", "", "", "z P Q"]);
    assert_eq!(read[1..3], [Facts::default(), Facts::default()]);
}

#[test]
fn a_statement_that_stops_short_is_read_as_far_as_it_goes() {
    check_members(&[
        ("x = a.", "x a"),
        ("x = []", "x"),
        ("r.Replace What:=", "r Replace What:="),
        ("f(a, (b", "f a b"),
    ]);
}

#[test]
fn a_very_long_statement_is_read_in_proportion_to_its_size() {
    let chain = facts_in_time(&format!("x = a{}", ".b".repeat(100_000)));
    assert_eq!(chain.members.len(), 100_002);
    let arguments = facts_in_time(&format!("f({})", "a, ".repeat(100_000)));
    assert_eq!(arguments.members.len(), 100_001);
}

#[test]
fn nesting_beyond_the_limit_hides_no_name() {
    // Read flat, without overflowing the stack and with the statement
    // flagged: what lies below the limit is listed all the same.
    let (open, close) = ("(".repeat(100_000), ")".repeat(100_000));
    for text in [
        format!("{}Shell(x){close}", "f(".repeat(100_000)),
        format!("{}Shell(x)", "a.b(".repeat(100_000)),
        format!("y = {}Shell(x)", "Not ".repeat(100_000)),
        format!("If {open}x{close} Then Shell x"),
    ] {
        let read = facts_in_time(&text);
        assert!(read.too_deep);
        assert!(read.members.iter().any(|member| member.name == "Shell"));
        assert!(read.members.iter().any(|member| member.name == "x"));
    }
    let read = facts_in_time(&format!("{open}.Run Path:=1"));
    assert_eq!(members_of(&read), "Run Path:=");
}
