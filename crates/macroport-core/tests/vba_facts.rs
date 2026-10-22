// SPDX-License-Identifier: Apache-2.0
//! The members, the calls and the chains of a statement (SPEC M2 item 3, SPEC
//! section 7). The cases give them as the names in order, apart by spaces:
//! `Name:=` for a named argument, `.Name` for a call after a dot, and the
//! elements of a chain joined by dots.
#![allow(clippy::expect_used)]
// clippy.toml keeps the clock out of the core, not out of its tests.
#![allow(clippy::disallowed_methods, clippy::disallowed_types)]

use std::time::{Duration, Instant};

use macroport_core::vba::{
    ConstTable, Facts, LogicalKind, MAX_WITH_BLOCKS, StatementReader, split_lines, tokenize,
};

/// What reading a long statement may take: far above one pass, far below a
/// quadratic one.
const LIMIT: Duration = Duration::from_secs(5);

/// The facts of the statement `text`, with no constant.
fn facts(text: &str) -> Facts {
    StatementReader::new(&ConstTable::default()).read_tokens(text, &tokenize(text))
}

/// What `work` gives, which it must give within [`LIMIT`].
fn in_time<T>(work: impl FnOnce() -> T) -> T {
    let started = Instant::now();
    let result = work();
    let elapsed = started.elapsed();
    assert!(elapsed < LIMIT, "took {elapsed:?}");
    result
}

/// The facts of the statement `text`, which must be read within [`LIMIT`].
fn facts_in_time(text: &str) -> Facts {
    in_time(|| facts(text))
}

/// The facts of each statement of `source` that is code, which one reader
/// reads in order.
fn block(source: &str) -> Vec<Facts> {
    let lines = split_lines(source).expect("lines").logical;
    let consts = ConstTable::default();
    let mut reader = StatementReader::new(&consts);
    let code = lines.iter().filter(|line| line.kind == LogicalKind::Code);
    code.map(|line| reader.read(line)).collect()
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

/// The calls of a set of facts, written as the cases do.
fn calls_of(facts: &Facts) -> String {
    let names = facts.calls.iter().map(|call| match call.after_dot {
        true => format!(".{}", call.name),
        false => call.name.clone(),
    });
    names.collect::<Vec<_>>().join(" ")
}

/// Checks that each statement gives the members next to it.
fn check_members(cases: &[(&str, &str)]) {
    for (text, expected) in cases {
        assert_eq!(members_of(&facts(text)), *expected, "{text}");
    }
}

/// Checks that each statement gives the calls next to it.
fn check_calls(cases: &[(&str, &str)]) {
    for (text, expected) in cases {
        assert_eq!(calls_of(&facts(text)), *expected, "{text}");
    }
}

/// The chains of a set of facts, written as the cases do.
fn chains_of(facts: &Facts) -> String {
    let chains = facts.chains.iter().map(|chain| chain.join("."));
    chains.collect::<Vec<_>>().join(" ")
}

/// Checks that each statement gives the chains next to it.
fn check_chains(cases: &[(&str, &str)]) {
    for (text, expected) in cases {
        assert_eq!(chains_of(&facts(text)), *expected, "{text}");
    }
}

/// The chains of each statement of `source` that is code, written as the
/// cases do.
fn block_chains(source: &str) -> Vec<String> {
    block(source).iter().map(chains_of).collect()
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
fn a_name_that_is_not_assigned_to_nor_declared_is_a_call() {
    check_calls(&[
        (r#"x = Shell("calc")"#, "Shell"),
        ("Kill path", "Kill path"),
        ("Call Helper(1, 2)", "Helper"),
        ("p = CurDir$", "CurDir"),
        ("x = Left$(s, 2)", "Left s"),
        (r#"Mid(s, 1, 2) = "ab""#, "Mid s"),
        // The bounds of a `ReDim` are expressions, not constants.
        (
            r#"ReDim Preserve a(Len(s)), b(1 To Shell("x"))"#,
            "a Len s b Shell",
        ),
    ]);
}

#[test]
fn a_name_that_is_assigned_to_or_followed_by_a_dot_is_no_call() {
    check_calls(&[
        ("Dir = 1", ""),
        (r#"If Dir = "" Then"#, ""),
        ("x = Kill.Count", ""),
        ("For i = 1 To 3", ""),
        ("For Each c In rng", "rng"),
        ("Set o = New Collection", ""),
        ("x = ws.Name", ""),
        ("If ws.Visible Then x = 1", ""),
    ]);
}

#[test]
fn a_declared_name_is_no_call() {
    check_calls(&[
        ("Dim Kill As Long", ""),
        (r#"Private Declare Function Shell Lib "k" () As Long"#, ""),
        ("Function Trim(s As String) As String", ""),
        ("Kill As Long", ""),
        ("Implements Shell", ""),
        ("Option Explicit", ""),
    ]);
}

#[test]
fn a_call_after_a_dot_is_flagged() {
    check_calls(&[
        (r#"ws.Range("A1").Select"#, ".Range"),
        ("x = ws.Cells(1, 1).Value", ".Cells"),
        ("With ws.Item(2)", ".Item"),
        (r#"Range("A1").Font.Bold = True"#, "Range"),
        (r#"Application.Run "x""#, ""),
    ]);
}

#[test]
fn the_name_of_a_constant_is_no_call_and_a_line_that_is_not_code_holds_no_fact() {
    let mut consts = ConstTable::default();
    let lines = split_lines("Const P = \"x\"\n' y = Shell(1)\nDone:\nz = P + Q\n")
        .expect("lines")
        .logical;
    lines.iter().for_each(|line| consts.declare(line));
    let mut reader = StatementReader::new(&consts);
    let read: Vec<_> = lines.iter().map(|line| reader.read(line)).collect();
    let calls: Vec<_> = read.iter().map(calls_of).collect();
    assert_eq!(calls, ["", "", "", "Q"]);
    assert_eq!(members_of(&read[3]), "z P Q");
    assert_eq!(read[1..3], [Facts::default(), Facts::default()]);
}

#[test]
fn a_chain_lists_its_elements_without_the_arguments_of_the_calls_in_it() {
    check_chains(&[
        (r#"Workbooks.Open "a.xls""#, "Workbooks.Open"),
        (
            r#"ThisWorkbook.Worksheets("X").Range("A1").Interior.Color = 1"#,
            "ThisWorkbook.Worksheets.Range.Interior.Color",
        ),
        ("x = ws.End(xlUp).Row", "ws.End.Row"),
        ("rs!Name.Value = 1", "rs.Name.Value"),
        ("Call Application.Run(m)", "Application.Run"),
        ("If x.y Then z.w = 1", "x.y z.w"),
    ]);
}

#[test]
fn a_chain_in_an_argument_is_listed_apart_in_the_order_of_its_first_token() {
    check_chains(&[
        (
            "x = Worksheets(1).Range(a.b).Interior",
            "Worksheets.Range.Interior a.b",
        ),
        ("x = Foo(a.b, c.d).e", "Foo.e a.b c.d"),
        (r#"r.Replace What:="a", With:=x.y"#, "r.Replace x.y"),
    ]);
}

#[test]
fn a_name_between_brackets_is_one_element_of_a_chain() {
    assert_eq!(facts("[Sheet 1].Select").chains, [["Sheet 1", "Select"]]);
    assert_eq!(facts("[a.b].c = 1").chains, [["a.b", "c"]]);
}

#[test]
fn a_name_alone_or_a_type_is_no_chain() {
    check_chains(&[
        ("x = y", ""),
        ("Foo(1)", ""),
        ("Call Helper(1, 2)", ""),
        ("x = a.", ""),
        ("x = .", ""),
        ("Set d = New Scripting.Dictionary", ""),
        ("Dim d As Scripting.Dictionary", ""),
    ]);
}

#[test]
fn the_index_lists_of_a_call_do_not_end_its_chain() {
    check_chains(&[
        (r#"Range("A1:C3")(2, 2).Value = 1"#, "Range.Value"),
        ("x = Cells(1)(2)(3).Value", "Cells.Value"),
    ]);
}

#[test]
fn a_dot_after_a_group_starts_a_chain_of_its_own() {
    // The reader does not know what the group is: the chain is what follows.
    check_chains(&[("x = (a).b.c", "b.c"), ("x = (a + b).c", "")]);
}

#[test]
fn a_chain_that_starts_with_a_dot_outside_a_block_has_the_names_after_the_dot_only() {
    check_chains(&[
        (".Font.Bold = True", "Font.Bold"),
        ("x = .Text", ""),
        ("x = !Id.Value", "Id.Value"),
    ]);
}

#[test]
fn a_dot_after_a_space_starts_an_operand_of_its_own() {
    check_chains(&[
        // The argument of `Add` is a member of the object of a `With` block,
        // not a member of `Add`.
        ("list.Add .Name", "list.Add"),
        ("Set c = list.Item(1) .Value", "list.Item"),
        ("x = a.b . c", "a.b"),
        ("Kill .path", ""),
        // The chain that the detached dot starts goes on with the dots that touch.
        ("x = a.b .c.d", "a.b c.d"),
    ]);
    // Neither the members nor the calls tie the name to the one before.
    let read = facts("list.Add .Name");
    assert_eq!(members_of(&read), "list Add Name");
    assert_eq!(calls_of(&read), "");
    assert_eq!(calls_of(&facts("Erase .arr")), "Erase");
}

#[test]
fn a_dot_that_opens_a_continued_line_continues_the_chain_before_it() {
    let source = "x.Interior _\n    .Color = 1\n";
    assert_eq!(block_chains(source), ["x.Interior.Color"]);
    let source = "Set r = ws.Range(\"A1\") _\n    .Offset(1) _\n    .Value\n";
    assert_eq!(block_chains(source), ["ws.Range.Offset.Value"]);
    // Only the dot that opens the line does; a dot after a space does not.
    let source = "x.Interior _\n    .Color = c .Value\n";
    assert_eq!(block_chains(source), ["x.Interior.Color"]);
    // Without the continuation mark the line is a statement of its own.
    assert_eq!(block_chains("x.Interior\n.Color = 1\n"), ["x.Interior", ""]);
}

#[test]
fn a_dot_inside_a_with_block_continues_its_object() {
    let source = r#"
With Range("A1")
    .Font
    .Font.Bold = True
    x = .Text
End With
y = .Font
"#;
    assert_eq!(
        block_chains(source),
        ["", "Range.Font", "Range.Font.Bold", "Range.Text", "", ""],
    );
    let source = r#"
With ThisWorkbook.Worksheets(1).Range("A1")
    .Interior.Color = 1
End With
"#;
    assert_eq!(
        block_chains(source),
        [
            "ThisWorkbook.Worksheets.Range",
            "ThisWorkbook.Worksheets.Range.Interior.Color",
            ""
        ],
    );
    // `!` continues the object like `.`.
    let source = "With rs\n!Name = 1\nx = !Id\nEnd With\n";
    assert_eq!(block_chains(source), ["", "rs.Name", "rs.Id", ""]);
}

#[test]
fn a_dot_after_a_space_in_a_block_starts_a_chain_on_the_object() {
    let source = "With rng\nlist.Add .Name\nx.Interior _\n    .Color = .Value\nEnd With\n";
    assert_eq!(
        block_chains(source),
        ["", "list.Add rng.Name", "x.Interior.Color rng.Value", ""],
    );
}

#[test]
fn a_dot_after_a_group_in_a_block_gets_no_object() {
    // The dot continues what the parenthesis closed, not the object of the block.
    let source = "With a\nx = (b).c.d\nEnd With\n";
    assert_eq!(block_chains(source), ["", "c.d", ""]);
}

#[test]
fn a_dot_that_names_nothing_lists_no_chain() {
    // The object of the block is a chain, which its `With` lists.
    let source = "With a.b\nx = .\ny = !\nEnd With\n";
    assert_eq!(block_chains(source), ["a.b", "", "", ""]);
}

#[test]
fn with_blocks_nest_and_each_end_with_closes_one() {
    let source = r#"
With Worksheets(1)
    With .Range("A1")
        .Interior.Color = 1
    End With
    .Name = "x"
End With
.Name = "y"
"#;
    assert_eq!(
        block_chains(source),
        [
            "",
            "Worksheets.Range",
            "Worksheets.Range.Interior.Color",
            "",
            "Worksheets.Name",
            "",
            ""
        ],
    );
}

#[test]
fn an_object_that_is_no_chain_of_names_gives_no_prefix() {
    for opener in ["With New Collection", "With (a)", "With a + b", "With"] {
        let source = format!("{opener}\n.Font.Bold = True\n.Add x\nEnd With\n.Font.Bold = False\n");
        assert_eq!(
            block_chains(&source)[1..],
            ["Font.Bold", "", "", "Font.Bold"],
            "{opener}"
        );
    }
    // A comment after the object does not make it something else.
    assert_eq!(block_chains("With a.b ' c\n.d.e = 1\n")[1], "a.b.d.e");
}

#[test]
fn an_object_that_weighs_more_than_a_block_may_keep_gives_no_prefix() {
    // A name weighs its bytes and 32, and an object 4,096 at most: one name
    // of 4,064 bytes or 97 of ten.
    let name = "aaaaaaaaaa";
    let kept = ["a".repeat(4_064), [name; 97].join(".")];
    let dropped = ["a".repeat(4_065), [name; 98].join(".")];
    for object in kept {
        let read = block_chains(&format!("With {object}\n.x.y = 1\n"));
        assert_eq!(read[1], format!("{object}.x.y"));
    }
    for object in dropped {
        let read = block_chains(&format!("With {object}\n.x.y = 1\n"));
        assert_eq!(read[1], "x.y");
    }
}

#[test]
fn an_end_with_that_closes_nothing_is_ignored() {
    let source = "End With\n.A.B = 1\nWith x\nEnd With\nEnd With\n.C.D = 1\n";
    assert_eq!(block_chains(source), ["", "A.B", "", "", "", "C.D"]);
}

#[test]
fn the_end_of_a_procedure_closes_the_blocks_it_left_open() {
    for end in ["End Sub", "End Function", "End Property"] {
        let source = format!("With x\nWith y\n{end}\n.A.B = 1\n");
        assert_eq!(block_chains(&source), ["", "", "", "A.B"], "{end}");
    }
}

#[test]
fn blocks_beyond_the_limit_have_no_known_object_and_still_close_in_order() {
    let mut source: String = (0..MAX_WITH_BLOCKS)
        .map(|n| format!("With o{n}\n"))
        .collect();
    // One block too many, and what it holds.
    source.push_str("With .b\n.c.d = 1\nEnd With\n.c.d = 1\n");
    source.push_str(&"End With\n".repeat(MAX_WITH_BLOCKS));
    source.push_str(".e.f = 1\n");
    let read = block_chains(&source);
    assert_eq!(read[MAX_WITH_BLOCKS + 1], "c.d");
    let innermost = MAX_WITH_BLOCKS - 1;
    assert_eq!(read[MAX_WITH_BLOCKS + 3], format!("o{innermost}.c.d"));
    assert_eq!(read.last().map(String::as_str), Some("e.f"));
}

#[test]
fn a_long_object_is_not_copied_into_every_statement_of_a_long_block() {
    // Ninety names of ten letters weigh 3,780, and 20,000 statements start
    // with a dot.
    let object = ["aaaaaaaaaa"; 90].join(".");
    let source = format!("With {object}\n{}End With\n", ".x.y = 1\n".repeat(20_000));
    let read = in_time(|| block(&source));
    // What the statements copy of the object, a name weighing its bytes and
    // 32, is paid for out of what they earn: 128 and four times the text each.
    let copied: usize = read
        .iter()
        .skip(1)
        .flat_map(|facts| &facts.chains)
        .map(|chain| chain.len().saturating_sub(2))
        .sum();
    let (weight, earned) = (copied * 42, 128 * read.len() + 4 * source.len());
    assert!(weight <= earned, "{weight} copied for {earned} earned");
    // The first statement gets the object, and the second falls short.
    assert_eq!(read[1].chains.first().map(Vec::len), Some(92));
    assert_eq!(read[2].chains, [["x", "y"]]);
}

#[test]
fn a_nested_block_keeps_the_end_of_the_chain_when_the_allowance_falls_short() {
    // The allowance falls short for the second statement and the nested
    // `With`: its object is the end of the real chain, and so is what it holds.
    let object = ["aaaaaaaaaa"; 90].join(".");
    let inside = ".x.y = 1\n.x.y = 1\nWith .z\n.u.v = 1\nEnd With\n";
    let read = block_chains(&format!("With {object}\n{inside}End With\n"));
    assert_eq!(read[1], format!("{object}.x.y"));
    assert_eq!(read[2..=4], ["x.y", "", "z.u.v"]);
}

#[test]
fn a_long_statement_does_not_bank_copies_for_the_next_ones() {
    // Ninety-seven names of ten letters weigh 4,074. The string of 100,000
    // bytes earns 400,000, and the statement after it, with 200 dots, copies
    // the object as many times as the 16 KiB it may carry and its own text
    // allow: five, not a hundred.
    let object = ["aaaaaaaaaa"; 97].join(".");
    let string = format!("x = \"{}\"", "a".repeat(100_000));
    let dots = format!("y = {}", [".a"; 200].join(" + "));
    let read = block(&format!("With {object}\n{string}\n{dots}\n"));
    // A dot that gets no object makes a chain of one name, which is not listed.
    assert_eq!(read[2].chains.len(), 5);
}

#[test]
fn a_with_after_the_then_or_the_else_of_a_single_line_if_opens_a_block() {
    for opener in [
        "If c Then With b",
        "If c Then x = 1 Else With b",
        "ElseIf c Then With b",
    ] {
        let source = format!("With a\n{opener}: .x.y = 1: End With\n.z.w = 1\nEnd With\n");
        let expected = ["", "", "b.x.y", "", "a.z.w", ""];
        assert_eq!(block_chains(&source), expected, "{opener}");
    }
}

#[test]
fn many_statements_in_a_block_are_read_in_proportion_to_their_number() {
    let source = format!("With a\n{}End With\n", ".x.y = 1\n".repeat(100_000));
    let read = in_time(|| block(&source));
    assert_eq!(read.len(), 100_002);
    assert!(
        read[1..=100_000]
            .iter()
            .all(|facts| facts.chains == [["a", "x", "y"]])
    );
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
    assert_eq!((chain.members.len(), chain.calls.len()), (100_002, 0));
    // The chain is `a` and the 100,000 names after it, without the `x`.
    let lengths: Vec<_> = chain.chains.iter().map(Vec::len).collect();
    assert_eq!(lengths, [100_001]);
    let arguments = facts_in_time(&format!("f({})", "a, ".repeat(100_000)));
    assert_eq!(arguments.members.len(), 100_001);
    assert_eq!(arguments.calls.len(), 100_001);
    assert!(arguments.chains.is_empty());
}

#[test]
fn nesting_beyond_the_limit_hides_no_name() {
    // Read flat, without overflowing the stack and with the statement
    // flagged: what lies below the limit is listed all the same, a name
    // that stands alone (`Shell x`) as a call too.
    let (open, close) = ("(".repeat(100_000), ")".repeat(100_000));
    for text in [
        format!("{}Shell(x){close}", "f(".repeat(100_000)),
        format!("{}Shell(x)", "a.b(".repeat(100_000)),
        format!("y = {}Shell(x)", "Not ".repeat(100_000)),
        format!("If {open}x{close} Then Shell x"),
    ] {
        let read = facts_in_time(&text);
        assert!(read.too_deep);
        assert!(read.calls.iter().any(|call| call.name == "Shell"));
        assert!(read.members.iter().any(|member| member.name == "x"));
    }
    let read = facts_in_time(&format!("{open}.Run Path:=1"));
    assert_eq!(members_of(&read), "Run Path:=");
    // What lies below the limit is no chain: its names are listed flat.
    let read = facts_in_time(&format!("y = {}a.b", "Not ".repeat(100_000)));
    assert!(read.too_deep);
    assert!(read.chains.is_empty());
}

#[test]
fn a_long_run_of_library_names_is_read_in_proportion_to_its_size() {
    // Read flat, a call has its qualifiers read back from the tokens before
    // it: a run of names costs once, whether one call or many follow runs.
    let (run, open) = ("VBA.".repeat(100_000), "f(".repeat(100_000));
    let library = |facts: &Facts| facts.calls.iter().filter(|call| call.library).count();
    let read = facts_in_time(&format!("x = {run}Shell(x)"));
    assert!(!read.too_deep);
    assert_eq!(library(&read), 1);
    let read = facts_in_time(&format!("{open}{run}Shell(x)"));
    assert!(read.too_deep);
    assert_eq!(library(&read), 1);
    let read = facts_in_time(&format!("{open}{}", "VBA.Shell(x) + ".repeat(50_000)));
    assert_eq!(library(&read), 50_000);
}
