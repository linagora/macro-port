// SPDX-License-Identifier: Apache-2.0
//! The metrics of a procedure (SPEC M2 item 7): each rule of the cyclomatic
//! complexity, the `GoTo`s, the nesting and the sizes, one test each, and the
//! comparisons in nested loops (SPEC section 7).

// clippy.toml excuses `expect` in the `#[test]` functions only, not in helpers.
#![allow(clippy::expect_used)]

use macroport_core::vba::{ProcedureMeasure, measure_procedure, split_lines};

/// The measure of the procedure that `source` is, with `parameters`.
fn measure_source(source: &str, parameters: usize) -> ProcedureMeasure {
    let lines = split_lines(source).expect("lines");
    measure_procedure(1, lines.physical_lines, parameters, &lines.logical)
}

/// The measure of a `Sub` whose body is `body`.
fn measure(body: &str) -> ProcedureMeasure {
    measure_source(&format!("Sub Test()\n{body}\nEnd Sub\n"), 0)
}

fn cyclomatic(body: &str) -> u32 {
    measure(body).metrics.cyclomatic.expect("cyclomatic")
}

fn nesting(body: &str) -> u32 {
    measure(body).metrics.max_nesting.expect("max_nesting")
}

fn gotos(body: &str) -> u32 {
    measure(body).metrics.goto_count.expect("goto_count")
}

/// The positions of the comparisons in nested loops: the header is 0.
fn compared(body: &str) -> Vec<usize> {
    measure(body).nested_loop_compare
}

/// Whether the blocks of the procedure do not match.
fn unbalanced(body: &str) -> bool {
    measure(body).unbalanced_blocks
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
        "Exit For",
        "Err.Raise 5",
        "Set r = Nothing",
        "Dim a As Long, b As Variant",
    ] {
        assert_eq!(cyclomatic(body), 1, "{body}");
        assert_eq!(nesting(body), 0, "{body}");
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
fn nesting_counts_the_blocks_inside_one_another() {
    assert_eq!(nesting("x = 1"), 0);
    assert_eq!(nesting("If a Then\nx = 1\nEnd If"), 1);
    let three = "If a Then\nFor i = 1 To 3\nWith r\nx = 1\nEnd With\nNext i\nEnd If";
    assert_eq!(nesting(three), 3);
    let five = "If a Then\nSelect Case n\nCase 1\nDo While x\nWhile y\nWith r\nEnd With\n\
                Wend\nLoop\nEnd Select\nEnd If";
    assert_eq!(nesting(five), 5);
    let each = "For Each c In r\nFor Each d In c\nNext d\nNext c";
    assert_eq!(nesting(each), 2);
    // Blocks one after the other do not add up.
    let in_a_row = "If a Then\nx = 1\nEnd If\nFor i = 1 To 3\nNext i\nWith r\nEnd With";
    assert_eq!(nesting(in_a_row), 1);
}

#[test]
fn an_if_is_a_block_only_when_its_then_ends_the_line() {
    assert_eq!(nesting("If a Then x = 1"), 0);
    assert_eq!(nesting("If a Then x = 1 Else x = 2"), 0);
    assert_eq!(nesting("If a Then: x = 1"), 0);
    // A comment after the Then is no statement.
    assert_eq!(nesting("If a Then ' note\nx = 1\nEnd If"), 1);
    assert_eq!(nesting("If a Then Rem note\nx = 1\nEnd If"), 1);
    // ElseIf is no further level, but Else If in two words opens a block.
    let chain = "If a Then\nx = 1\nElseIf b Then\nx = 2\nElse\nx = 3\nEnd If";
    assert_eq!(nesting(chain), 1);
    let two_words = "If a Then\nx = 1\nElse If b Then\nx = 2\nEnd If\nEnd If";
    assert_eq!(nesting(two_words), 2);
}

#[test]
fn next_with_several_variables_closes_as_many_loops() {
    // Were only one closed, the three Withs would make four levels.
    let body = "For i = 1 To 3\nFor j = 1 To 3\nNext j, i\nWith r\nWith s\nWith t\n\
                End With\nEnd With\nEnd With";
    assert_eq!(nesting(body), 3);
}

#[test]
fn a_directive_closes_and_opens_nothing() {
    // The `#If` opens no If: the With in the pair is on two levels, not three.
    let opens = "If a Then\n#If Win64 Then\nWith r\nEnd With\n#End If\nEnd If";
    assert_eq!(nesting(opens), 2);
    // The `#End If` closes no If: the With after the pair is still in the If.
    let closes = "If a Then\n#If Win64 Then\n#End If\nWith r\nEnd With\nEnd If";
    assert_eq!(nesting(closes), 2);
}

#[test]
fn a_closer_with_no_block_of_its_kind_is_ignored() {
    let body = "End If\nNext i\nLoop\nWend\nEnd With\nEnd Select\nIf a Then\nx = 1\nEnd If";
    assert_eq!(nesting(body), 1);
    // A Wend does not close a For.
    let wend = "For i = 1 To 3\nWend\nIf a Then\nEnd If\nNext i";
    assert_eq!(nesting(wend), 2);
}

#[test]
fn a_closer_closes_the_blocks_left_open_inside_its_block() {
    // Two Withs follow each time: three levels if the first block stayed open.
    let withs = "With r\nWith s\nEnd With\nEnd With";
    // The For is never closed: the End If closes it with the If.
    let for_inside = format!("If a Then\nFor i = 1 To 3\nEnd If\n{withs}");
    assert_eq!(nesting(&for_inside), 2);
    // The If is never closed: the Next closes it with the For.
    let if_inside = format!("For i = 1 To 3\nIf a Then\nNext i\n{withs}");
    assert_eq!(nesting(&if_inside), 2);
}

#[test]
fn blocks_closed_in_the_order_they_open_are_balanced() {
    assert!(!unbalanced(""));
    assert!(!unbalanced("x = 1"));
    let every = "If a Then\nSelect Case n\nCase 1\nFor i = 1 To 3\nFor Each c In r\nDo While p\n\
                 Loop\nNext c, i\nWhile q\nWend\nWith r\nEnd With\nEnd Select\nEnd If";
    assert!(!unbalanced(every));
    // A one-line If is no block, ElseIf and Else need no closer, and an Else If
    // in two words is a block with its own.
    assert!(!unbalanced("If a Then x = 1 Else x = 2\nIf b Then: x = 3"));
    assert!(!unbalanced("If a Then\nElseIf b Then\nElse\nEnd If"));
    assert!(!unbalanced("If a Then\nElse If b Then\nEnd If\nEnd If"));
}

#[test]
fn a_block_never_closed_is_unbalanced() {
    for opener in [
        "If a Then",
        "Select Case n",
        "For i = 1 To 3",
        "For Each c In r",
        "Do",
        "Do While p",
        "While q",
        "With r",
    ] {
        assert!(unbalanced(&format!("{opener}\nx = 1")), "{opener}");
    }
    assert!(unbalanced("With r\nWith s\nEnd With"));
    assert!(unbalanced("If a Then\nElse If b Then\nEnd If"));
    // The prose line of the sample 13-05 starts a Do.
    assert!(unbalanced("Do manipulations of your file here.\nx = 1"));
}

#[test]
fn a_closer_with_no_block_of_its_kind_is_unbalanced() {
    for closer in ["End If", "End Select", "End With", "Next i", "Loop", "Wend"] {
        assert!(unbalanced(closer), "{closer}");
        // The closer of a block that is over, or of another kind than the open one.
        assert!(
            unbalanced(&format!("If a Then\nEnd If\n{closer}")),
            "{closer}"
        );
        assert!(
            unbalanced(&format!("With r\n{closer}\nEnd With")),
            "{closer}"
        );
    }
    // The second name of a Next with one loop open.
    assert!(unbalanced("For i = 1 To 3\nNext j, i"));
    // The Else and the End If that the sample 09-03 has too many.
    assert!(unbalanced("If a Then\nx = 1\nEnd If\nElse\nEnd If"));
}

#[test]
fn a_closer_that_closes_blocks_left_open_inside_its_own_is_unbalanced() {
    assert!(unbalanced("If a Then\nFor i = 1 To 3\nEnd If"));
    assert!(unbalanced("For i = 1 To 3\nIf a Then\nNext i"));
    assert!(unbalanced("With r\nDo\nEnd With"));
}

#[test]
fn a_comparison_in_a_loop_nested_in_a_loop_is_cited_by_its_position() {
    // The header is 0, the first loop 1: each If and ElseIf that compares is cited.
    let body = "For i = 1 To n\nFor j = 1 To n\nIf a(i) = b(j) Then\nc = 1\n\
                ElseIf a(i) <> b(j) Then\nc = 2\nElseIf d Then\nc = 3\nEnd If\nNext j\nNext i";
    assert_eq!(compared(body), [3, 5]);
}

#[test]
fn the_comparisons_are_equals_not_equals_and_strcomp() {
    let inner = |condition: &str| {
        compared(&format!(
            "For i = 1 To n\nFor j = 1 To n\nIf {condition} Then\nc = 1\nEnd If\nNext j\nNext i"
        ))
    };
    for condition in [
        "a = b",
        "a <> b",
        "a >< b",
        "StrComp(a, b) = 0",
        "StrComp(a, b)",
        "x And a = b",
    ] {
        assert_eq!(inner(condition), [3], "{condition}");
    }
    for condition in [
        "a < b",
        "a > b",
        "a <= b",
        "a >= b",
        "a",
        "IsEmpty(a)",
        "r.StrComp",
    ] {
        assert!(inner(condition).is_empty(), "{condition}");
    }
}

#[test]
fn the_condition_alone_decides_what_is_compared() {
    let body = "For i = 1 To n\nFor j = 1 To n\nIf a Then x = (b = c)\n\
                If a Then y = 1 Else y = (b = c)\nNext j\nNext i";
    assert!(compared(body).is_empty());
    let single = "For i = 1 To n\nFor j = 1 To n\nIf a = b Then x = 1\nNext j\nNext i";
    assert_eq!(compared(single), [3]);
}

#[test]
fn one_loop_is_not_enough_and_loops_in_a_row_are_not_nested() {
    assert!(compared("For i = 1 To n\nIf a = b Then x = 1\nNext i").is_empty());
    let in_a_row = "For i = 1 To n\nNext i\nFor j = 1 To n\nIf a = b Then x = 1\nNext j";
    assert!(compared(in_a_row).is_empty());
    // Between the loops, in the body of the outer one only.
    let outer = "For i = 1 To n\nIf a = b Then x = 1\nFor j = 1 To n\nNext j\nNext i";
    assert!(compared(outer).is_empty());
    // After the inner loop is over, the comparison is in the outer one alone.
    let after = "For i = 1 To n\nFor j = 1 To n\nNext j\nIf a = b Then x = 1\nNext i";
    assert!(compared(after).is_empty());
}

#[test]
fn every_kind_of_loop_nests() {
    for (outer, outer_end, inner, inner_end) in [
        ("For Each a In r", "Next a", "For Each b In s", "Next b"),
        ("Do While p", "Loop", "Do Until q", "Loop"),
        ("While p", "Wend", "While q", "Wend"),
        ("For i = 1 To 3", "Next i", "Do", "Loop"),
        ("Do", "Loop While p", "While q", "Wend"),
    ] {
        let body = format!("{outer}\n{inner}\nIf a = b Then x = 1\n{inner_end}\n{outer_end}");
        assert_eq!(compared(&body), [3], "{body}");
    }
}

#[test]
fn a_comparison_deeper_in_other_blocks_is_still_in_the_loops() {
    let body = "For i = 1 To n\nWith r\nFor j = 1 To n\nSelect Case i\nCase 1\n\
                If a = b Then x = 1\nEnd Select\nNext j\nEnd With\nNext i";
    assert_eq!(compared(body), [6]);
}

#[test]
fn the_test_of_a_loop_is_no_comparison_to_cite() {
    let body = "For i = 1 To n\nFor j = 1 To n\nDo While a = b\nLoop\nNext j\nNext i";
    assert!(compared(body).is_empty());
}

#[test]
fn loc_counts_the_physical_lines_from_the_header_to_the_end() {
    assert_eq!(measure_procedure(10, 19, 0, &[]).metrics.loc, 10);
    assert_eq!(measure_procedure(4, 4, 0, &[]).metrics.loc, 1);
    // Bad bounds do not wrap around.
    assert_eq!(measure_procedure(9, 3, 0, &[]).metrics.loc, 1);
    let source = "Sub Test()\n\n' note\nx = 1\n\nEnd Sub\n";
    assert_eq!(measure_source(source, 0).metrics.loc, 6);
}

#[test]
fn logical_loc_leaves_out_blank_and_comment_only_lines() {
    let source = "Sub Test()\n\n' note\nx = 1 ' trailing\n\nRem another\ny = _\n  2\nEnd Sub\n";
    let metrics = measure_source(source, 0).metrics;
    assert_eq!(metrics.loc, 9);
    // The header, the two statements, the End.
    assert_eq!(metrics.logical_loc, 4);
}

#[test]
fn logical_loc_counts_each_statement_of_a_chain() {
    let source = "Sub Test()\na = 1: b = 2: c = 3\nEnd Sub\n";
    assert_eq!(measure_source(source, 0).metrics.logical_loc, 5);
}

#[test]
fn a_label_is_a_line_unless_a_statement_follows_it_on_its_line() {
    let source = "Sub Test()\nHandler:\nx = 1\n10 y = 2\nDone: z = 3\nEnd Sub\n";
    // The header, `Handler:`, `x = 1`, `10 y = 2`, `Done: z = 3`, the End.
    assert_eq!(measure_source(source, 0).metrics.logical_loc, 6);
}

#[test]
fn the_count_of_parameters_is_the_parsers() {
    let two = measure_source("Sub Test(a, b)\nEnd Sub\n", 2);
    let many = measure_procedure(1, 1, usize::MAX, &[]);
    assert_eq!(two.metrics.parameters, Some(2));
    assert_eq!(many.metrics.parameters, Some(u32::MAX));
}

#[test]
fn only_the_metrics_of_a_procedure_are_given() {
    let metrics = measure("x = 1").metrics;
    assert_eq!(metrics.procedures, None);
    assert_eq!(metrics.cyclomatic_total, None);
    assert_eq!(metrics.forms, None);
    assert_eq!(metrics.declare_count, None);
    assert_eq!(metrics.external_dependencies, None);
}

#[test]
fn a_text_that_is_not_a_procedure_is_measured_without_panicking() {
    let words = [
        "If", "ElseIf", "Else", "Then", "End", "Case", "Select", "For", "Each", "Next", "Do",
        "Loop", "While", "Until", "Wend", "With", "On", "Error", "GoTo", "GoSub", "IIf", "StrComp",
        "And", "Rem", "=", "<>", ":", ",", "(", ")", "\"", "'", "#", "[", "0",
    ];
    for first in words {
        for second in words {
            for body in [
                format!("{first} {second}"),
                format!("{first} {second} {first}"),
                format!("{first}\n{second}\n{first} {second}"),
            ] {
                // At least the one of a procedure with no decision point.
                assert!(measure(&body).metrics.cyclomatic >= Some(1), "{body:?}");
            }
        }
    }
}
