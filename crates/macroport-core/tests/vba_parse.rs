// SPDX-License-Identifier: Apache-2.0
//! The procedures of the structural parser: their headers, their ends and
//! their spans.

mod vba_parse_common;

use macroport_core::report::{ProcedureKind, Visibility};
use macroport_core::vba::{MAX_RECOVERIES, Recovery, RecoveryKind};
use vba_parse_common::{module_of, spans};

use ProcedureKind::{Function, PropertyGet, PropertyLet, PropertySet, Sub};

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
fn the_parameters_of_a_header_are_counted() {
    let text = "Sub A()\nEnd Sub\nSub B\nEnd Sub\nSub C(x)\nEnd Sub\n\
                Function D(ByVal a As Long, Optional b As String = \"x,y\", ParamArray c() As Variant) As Long\n\
                End Function\nSub E(a As Long, _\n    b As Long)\nEnd Sub\n\
                Property Let F(Optional ByVal i As Long = -1, v)\nEnd Property\n\
                Sub G(a() As Long, ByRef b() As String, )\nEnd Sub\nSub (a, b)\nEnd Sub\n\
                Sub H(a, b\nEnd Sub\n";
    let module = module_of(text);
    let counts: Vec<_> = module
        .procedures
        .iter()
        .map(|p| p.parameter_count)
        .collect();
    assert_eq!(counts, [0, 0, 1, 3, 2, 2, 2, 0, 2]);
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
    assert_eq!(a.recoveries.len(), 1);
    assert_eq!(a.recoveries[0].kind, RecoveryKind::MissingEnd);
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
    let lines: Vec<_> = module
        .procedures
        .iter()
        .map(|p| p.recoveries.clone())
        .collect();
    for (recoveries, line) in lines.iter().zip([2, 4, 6]) {
        assert_eq!(
            recoveries,
            &[Recovery {
                line,
                kind: RecoveryKind::WrongEnd
            }]
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
fn statements_that_look_like_headers_are_not_headers() {
    let text = "Declare Function F Lib \"x\" () As Long\nPublic Declare PtrSafe Sub S Lib \"y\" ()\n\
                Public WS As Worksheet\nProperty\nSub A()\n    Exit Sub\n    x = \"Sub B()\"\n    \
                ' Sub C()\n    End\nEnd Sub\n";
    let module = module_of(text);
    assert_eq!(spans(&module), [("A", Sub, 5, 10)]);
    assert!(module.recoveries.is_empty() && !module.procedures[0].is_recovered());
}

#[test]
fn the_recoveries_of_a_module_are_capped() {
    let text = "End Sub\n".repeat(MAX_RECOVERIES * 3);
    let module = module_of(&text);
    assert!(module.procedures.is_empty());
    assert_eq!(module.recoveries.len(), MAX_RECOVERIES);
    assert_eq!(module.recoveries[0].line, 1);
}
