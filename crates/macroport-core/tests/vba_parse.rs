// SPDX-License-Identifier: Apache-2.0
//! The procedures of the structural parser: their headers, their ends and
//! their spans.

mod vba_parse_common;

use macroport_core::report::{ProcedureKind, Visibility};
use macroport_core::vba::{MAX_RECOVERIES, ParsedModule, Procedure, Recovery, RecoveryKind};
use vba_parse_common::{module_of, spans};

use ProcedureKind::{Function, PropertyGet, PropertyLet, PropertySet, Sub};

/// Whether each procedure is recovered, in source order.
fn recovered(module: &ParsedModule) -> Vec<bool> {
    module
        .procedures
        .iter()
        .map(Procedure::is_recovered)
        .collect()
}

#[test]
fn a_procedure_runs_from_its_header_to_its_end() {
    let module = module_of("Option Explicit\n\nSub A()\n    x = 1\nEnd Sub\n");
    let [a] = module.procedures.as_slice() else {
        panic!("{module:?}");
    };
    assert_eq!(
        (a.name.as_str(), a.kind, a.visibility, a.is_static),
        ("A", Sub, Visibility::Default, false)
    );
    // The logical lines are `Option Explicit` and then the four of `A`.
    assert_eq!((a.start_line, a.end_line, a.lines.clone()), (3, 5, 1..4));
    assert!(!a.is_recovered());
    assert!(module.recoveries.is_empty());
}

#[test]
fn each_kind_of_procedure_is_told_by_its_header() {
    let text = "Sub S()\nEnd Sub\nFunction F() As Long\nEnd Function\n\
                Property Get G() As Long\nEnd Property\n\
                Property Let G(v As Long)\nEnd Property\n\
                Property Set G(v As Object)\nEnd Property\n";
    let module = module_of(text);
    assert_eq!(
        spans(&module),
        [
            ("S", Sub, 1, 2),
            ("F", Function, 3, 4),
            ("G", PropertyGet, 5, 6),
            ("G", PropertyLet, 7, 8),
            ("G", PropertySet, 9, 10),
        ]
    );
    assert!(module.procedures.iter().all(|p| !p.is_recovered()));
}

#[test]
fn the_modifiers_of_a_header_come_in_any_order_and_letter_case() {
    let text = "Public Sub A()\nEnd Sub\nPrivate Function B()\nEnd Function\n\
                Friend Property Get C()\nEnd Property\nStatic Sub D()\nEnd Sub\n\
                Public Static Sub E()\nEnd Sub\nSTATIC PRIVATE sub F()\nend SUB\n\
                Sub G()\nEnd Sub\n";
    let module = module_of(text);
    let found: Vec<_> = module
        .procedures
        .iter()
        .map(|p| (p.name.as_str(), p.visibility, p.is_static))
        .collect();
    assert_eq!(
        found,
        [
            ("A", Visibility::Public, false),
            ("B", Visibility::Private, false),
            ("C", Visibility::Friend, false),
            ("D", Visibility::Default, true),
            ("E", Visibility::Public, true),
            ("F", Visibility::Private, true),
            ("G", Visibility::Default, false),
        ]
    );
    assert!(module.recoveries.is_empty());
    // The other modules can call the public ones, and those that say nothing.
    let public: Vec<_> = module.procedures.iter().map(Procedure::is_public).collect();
    assert_eq!(public, [true, false, false, true, true, false, true]);
}

#[test]
fn a_name_is_given_without_its_brackets_or_its_type_character() {
    let text = "Function Name$() As String\nEnd Function\n\
                Sub [Odd Name]()\nEnd Sub\nFunction Count%()\nEnd Function\n";
    let module = module_of(text);
    let names: Vec<_> = module.procedures.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["Name", "Odd Name", "Count"]);
}

#[test]
fn a_header_that_names_nothing_opens_a_procedure_that_is_recovered() {
    let text = "Sub (a, b)\nEnd Sub\nPublic Sub\nEnd Sub\nSub 123()\nEnd Sub\nSub A()\nEnd Sub\n";
    let module = module_of(text);
    assert_eq!(
        spans(&module),
        [
            ("", Sub, 1, 2),
            ("", Sub, 3, 4),
            ("", Sub, 5, 6),
            ("A", Sub, 7, 8)
        ]
    );
    for (procedure, line) in module.procedures.iter().zip([1, 3, 5]) {
        assert_eq!(
            procedure.recoveries,
            [Recovery {
                line,
                kind: RecoveryKind::Unparseable
            }],
            "line {line}"
        );
    }
    assert!(!module.procedures[3].is_recovered());
    assert!(module.recoveries.is_empty());
    // It has no end either: the recoveries come in the order they are found.
    let module = module_of("Sub\n");
    let kinds: Vec<_> = module.procedures[0]
        .recoveries
        .iter()
        .map(|r| r.kind)
        .collect();
    assert_eq!(kinds, [RecoveryKind::Unparseable, RecoveryKind::MissingEnd]);
}

#[test]
fn the_parameters_of_a_header_are_counted() {
    // Each header stands alone: the End it lacks changes nothing to its count.
    let headers = [
        ("Sub A()", 0),
        ("Sub B", 0),
        ("Sub C(x)", 1),
        (
            "Function D(ByVal a As Long, Optional b As String = \"x,y\", ParamArray c() As Variant) As Long",
            3,
        ),
        ("Sub E(a As Long, _\n    b As Long)", 2),
        ("Property Let F(Optional ByVal i As Long = -1, v)", 2),
        ("Sub G(a() As Long, ByRef b() As String, )", 2),
        ("Sub (a, b)", 0),
        ("Sub H(a, b", 2),
    ];
    for (header, count) in headers {
        let module = module_of(header);
        let found: Vec<_> = module
            .procedures
            .iter()
            .map(|p| p.parameter_count)
            .collect();
        assert_eq!(found, [count], "{header:?}");
    }
}

#[test]
fn the_comments_inside_a_procedure_belong_to_it_and_the_ones_around_it_do_not() {
    let text = "' before\nSub A()\n    ' inside\n    x = 1\nEnd Sub ' after\n' between\nSub B()\nEnd Sub\n";
    let module = module_of(text);
    // The logical lines are the eight lines of the text, one for each.
    let ranges: Vec<_> = module.procedures.iter().map(|p| p.lines.clone()).collect();
    assert_eq!(ranges, [1..5, 6..8]);
    assert_eq!(spans(&module), [("A", Sub, 2, 5), ("B", Sub, 7, 8)]);
}

#[test]
fn a_procedure_written_on_one_line_starts_and_ends_on_it() {
    let module = module_of("Sub A(): End Sub\nSub B(): x = 1: End Sub\n");
    assert_eq!(spans(&module), [("A", Sub, 1, 1), ("B", Sub, 2, 2)]);
    let ranges: Vec<_> = module.procedures.iter().map(|p| p.lines.clone()).collect();
    assert_eq!(ranges, [0..2, 2..5]);
    assert!(module.procedures.iter().all(|p| !p.is_recovered()));
}

#[test]
fn a_header_over_several_physical_lines_starts_at_its_keyword() {
    let text =
        "Public Sub A( _\n    ByVal x As Long)\n    x = 1\nEnd Sub\nPublic _\nSub B()\nEnd Sub\n";
    let module = module_of(text);
    assert_eq!(spans(&module), [("A", Sub, 1, 4), ("B", Sub, 6, 7)]);
}

#[test]
fn a_procedure_without_an_end_is_closed_by_the_next_header() {
    let module = module_of("Sub A()\n    x = 1\n' trailing\nSub B()\nEnd Sub\n");
    assert_eq!(spans(&module), [("A", Sub, 1, 2), ("B", Sub, 4, 5)]);
    let [a, b] = module.procedures.as_slice() else {
        panic!("{module:?}");
    };
    // The comment after the last statement is no part of `A`.
    assert_eq!(a.lines, 0..2);
    assert_eq!(
        a.recoveries,
        [Recovery {
            line: 1,
            kind: RecoveryKind::MissingEnd
        }]
    );
    assert!(a.is_recovered() && !b.is_recovered());
}

#[test]
fn a_procedure_without_an_end_is_closed_by_the_end_of_the_module() {
    let module = module_of("Sub A()\n    x = 1\n\n' the end\n");
    let [a] = module.procedures.as_slice() else {
        panic!("{module:?}");
    };
    assert_eq!((a.start_line, a.end_line, a.lines.clone()), (1, 2, 0..2));
    assert_eq!(
        a.recoveries,
        [Recovery {
            line: 1,
            kind: RecoveryKind::MissingEnd
        }]
    );
}

#[test]
fn an_end_of_another_kind_closes_the_procedure_and_is_recovered() {
    let text = "Sub A()\nEnd Function\nProperty Get P()\nEnd Sub\nFunction F()\nEnd Property\n";
    let module = module_of(text);
    assert_eq!(
        spans(&module),
        [
            ("A", Sub, 1, 2),
            ("P", PropertyGet, 3, 4),
            ("F", Function, 5, 6)
        ]
    );
    for (procedure, line) in module.procedures.iter().zip([2, 4, 6]) {
        assert_eq!(
            procedure.recoveries,
            [Recovery {
                line,
                kind: RecoveryKind::WrongEnd
            }],
            "{}",
            procedure.name
        );
    }
}

#[test]
fn an_end_outside_a_procedure_is_recorded_for_the_module() {
    let module = module_of("End Sub\nSub A()\nEnd Sub\nEnd Function\n");
    assert_eq!(spans(&module), [("A", Sub, 2, 3)]);
    assert!(!module.procedures[0].is_recovered());
    assert_eq!(
        module.recoveries,
        [
            Recovery {
                line: 1,
                kind: RecoveryKind::StrayTerminator
            },
            Recovery {
                line: 4,
                kind: RecoveryKind::StrayTerminator
            },
        ]
    );
}

#[test]
fn a_module_is_recovered_when_a_procedure_is_or_when_something_stands_outside_one() {
    let cases = [
        ("", false),
        ("Sub A()\nEnd Sub\n", false),
        ("Sub A()\n", true),
        ("Sub A()\nEnd Function\n", true),
        // The procedure is fine: the stray end is the module's.
        ("Sub A()\nEnd Sub\nEnd Sub\n", true),
        ("End Sub\n", true),
    ];
    for (text, recovered) in cases {
        assert_eq!(module_of(text).is_recovered(), recovered, "{text:?}");
    }
}

#[test]
fn statements_that_look_like_headers_are_not_headers() {
    let text = "Declare Function F Lib \"x\" () As Long\nPublic Declare PtrSafe Sub S Lib \"y\" ()\n\
                Public WS As Worksheet\nProperty\nSub A()\n    Exit Sub\n    x = \"Sub B()\"\n    \
                ' Sub C()\n    End\nEnd Sub\n";
    let module = module_of(text);
    assert_eq!(spans(&module), [("A", Sub, 5, 10)]);
    assert!(module.recoveries.is_empty() && !module.procedures[0].is_recovered());
}

#[test]
fn two_forms_of_a_header_under_conditional_compilation_are_one_procedure() {
    let text = "#If VBA7 Then\nPublic Sub S(h As LongPtr)\n#Else\nPublic Sub S(h As Long)\n\
                #End If\n    x = 1\nEnd Sub\nSub T()\nEnd Sub\n";
    let module = module_of(text);
    // The procedure starts at its first form and holds the directives between.
    assert_eq!(spans(&module), [("S", Sub, 2, 7), ("T", Sub, 8, 9)]);
    let ranges: Vec<_> = module.procedures.iter().map(|p| p.lines.clone()).collect();
    assert_eq!(ranges, [1..7, 7..9]);
    assert!(!module.is_recovered());
}

#[test]
fn an_elseif_leads_to_another_form_of_the_header_too() {
    let text =
        "#If A Then\nSub S(a)\n#ElseIf B Then\nSub S(a, b)\n#Else\nSub S()\n#End If\nEnd Sub\n";
    let module = module_of(text);
    assert_eq!(spans(&module), [("S", Sub, 2, 8)]);
    // The name, the kind and the parameters are the ones of the first form.
    assert_eq!(module.procedures[0].parameter_count, 1);
    assert!(!module.is_recovered());
}

#[test]
fn a_procedure_in_each_branch_of_conditional_compilation_is_two_procedures() {
    let text = "#If Win64 Then\nSub P()\n    x = 64\nEnd Sub\n#Else\nSub P()\n    x = 32\nEnd Sub\n#End If\n";
    let module = module_of(text);
    assert_eq!(spans(&module), [("P", Sub, 2, 4), ("P", Sub, 6, 8)]);
    assert!(!module.is_recovered());
}

#[test]
fn conditional_compilation_inside_a_body_changes_nothing() {
    let text = "Sub A()\n#If Win64 Then\n    x = 64\n#Else\n    x = 32\n#End If\nEnd Sub\nSub B()\nEnd Sub\n";
    let module = module_of(text);
    assert_eq!(spans(&module), [("A", Sub, 1, 7), ("B", Sub, 8, 9)]);
    assert!(!module.is_recovered());
}

#[test]
fn a_header_after_the_end_of_the_conditional_is_a_new_procedure() {
    let module = module_of("#If A Then\nSub X()\n#End If\nSub Y()\nEnd Sub\n");
    assert_eq!(spans(&module), [("X", Sub, 2, 3), ("Y", Sub, 4, 5)]);
    assert_eq!(recovered(&module), [true, false]);
}

#[test]
fn another_form_of_a_header_is_the_same_procedure_whatever_it_declares() {
    let text =
        "#If X Then\nPublic Sub A(p)\n#Else\nPrivate Function B(p, q) As Long\n#End If\nEnd Sub\n";
    let module = module_of(text);
    let [a] = module.procedures.as_slice() else {
        panic!("{module:?}");
    };
    // The name, the kind, the visibility and the parameters are the first form's.
    assert_eq!(
        (a.name.as_str(), a.kind, a.visibility, a.parameter_count),
        ("A", Sub, Visibility::Public, 1)
    );
    assert_eq!((a.start_line, a.end_line, a.lines.clone()), (2, 6, 1..6));
    assert!(!module.is_recovered());
}

#[test]
fn the_end_of_the_conditional_that_holds_a_header_ends_its_other_forms() {
    // The `#Else` leads to no other form: `#End If` came before the next header.
    let text = "#If V Then\nSub A()\n#Else\n#End If\nSub B()\nEnd Sub\n";
    let module = module_of(text);
    assert_eq!(spans(&module), [("A", Sub, 2, 4), ("B", Sub, 5, 6)]);
    assert_eq!(recovered(&module), [true, false]);
}

#[test]
fn a_conditional_that_opens_after_a_header_leads_to_no_other_form_of_it() {
    // The `#Else` belongs to a conditional inside the body of `A`, which has no
    // end: the header after it is `B`, whether `A` is under an `#If` or not.
    let module = module_of("Sub A()\n#If X Then\n#Else\n#End If\nSub B()\nEnd Sub\n");
    assert_eq!(spans(&module), [("A", Sub, 1, 4), ("B", Sub, 5, 6)]);
    assert_eq!(recovered(&module), [true, false]);
    let text = "#If V Then\nSub A()\n#If W Then\n#Else\n#End If\nSub B()\nEnd Sub\n#End If\n";
    let module = module_of(text);
    assert_eq!(spans(&module), [("A", Sub, 2, 5), ("B", Sub, 6, 7)]);
    assert_eq!(recovered(&module), [true, false]);
}

#[test]
fn a_header_that_comes_after_the_forms_of_a_header_is_a_new_procedure() {
    // `S2` is the other form of `S`, so `S3` is a header of its own.
    let module = module_of("#If A Then\nSub S()\n#Else\nSub S2()\nSub S3()\n#End If\nEnd Sub\n");
    assert_eq!(spans(&module), [("S", Sub, 2, 4), ("S3", Sub, 5, 7)]);
    assert_eq!(recovered(&module), [true, false]);
    // The conditional that holds `S2` is closed when `X` comes.
    let text = "#If A Then\nSub S()\n#Else\n#If B Then\nSub S2()\n#Else\n#End If\nSub X()\n#End If\nEnd Sub\n";
    let module = module_of(text);
    assert_eq!(spans(&module), [("S", Sub, 2, 7), ("X", Sub, 8, 10)]);
    assert_eq!(recovered(&module), [true, false]);
}

#[test]
fn a_conditional_nested_in_a_branch_of_a_header_does_not_disturb_its_forms() {
    // The text and the line of the `End Sub`. The first conditional is in the
    // body of the first form and closed before the `#Else`; the second is in the
    // `#Else` branch, and closed before the second form.
    let cases = [
        (
            "#If V Then\nSub A()\n#If W Then\n    x = 1\n#End If\n#Else\nSub A2()\n#End If\nEnd Sub\n",
            9,
        ),
        (
            "#If V Then\nSub A()\n#Else\n#If W Then\n#End If\nSub A2()\n#End If\nEnd Sub\n",
            8,
        ),
    ];
    for (text, end) in cases {
        let module = module_of(text);
        assert_eq!(spans(&module), [("A", Sub, 2, end)], "{text:?}");
        assert!(!module.is_recovered(), "{text:?}");
    }
}

#[test]
fn the_forms_of_a_header_may_be_under_conditionals_nested_in_each_other() {
    // The text, the line of the first form and the line of the `End Sub`.
    let cases = [
        // The second form is a conditional of its own, in the `#Else` branch.
        (
            "#If Mac Then\nSub S()\n#Else\n#If VBA7 Then\nSub S(h As LongPtr)\n#Else\n\
             Sub S(h As Long)\n#End If\n#End If\n    x = 1\nEnd Sub\n",
            2,
            11,
        ),
        // The same with `#ElseIf`.
        (
            "#If Mac Then\nSub S()\n#ElseIf Win Then\n#If VBA7 Then\nSub S(h As LongPtr)\n\
             #ElseIf X Then\nSub S(h As Long)\n#End If\n#End If\n    x = 1\nEnd Sub\n",
            2,
            11,
        ),
        // The first form is under a conditional of its own, in the first branch.
        (
            "#If Mac Then\n#If VBA7 Then\nSub S(h As LongPtr)\n#Else\nSub S(h As Long)\n#End If\n\
             #Else\nSub S()\n#End If\n    x = 1\nEnd Sub\n",
            3,
            11,
        ),
    ];
    for (text, start, end) in cases {
        let module = module_of(text);
        assert_eq!(spans(&module), [("S", Sub, start, end)], "{text:?}");
        assert!(!module.is_recovered(), "{text:?}");
    }
}

#[test]
fn a_second_form_that_names_nothing_is_recovered() {
    let module = module_of("#If A Then\nSub S(a)\n#Else\nSub 123()\n#End If\nEnd Sub\n");
    assert_eq!(spans(&module), [("S", Sub, 2, 6)]);
    assert_eq!(
        module.procedures[0].recoveries,
        [Recovery {
            line: 4,
            kind: RecoveryKind::Unparseable
        }]
    );
    assert!(module.is_recovered());
}

#[test]
fn an_end_that_does_not_end_a_conditional_is_no_directive() {
    // `#End Sub` and a lone `#End` end no `#If`: `B` is still the other form of
    // `A`, which the `#Else` led to.
    let text = "#If V Then\nSub A()\n#Else\n#End Sub\n#End\nSub B()\nEnd Sub\n#End If\n";
    let module = module_of(text);
    assert_eq!(spans(&module), [("A", Sub, 2, 7)]);
    assert!(!module.is_recovered());
}

#[test]
fn directives_that_nothing_opens_are_harmless() {
    let module = module_of("#Else\n#End If\n#ElseIf x Then\nSub A()\nEnd Sub\n#End If\n");
    assert_eq!(spans(&module), [("A", Sub, 4, 5)]);
    assert!(!module.is_recovered());
}

#[test]
fn the_recoveries_of_a_module_are_capped() {
    let module = module_of(&"End Sub\n".repeat(MAX_RECOVERIES * 3));
    assert!(module.procedures.is_empty());
    // The first ones are the ones kept.
    let lines: Vec<_> = module.recoveries.iter().map(|r| r.line).collect();
    assert_eq!(lines, (1u32..).take(MAX_RECOVERIES).collect::<Vec<_>>());
}
