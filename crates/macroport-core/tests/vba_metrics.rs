// SPDX-License-Identifier: Apache-2.0
//! The metrics of a procedure (SPEC M2 item 7): each rule of the cyclomatic
//! complexity, the `GoTo`s and the sizes, one test each.

// clippy.toml excuses `expect` in the `#[test]` functions only, not in helpers.
#![allow(clippy::expect_used)]

use macroport_core::report::Metrics;
use macroport_core::vba::{measure_procedure, split_lines};

/// The metrics of the procedure that `source` is, with `parameters`.
fn measure_source(source: &str, parameters: usize) -> Metrics {
    let lines = split_lines(source).expect("lines");
    measure_procedure(1, lines.physical_lines, parameters, &lines.logical)
}

/// The metrics of a `Sub` whose body is `body`.
fn measure(body: &str) -> Metrics {
    measure_source(&format!("Sub Test()\n{body}\nEnd Sub\n"), 0)
}

fn cyclomatic(body: &str) -> u32 {
    measure(body).cyclomatic.expect("cyclomatic")
}

fn gotos(body: &str) -> u32 {
    measure(body).goto_count.expect("goto_count")
}

#[test]
fn a_procedure_without_a_decision_point_has_a_cyclomatic_complexity_of_one() {
    assert_eq!(cyclomatic(""), 1);
    assert_eq!(cyclomatic("x = 1\nDebug.Print x"), 1);
}

#[test]
fn each_if_and_elseif_adds_one() {
    assert_eq!(cyclomatic("If a Then\nx = 1\nEnd If"), 2);
    let chain = "If a Then\nx = 1\nElseIf b Then\nx = 2\nElseIf c Then\nx = 3\nElse\nx = 4\nEnd If";
    assert_eq!(cyclomatic(chain), 4);
    assert_eq!(cyclomatic("If a Then\nIf b Then\nx = 1\nEnd If\nEnd If"), 3);
}

#[test]
fn a_single_line_if_adds_one_whatever_it_holds() {
    assert_eq!(cyclomatic("If a Then x = 1"), 2);
    assert_eq!(cyclomatic("If a Then x = 1 Else x = 2"), 2);
    assert_eq!(cyclomatic("If a Then: x = 1"), 2);
    // An If inside the one-line branch is another decision.
    assert_eq!(cyclomatic("If a Then If b Then x = 1 Else x = 2"), 3);
    assert_eq!(cyclomatic("If a Then x = 1: If b Then y = 2"), 3);
}

#[test]
fn a_conditional_compilation_directive_adds_nothing() {
    let body = "#If Win64 Then\nx = 1\n#ElseIf Mac Then\nx = 2\n#Else\nx = 3\n#End If";
    assert_eq!(cyclomatic(body), 1);
}

#[test]
fn each_case_clause_adds_one_except_case_else() {
    let body = "Select Case n\nCase 1\nx = 1\nCase 2\nx = 2\nCase Else\nx = 3\nEnd Select";
    assert_eq!(cyclomatic(body), 3);
    // The Select Case itself is no decision.
    assert_eq!(cyclomatic("Select Case n\nCase Else\nx = 1\nEnd Select"), 1);
}

#[test]
fn a_case_clause_with_several_values_adds_one() {
    assert_eq!(
        cyclomatic("Select Case n\nCase 1, 2, 3\nx = 1\nEnd Select"),
        2
    );
    let ranges = "Select Case n\nCase 1 To 5, 7, Is > 9\nx = 1\nEnd Select";
    assert_eq!(cyclomatic(ranges), 2);
}

#[test]
fn each_loop_statement_adds_one() {
    for (body, expected) in [
        ("For i = 1 To 3\nNext i", 2),
        ("For i = 1 To 9 Step 2\nNext", 2),
        ("For Each c In r\nNext c", 2),
        ("Do\nLoop", 2),
        ("While x\nWend", 2),
        ("For i = 1 To 3\nFor j = 1 To 3\nNext j\nNext i", 3),
        ("For i = 1 To 3\nFor Each c In r\nDo\nLoop\nNext\nNext", 4),
    ] {
        assert_eq!(cyclomatic(body), expected, "{body}");
    }
}

#[test]
fn a_do_loop_adds_one_whether_its_condition_is_on_do_or_on_loop() {
    for body in [
        "Do While x\nLoop",
        "Do Until x\nLoop",
        "Do\nLoop While x",
        "Do\nLoop Until x",
    ] {
        assert_eq!(cyclomatic(body), 2, "{body}");
    }
}

#[test]
fn the_words_of_other_statements_start_no_loop_and_no_decision() {
    for body in [
        "Open f For Input As #1",
        "Open f For Output As #1\nClose #1",
        "On Error Resume Next",
        "Exit Sub",
        "Err.Raise 5",
        "Set r = Nothing",
        "Dim a As Long, b As Variant",
    ] {
        assert_eq!(cyclomatic(body), 1, "{body}");
    }
    // The Exit For and Exit Do inside loops add nothing.
    let exits = "For i = 1 To 3\nIf i = 2 Then Exit For\nNext i\nDo\nExit Do\nLoop";
    assert_eq!(cyclomatic(exits), 4);
}

#[test]
fn and_or_and_xor_add_one_each_in_the_condition_of_an_if_or_an_elseif() {
    assert_eq!(cyclomatic("If a And b Or c Xor d Then\nx = 1\nEnd If"), 5);
    let elseif = "If a Then\nx = 1\nElseIf b And c Then\nx = 2\nEnd If";
    assert_eq!(cyclomatic(elseif), 4);
    assert_eq!(cyclomatic("If (a And b) Or Not c Then x = 1"), 4);
    // The condition of a second If on the line counts too.
    assert_eq!(cyclomatic("If a Then If b And c Then x = 1"), 4);
}

#[test]
fn and_or_and_xor_add_one_each_in_the_condition_of_a_loop() {
    assert_eq!(cyclomatic("Do While a And b\nLoop"), 3);
    assert_eq!(cyclomatic("Do Until a Or b\nLoop"), 3);
    assert_eq!(cyclomatic("Do\nLoop While a Xor b"), 3);
    assert_eq!(cyclomatic("Do\nLoop Until a And b Or c"), 4);
    assert_eq!(cyclomatic("While a And b And c\nWend"), 4);
}

#[test]
fn and_or_and_xor_outside_a_condition_add_nothing() {
    assert_eq!(cyclomatic("x = a And b"), 1);
    assert_eq!(cyclomatic("x = (a Or b) Xor c"), 1);
    // What follows the Then of a single-line If is not its condition.
    assert_eq!(cyclomatic("If a Then x = b And c"), 2);
    assert_eq!(cyclomatic("If a Then x = 1 Else x = b Or c"), 2);
    // Nor is the line of a For, a Select Case or a Case.
    assert_eq!(cyclomatic("For i = 1 To a And b\nNext i"), 2);
    let select = "Select Case a And b\nCase c Or d\nx = 1\nEnd Select";
    assert_eq!(cyclomatic(select), 2);
}

#[test]
fn on_error_goto_a_label_adds_one_but_goto_zero_adds_none() {
    assert_eq!(cyclomatic("On Error GoTo Handler"), 2);
    assert_eq!(cyclomatic("On Error GoTo 10"), 2);
    assert_eq!(cyclomatic("On Error GoTo 0"), 1);
    assert_eq!(cyclomatic("On Error GoTo 00"), 1);
    assert_eq!(cyclomatic("On Error GoTo 0%"), 1);
    assert_eq!(cyclomatic("On Error Resume Next"), 1);
    assert_eq!(cyclomatic("On Error GoTo -1"), 1);
    assert_eq!(cyclomatic("On Error GoTo"), 1);
    let body = "On Error GoTo Handler\nOn Error GoTo 0\nOn Error GoTo Other";
    assert_eq!(cyclomatic(body), 3);
    assert_eq!(cyclomatic("If a Then On Error GoTo Handler"), 3);
}

#[test]
fn each_iif_call_adds_one() {
    assert_eq!(cyclomatic("x = IIf(a, 1, 2)"), 2);
    assert_eq!(cyclomatic("x = IIf(a, IIf(b, 1, 2), 3)"), 3);
    assert_eq!(cyclomatic("x = iif (a, 1, 2) + IIF(b, 3, 4)"), 3);
    assert_eq!(cyclomatic("If IIf(a, b, c) Then x = 1"), 3);
    // Not a call of the function: a member, or a name with no arguments.
    assert_eq!(cyclomatic("x = obj.IIf(a, 1, 2)"), 1);
    assert_eq!(cyclomatic("x = IIf"), 1);
}

#[test]
fn keywords_in_strings_and_comments_count_for_nothing() {
    let strings = "x = \"If a Then\" & \"And For Do\" ' If b Then";
    assert_eq!(cyclomatic(strings), 1);
    assert_eq!(cyclomatic("Rem If a Then\n' For i = 1 To 3"), 1);
    assert_eq!(cyclomatic("If a Then Rem And b"), 2);
}

#[test]
fn the_rules_add_up() {
    let body = "On Error GoTo Fail\n\
                For Each c In r\n\
                If c.Value > 0 And c.Value < 9 Then\n\
                Select Case c.Value\n\
                Case 1, 2\n\
                x = IIf(a, 1, 2)\n\
                Case Else\n\
                x = 0\n\
                End Select\n\
                End If\n\
                Next c";
    // 1, then the On Error, For Each, If, And, Case and IIf.
    assert_eq!(cyclomatic(body), 7);
}

#[test]
fn goto_and_gosub_count_but_on_error_goto_does_not() {
    assert_eq!(gotos("GoTo Done\nGoSub Work\nDone:\nWork:"), 2);
    assert_eq!(gotos("If a Then GoTo Done"), 1);
    assert_eq!(gotos("On n GoTo A, B, C"), 1);
    assert_eq!(gotos("On n GoSub A, B"), 1);
    assert_eq!(gotos("On Error GoTo Handler"), 0);
    assert_eq!(gotos("On Error GoTo 0"), 0);
    assert_eq!(gotos("On Error GoTo Handler\nGoTo Done"), 1);
    assert_eq!(gotos("Application.Goto Reference:=Range(\"A1\")"), 0);
    assert_eq!(gotos("With Application\n.GoTo Range(\"A1\")\nEnd With"), 0);
    assert_eq!(gotos("x = \"GoTo Done\" ' GoSub Work"), 0);
}

#[test]
fn loc_counts_the_physical_lines_from_the_header_to_the_end() {
    assert_eq!(measure_procedure(10, 19, 0, &[]).loc, 10);
    assert_eq!(measure_procedure(4, 4, 0, &[]).loc, 1);
    // Bad bounds do not wrap around.
    assert_eq!(measure_procedure(9, 3, 0, &[]).loc, 1);
    let source = "Sub Test()\n\n' note\nx = 1\n\nEnd Sub\n";
    assert_eq!(measure_source(source, 0).loc, 6);
}

#[test]
fn logical_loc_leaves_out_blank_and_comment_only_lines() {
    let source = "Sub Test()\n\n' note\nx = 1 ' trailing\n\nRem another\ny = _\n  2\nEnd Sub\n";
    let metrics = measure_source(source, 0);
    assert_eq!(metrics.loc, 9);
    // The header, the two statements, the End.
    assert_eq!(metrics.logical_loc, 4);
}

#[test]
fn logical_loc_counts_each_statement_of_a_chain() {
    let source = "Sub Test()\na = 1: b = 2: c = 3\nEnd Sub\n";
    assert_eq!(measure_source(source, 0).logical_loc, 5);
}

#[test]
fn a_label_is_a_line_unless_a_statement_follows_it_on_its_line() {
    let source = "Sub Test()\nHandler:\nx = 1\n10 y = 2\nDone: z = 3\nEnd Sub\n";
    // The header, `Handler:`, `x = 1`, `10 y = 2`, `Done: z = 3`, the End.
    assert_eq!(measure_source(source, 0).logical_loc, 6);
}

#[test]
fn the_count_of_parameters_is_the_parsers() {
    let two = measure_source("Sub Test(a, b)\nEnd Sub\n", 2);
    let many = measure_procedure(1, 1, usize::MAX, &[]);
    assert_eq!(two.parameters, Some(2));
    assert_eq!(many.parameters, Some(u32::MAX));
}

#[test]
fn only_the_metrics_of_a_procedure_are_given() {
    let metrics = measure("x = 1");
    assert_eq!(metrics.procedures, None);
    assert_eq!(metrics.cyclomatic_total, None);
    assert_eq!(metrics.max_nesting, None);
    assert_eq!(metrics.forms, None);
    assert_eq!(metrics.declare_count, None);
    assert_eq!(metrics.external_dependencies, None);
}

#[test]
fn a_text_that_is_not_a_procedure_is_measured_without_panicking() {
    let words = [
        "If", "ElseIf", "Else", "Then", "End", "Case", "Select", "For", "Each", "Next", "Do",
        "Loop", "While", "Until", "Wend", "With", "On", "Error", "GoTo", "GoSub", "IIf", "And",
        "Rem", "=", "<>", ":", ",", "(", ")", "\"", "'", "#", "[", "0",
    ];
    for first in words {
        for second in words {
            for body in [
                format!("{first} {second}"),
                format!("{first} {second} {first}"),
                format!("{first}\n{second}\n{first} {second}"),
            ] {
                // At least the one of a procedure with no decision point.
                assert!(measure(&body).cyclomatic >= Some(1), "{body:?}");
            }
        }
    }
}
