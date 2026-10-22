// SPDX-License-Identifier: Apache-2.0
//! The calls that the names of the VBA library qualify (SPEC section 7,
//! ADR-0010): `VBA.Shell`, `Interaction.Shell` and `VBA.Interaction.Shell`
//! are the function `Shell` of the library, and the call says so, where
//! `ws.Range` or `Application.SendKeys` is the member of an object. The cases
//! write a call as its name when nothing qualifies it, `.Name` for the member
//! of an object and `VBA.Name` for the function of the library.
#![allow(clippy::expect_used)]

use macroport_core::vba::{
    CallSite, ConstTable, Facts, LogicalKind, MAX_NESTING, StatementReader, split_lines, tokenize,
};

/// The names that qualify a function of the library: `VBA` and the modules of
/// its standard library.
const LIBRARY: [&str; 9] = [
    "VBA",
    "Conversion",
    "DateTime",
    "FileSystem",
    "Financial",
    "Information",
    "Interaction",
    "Math",
    "Strings",
];

/// The facts of the statement `text`, with no constant.
fn facts(text: &str) -> Facts {
    StatementReader::new(&ConstTable::default()).read_tokens(text, &tokenize(text))
}

/// The text and the facts of each statement of `source` that is code, which
/// one reader reads in order.
fn read_block(source: &str) -> Vec<(String, Facts)> {
    let lines = split_lines(source).expect("lines").logical;
    let consts = ConstTable::default();
    let mut reader = StatementReader::new(&consts);
    let code = lines.iter().filter(|line| line.kind == LogicalKind::Code);
    code.map(|line| (line.text.clone(), reader.read(line)))
        .collect()
}

/// A call as the cases write it. The function of the library is after a dot.
fn written(call: &CallSite) -> String {
    assert!(!call.library || call.after_dot, "{call:?}");
    match (call.after_dot, call.library) {
        (false, _) => call.name.clone(),
        (true, false) => format!(".{}", call.name),
        (true, true) => format!("VBA.{}", call.name),
    }
}

/// The calls of a set of facts, written as the cases do, apart by spaces.
fn calls_of(facts: &Facts) -> String {
    let calls: Vec<_> = facts.calls.iter().map(written).collect();
    calls.join(" ")
}

/// The calls named `Shell` in a set of facts, written as the cases do.
fn shells(facts: &Facts) -> Vec<String> {
    let shells = facts.calls.iter().filter(|call| call.name == "Shell");
    shells.map(written).collect()
}

/// Checks that each statement gives the calls next to it.
fn check(cases: &[(&str, &str)]) {
    for (text, expected) in cases {
        assert_eq!(calls_of(&facts(text)), *expected, "{text}");
    }
}

/// The calls of each statement of `source` that is code, but for the `With`
/// and `End With` ones, written as the cases do.
fn block(source: &str) -> Vec<String> {
    let inside = read_block(source).into_iter().filter(|(text, _)| {
        !text.trim_start().starts_with("With") && !text.trim_start().starts_with("End With")
    });
    inside.map(|(_, facts)| calls_of(&facts)).collect()
}

#[test]
fn a_name_qualified_by_the_library_only_is_the_function_of_the_library() {
    // The five spellings of one call: only the first has no qualifier.
    check(&[
        (r#"x = Shell("calc")"#, "Shell"),
        (r#"x = VBA.Shell("calc")"#, "VBA.Shell"),
        (r#"x = VBA.Interaction.Shell("calc")"#, "VBA.Shell"),
        (r#"x = Interaction.Shell("calc")"#, "VBA.Shell"),
        (r#"VBA.Shell "calc""#, "VBA.Shell"),
    ]);
}

#[test]
fn the_library_and_each_of_its_modules_qualify_in_any_case() {
    for name in LIBRARY {
        let upper = name.to_uppercase();
        let lower = name.to_lowercase();
        let bracketed = format!("[{name}]");
        for spelled in [name, upper.as_str(), lower.as_str(), bracketed.as_str()] {
            for text in [
                format!("x = {spelled}.Foo(1)"),
                format!("x = VBA.{spelled}.Foo(1)"),
            ] {
                assert_eq!(calls_of(&facts(&text)), "VBA.Foo", "{text}");
            }
        }
    }
}

#[test]
fn a_function_of_the_library_with_no_parentheses_is_a_call_as_a_name_alone_is() {
    check(&[
        (r#"VBA.Interaction.Shell "calc", 1"#, "VBA.Shell"),
        ("Call VBA.Shell", "VBA.Shell"),
        ("FileSystem.Kill 1", "VBA.Kill"),
        ("x = DateTime.Now", "VBA.Now"),
        // Assigned to, it is a variable, as `Dir = 1` is.
        ("VBA.Shell = 1", ""),
        ("Kill 1", "Kill"),
    ]);
}

#[test]
fn the_member_of_an_object_is_not_the_function_of_the_library() {
    check(&[
        (r#"ws.Range("A1").Select"#, ".Range"),
        (r#"Application.SendKeys("x")"#, ".SendKeys"),
        (r#"Application.SendKeys "x""#, ""),
        (r#"o.Run("x")"#, ".Run"),
        (r#"x = Excel.Application.Run("x")"#, ".Run"),
        // An object before the names, or between them, qualifies the call.
        (r#"x = Foo.VBA.Shell("calc")"#, ".Shell"),
        (r#"x = Foo.Interaction.Shell("calc")"#, ".Shell"),
        (r#"x = VBA.Foo.Shell("calc")"#, ".Shell"),
        ("x = VBA.Err.Raise(5)", ".Raise"),
        ("x = VBA.Constants.vbCrLf", ""),
    ]);
}

#[test]
fn a_bang_or_an_argument_list_makes_a_name_before_a_dot_no_qualifier() {
    check(&[
        (r#"x = VBA!Shell("calc")"#, ".Shell"),
        (r#"x = VBA(1).Shell("calc")"#, "VBA .Shell"),
        (r#"x = Strings(1).Left("a", 1)"#, "Strings .Left"),
    ]);
}

#[test]
fn what_a_call_of_the_library_gives_is_an_object_of_its_own() {
    check(&[
        (r#"x = VBA.Shell("calc")(1)"#, "VBA.Shell"),
        (r#"x = VBA.Shell("calc").Foo("a")"#, "VBA.Shell .Foo"),
        (r#"x = VBA.Shell("calc")(1).Foo("a")"#, "VBA.Shell .Foo"),
        (
            r#"x = VBA.Interaction.CreateObject("A.B").Shell("c")"#,
            "VBA.CreateObject .Shell",
        ),
    ]);
}

#[test]
fn a_with_block_on_the_library_qualifies_its_dots() {
    let cases = [
        (
            "With VBA\n.Shell \"calc\"\nx = .Interaction.Shell(\"c\")\nEnd With\n",
            vec!["VBA.Shell", "VBA.Shell"],
        ),
        (
            "With VBA.Interaction\n.Shell \"calc\"\nEnd With\n",
            vec!["VBA.Shell"],
        ),
        (
            "With VBA\nWith .Interaction\n.Shell \"calc\"\nEnd With\nEnd With\n",
            vec!["VBA.Shell"],
        ),
        // Not the library: another object, an object in the library, a bang,
        // a library name with an argument list.
        ("With Foo\nx = .Shell(\"c\")\nEnd With\n", vec![".Shell"]),
        ("With Strings(1)\nx = .Left(2)\nEnd With\n", vec![".Left"]),
        (
            "With VBA\nx = .Foo.Shell(\"c\")\nx = !Shell(\"c\")\nEnd With\n",
            vec![".Shell", ".Shell"],
        ),
        (
            "With Foo\nWith .Interaction\nx = .Shell(\"c\")\nEnd With\nEnd With\n",
            vec![".Shell"],
        ),
    ];
    for (source, expected) in cases {
        assert_eq!(block(source), expected, "{source}");
    }
}

#[test]
fn the_flag_of_a_with_block_does_not_depend_on_the_copy_of_its_object() {
    // The 200 dots copy the object (35 in weight each) out of the allowance of
    // the statement, and use it up: the last dot gets no object, so that its
    // chain is `Shell` alone and is not listed. The call is the library's all
    // the same if the object is, and is not if it is another.
    let dots = [".a"; 200].join(" + ");
    for (object, expected) in [("VBA", "VBA.Shell"), ("Foo", ".Shell")] {
        let read = read_block(&format!(
            "With {object}\nx = {dots} + .Shell(1)\nEnd With\n"
        ));
        let facts = &read[1].1;
        let chain = |name: &str| vec![object.to_owned(), name.to_owned()];
        assert_eq!(facts.chains.first(), Some(&chain("a")));
        assert!(!facts.chains.contains(&chain("Shell")));
        assert_eq!(shells(facts), [expected], "{object}");
    }
}

#[test]
fn the_wide_list_of_the_calls_does_not_widen_those_of_folding_and_of_classes() {
    // `Math` qualifies a call, yet it holds neither `Chr` nor `CreateObject`.
    assert!(
        facts(r#"Set o = Math.CreateObject("A.B")"#)
            .progids
            .is_empty()
    );
    assert!(facts("x = Math.Chr(65)").strings.is_empty());
}

#[test]
fn the_part_of_a_statement_read_flat_still_reads_the_qualifiers_of_a_call() {
    let (open, close) = ("f(".repeat(MAX_NESTING), ")".repeat(MAX_NESTING));
    for (call, expected) in [
        ("Shell(x)", "Shell"),
        ("VBA.Shell(x)", "VBA.Shell"),
        ("VBA.Interaction.Shell(x)", "VBA.Shell"),
        ("Interaction.Shell x", "VBA.Shell"),
        ("ws.Shell(x)", ".Shell"),
        ("Foo.VBA.Shell(x)", ".Shell"),
        ("VBA.Foo.Shell(x)", ".Shell"),
        ("VBA!Shell(x)", ".Shell"),
        ("a(1).VBA.Shell(x)", ".Shell"),
    ] {
        let read = facts(&format!("{open}{call}{close}"));
        assert!(read.too_deep, "{call}");
        assert_eq!(shells(&read), [expected], "{call}");
    }
}

#[test]
fn the_object_of_a_with_block_is_not_followed_in_the_part_read_flat() {
    let (open, close) = ("f(".repeat(MAX_NESTING), ")".repeat(MAX_NESTING));
    let read = read_block(&format!("With VBA\n{open}.Shell(x){close}\nEnd With\n"));
    assert!(read[1].1.too_deep);
    assert_eq!(shells(&read[1].1), [".Shell"]);
}
