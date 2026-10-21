// SPDX-License-Identifier: Apache-2.0
//! The statements that cannot be read (SPEC M2 item 6): the ones that the
//! parser records as `Unparseable`, the line it records them at, and the valid
//! code that it must leave alone.

mod vba_parse_common;

use macroport_core::report::ProcedureKind::Sub;
use macroport_core::vba::{MAX_RECOVERIES, MAX_TOKENS, ParsedModule, Recovery, RecoveryKind};
use vba_parse_common::{module_of, spans, unreadable};

/// A module of one procedure, `P`, with `body` as its body: the header is on
/// the line 1 and the first line of the body on the line 2.
fn in_procedure(body: &str) -> String {
    format!("Sub P()\n{body}\nEnd Sub\n")
}

/// The recoveries of the procedures of `module`, procedure by procedure.
fn procedure_recoveries(module: &ParsedModule) -> Vec<&[Recovery]> {
    module
        .procedures
        .iter()
        .map(|p| p.recoveries.as_slice())
        .collect()
}

/// Statements that valid code holds, each at the edge of one of the checks.
const READABLE: &[&str] = &[
    // The operators and the literals.
    "x = 1",
    "x = -1",
    "x = a + b - c * d / e \\ f ^ g",
    "x = a & \"s\" & b",
    "x = Not a",
    "x = a Mod b",
    "x = a And b Or c Xor d Eqv e Imp f",
    "x = a Like \"*z\"",
    "x = (a + b) * c",
    "x = 1.",
    "x = .5",
    "x = 1E3",
    "x = &HFF&",
    "x = 1!",
    "x = #1/1/2000#",
    "x = \"a\"\"b\"",
    "x = \"(\" & \")\" & \":\"",
    // The type characters.
    "x$ = Left$(y$, 2)",
    "Dim a&, b%, c$, d!, e#, f@",
    "Const k! = 1.5!",
    "Dim s As String * 10",
    // A `To`, a `Then`, an `In` or a `Step` that belongs to the statement.
    "Dim a(1 To 10) As Long",
    "ReDim Preserve a(1 To n)",
    "Lock #1, 1 To 5",
    "Unlock #1, 1 To 5",
    // The member of a `Type` that a sample cut from a longer macro leaves
    // without its opener.
    "Items(1 To 10) As Long",
    "For i = 1 To 10 Step 2",
    "For Each c In r.Cells",
    "If a Is Nothing Then x = 1 Else x = 2",
    "If TypeOf x Is Foo Then",
    "If a Then b = 1: c = 2",
    "ElseIf a Then",
    "Select Case x",
    "Case 1 To 5",
    "Case Is >= 3",
    "Case Else",
    "Else",
    "Do While x",
    "Loop Until x",
    "Wend",
    "Next",
    // The words of a statement, used as names where VBA allows it.
    "Foo a:=1, To:=2",
    "x.Step = 5",
    "[To] = 5",
    "x = rs!Name",
    "x = rs![Full Name]",
    "x = rs.Fields(\"a\")!b",
    "x = y.Item(1).Name",
    "With rs",
    ".Fields!Name = 1",
    // The statements that end with a comma or a semicolon.
    "Print #1, x;",
    "Print #1, x,",
    "Debug.Print x; y",
    "Debug.Print \"Done!\"",
    // The statements of files, flow and objects.
    "Open \"f\" For Input As #1",
    "Line Input #1, s",
    "Get #1, , x",
    "Name \"a\" As \"b\"",
    "Mid$(s, 1, 2) = \"ab\"",
    "Set x = New Collection",
    "x = Foo(AddressOf Bar)",
    "On Error GoTo 0",
    "On Error Resume Next",
    "Resume Next",
    "Exit Sub",
    "GoTo Done",
    // Several statements, a label, a line number, a comment.
    "x = 1: y = 2",
    "Done:",
    "10 x = 1",
    "x = 1 ' what ( is this",
    "Rem an ( unbalanced comment",
];

#[test]
fn the_statements_that_valid_code_holds_are_not_flagged() {
    for &statement in READABLE {
        // Inside a procedure, then outside any.
        let module = module_of(&in_procedure(statement));
        assert!(!module.is_recovered(), "{statement:?}: {module:?}");
        let module = module_of(&format!("{statement}\n"));
        assert!(!module.is_recovered(), "{statement:?}: {module:?}");
    }
}

/// Statements that cannot be read, by the check that finds them.
const UNREADABLE: &[&str] = &[
    // A character that is no token.
    "x = 1 ? 2",
    "x = \u{201c}abc\u{201d}",
    "x = a ` b",
    // A string that never closes.
    "x = \"abc",
    "Debug.Print \"abc; x",
    // A parenthesis that nothing matches.
    "x = (1 + 2",
    "x = 1 + 2)",
    "Foo (1, (2)",
    "Foo 1, 2)",
    // A statement that stops short, on a `Not`, an operator or a `.`. The
    // second and the third are in the corpus.
    "x = Not",
    "MsgBox \"a\" &",
    "Do manipulations of your  file here.",
    // A line of prose: a name, then a word of the statements that start with a
    // keyword.
    "you need to import the module",
    "Please note: you need to import the module in order to work",
    "Foo Then",
    "Foo Step 2",
    "Foo In bar",
    // The parentheses of a group that closes do not hide what follows.
    "Items(1) To 5",
];

/// Checks that each of `statements` is flagged on the line 2 inside a
/// procedure and on the line 1 outside any, and that it is the procedure's
/// recovery in the first case and the module's in the second.
fn assert_all_flagged<'a>(statements: impl IntoIterator<Item = &'a str>) {
    for statement in statements {
        let module = module_of(&in_procedure(statement));
        assert_eq!(
            procedure_recoveries(&module),
            [&[unreadable(2)][..]],
            "{statement:?}"
        );
        assert!(module.recoveries.is_empty(), "{statement:?}");
        let module = module_of(&format!("{statement}\n"));
        assert_eq!(module.recoveries, [unreadable(1)], "{statement:?}");
    }
}

#[test]
fn a_statement_that_fails_a_check_is_flagged() {
    assert_all_flagged(UNREADABLE.iter().copied());
}

/// The operators that take two operands, spelled.
const BINARY_OPERATORS: &[&str] = &[
    "+", "-", "*", "/", "\\", "^", "&", "=", "<>", "<", ">", "<=", ">=", "And", "Or", "Xor", "Eqv",
    "Imp", "Mod", "Like", "Is",
];

#[test]
fn an_operator_that_ends_a_statement_has_no_right_operand() {
    let ending: Vec<String> = BINARY_OPERATORS
        .iter()
        .map(|operator| format!("x = a {operator}"))
        .collect();
    assert_all_flagged(ending.iter().map(String::as_str));
    // With its operand, the same operator is left alone.
    for operator in BINARY_OPERATORS {
        let module = module_of(&in_procedure(&format!("x = a {operator} b")));
        assert!(!module.is_recovered(), "{operator}: {module:?}");
    }
}

#[test]
fn a_dot_or_a_bang_that_ends_a_statement_reaches_no_member() {
    assert_all_flagged(["x = a.", "x = a.b.", "x = rs.Fields(1)!", "...", ".", "!"]);
    // A number may end with its dot, and a name with its type character.
    for statement in ["x = 1.", "x = a!", "x = a.b!", "x = a.b"] {
        let module = module_of(&in_procedure(statement));
        assert!(!module.is_recovered(), "{statement:?}: {module:?}");
    }
}

/// The marks, the literals and the words that no statement starts with, besides
/// the binary operators.
const NO_START: &[&str] = &[
    ",",
    ";",
    ":=",
    ")",
    "\"s\"",
    "1.5",
    "#1/1/2000#",
    "Not",
    "Then",
    "To",
    "In",
    "Step",
    "As",
    "Each",
    "Preserve",
    "ByVal",
    "ByRef",
    "Optional",
    "ParamArray",
    "New",
    "AddressOf",
    "TypeOf",
];

#[test]
fn a_mark_or_a_word_that_starts_no_statement_is_flagged() {
    let starting: Vec<String> = BINARY_OPERATORS
        .iter()
        .chain(NO_START)
        .map(|start| format!("{start} x"))
        .collect();
    assert_all_flagged(starting.iter().map(String::as_str));
}

#[test]
fn the_line_is_the_physical_line_of_the_token_that_fails() {
    // The lines are counted as the file presents them: the `Attribute` line and
    // the continuation lines included.
    let lines = [
        "Attribute VB_Name = \"M\"",
        "Sub P()",
        "    x = Foo(1, _",
        "            2",
        "    y = 1 + _",
        "        \"abc",
        "    z = (1 + _",
        "        2 +",
        "    w = Foo(1, _",
        "        (2)",
        "    v = Foo(1) + _",
        "        Bar(2",
        "End Sub",
    ];
    let module = module_of(&lines.join("\n"));
    // The `(` of the first, the string of the second, the first of the two
    // faults of the third, the `(` of the fourth, the one that nothing closes,
    // and the `(` of the fifth that nothing closes, after a pair that does.
    assert_eq!(
        procedure_recoveries(&module),
        [&[
            unreadable(3),
            unreadable(6),
            unreadable(7),
            unreadable(9),
            unreadable(12)
        ][..]]
    );
}

#[test]
fn an_unreadable_statement_changes_neither_the_spans_nor_the_declarations() {
    let text = "Dim a As (\nSub P()\n    x = (\nEnd Sub\nSub Q()\nEnd Sub\n";
    let module = module_of(text);
    assert_eq!(module.recoveries, [unreadable(1)]);
    assert_eq!(spans(&module), [("P", Sub, 2, 4), ("Q", Sub, 5, 6)]);
    assert_eq!(
        procedure_recoveries(&module),
        [&[unreadable(3)][..], &[][..]]
    );
    // The `Dim` is still a declaration.
    assert_eq!(module.declarations.len(), 1);
}

#[test]
fn the_header_and_the_end_of_a_procedure_are_read_like_the_other_statements() {
    // A parenthesis that nothing closes, in the header.
    let module = module_of("Sub P(a, b\nEnd Sub\n");
    assert_eq!(procedure_recoveries(&module), [&[unreadable(1)][..]]);
    // Something after the end that no end holds, recorded in the procedure that
    // the end closes.
    let module = module_of("Sub P()\nEnd Sub )\n");
    assert_eq!(procedure_recoveries(&module), [&[unreadable(2)][..]]);
    assert_eq!(spans(&module).len(), 1);
    // The second form of a header is a header too, and the recovery is the one
    // procedure's.
    let module = module_of("#If A Then\nSub P()\n#Else\nSub P(x\n#End If\nEnd Sub\n");
    assert_eq!(procedure_recoveries(&module), [&[unreadable(4)][..]]);
    // A stray end is read too: it is outside any procedure.
    let module = module_of("End Sub )\n");
    let stray = Recovery {
        line: 1,
        kind: RecoveryKind::StrayTerminator,
    };
    assert_eq!(module.recoveries, [unreadable(1), stray]);
}

#[test]
fn the_members_of_a_block_the_labels_the_directives_and_the_comments_are_not_read() {
    let text = "Type T\n    a As\n    ) (\nEnd Type\n\
                Enum E\n    A = (\nEnd Enum\n\
                #If Foo( Then\n#Else\n#End If\n\
                Sub P()\n' a comment with ( in it\nDone:\nEnd Sub\n";
    let module = module_of(text);
    assert!(!module.is_recovered(), "{module:?}");
}

#[test]
fn an_unreadable_statement_is_recorded_once_for_a_physical_line() {
    // Two statements on a line, both unreadable.
    let module = module_of(&in_procedure("x = ( : y = )"));
    assert_eq!(procedure_recoveries(&module), [&[unreadable(2)][..]]);
    // A header that names nothing and cannot be read either.
    let module = module_of("Sub (a, b\nEnd Sub\n");
    assert_eq!(procedure_recoveries(&module), [&[unreadable(1)][..]]);
    // Two lines are two recoveries.
    let module = module_of(&in_procedure("x = (\ny = )"));
    assert_eq!(
        procedure_recoveries(&module),
        [&[unreadable(2), unreadable(3)][..]]
    );
}

#[test]
fn the_other_recoveries_of_a_physical_line_are_not_merged() {
    // Two ends with nothing to close, on one line: two events.
    let stray = Recovery {
        line: 1,
        kind: RecoveryKind::StrayTerminator,
    };
    let module = module_of("End Sub : End Sub\n");
    assert_eq!(module.recoveries, [stray, stray]);
}

#[test]
fn the_unreadable_statements_of_a_procedure_and_of_a_module_are_capped() {
    let many = MAX_RECOVERIES + 5;
    let body = "x = (\n".repeat(many);
    let module = module_of(&format!("Sub P()\n{body}End Sub\n"));
    let [recoveries] = procedure_recoveries(&module)[..] else {
        panic!("{module:?}");
    };
    // The first ones are kept.
    let first: Vec<_> = (2u32..).take(MAX_RECOVERIES).map(unreadable).collect();
    assert_eq!(recoveries, first);
    let module = module_of(&body);
    let first: Vec<_> = (1u32..).take(MAX_RECOVERIES).map(unreadable).collect();
    assert_eq!(module.recoveries, first);
}

#[test]
fn a_statement_that_the_lexer_had_to_cut_cannot_be_read() {
    // The tokens past the limit make one last token that nothing reads.
    let statement = format!("x = 1{}", " + 1".repeat(MAX_TOKENS));
    let module = module_of(&in_procedure(&statement));
    assert_eq!(procedure_recoveries(&module), [&[unreadable(2)][..]]);
}
