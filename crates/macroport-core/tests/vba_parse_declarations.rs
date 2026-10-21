// SPDX-License-Identifier: Apache-2.0
//! The declarations of the structural parser: the statements outside the
//! procedures of a module that declare or set something.

mod vba_parse_common;

use macroport_core::report::ProcedureKind::Sub;
use macroport_core::vba::{
    DeclarationKind, Directive, OptionKind, ParsedModule, Recovery, RecoveryKind,
};
use vba_parse_common::{module_of, spans};

use DeclarationKind::{Const, Enum, Event, Implements, Type, Variable};

/// The kind and the first physical line of each declaration.
fn kinds(module: &ParsedModule) -> Vec<(DeclarationKind, u32)> {
    let found = module.declarations.iter();
    found.map(|d| (d.kind.clone(), d.line)).collect()
}

/// The first logical line of each declaration, and the one after its last.
fn ranges(module: &ParsedModule) -> Vec<(usize, usize)> {
    let found = module.declarations.iter();
    found.map(|d| (d.lines.start, d.lines.end)).collect()
}

fn declare(name: &str, is_function: bool) -> DeclarationKind {
    DeclarationKind::Declare {
        name: name.to_owned(),
        is_function,
    }
}

fn with_events(name: &str) -> DeclarationKind {
    DeclarationKind::WithEvents {
        name: name.to_owned(),
    }
}

fn unclosed(line: u32) -> Recovery {
    Recovery {
        line,
        kind: RecoveryKind::UnclosedBlock,
    }
}

fn unreadable(line: u32) -> Recovery {
    Recovery {
        line,
        kind: RecoveryKind::Unparseable,
    }
}

#[test]
fn an_option_statement_is_told_by_its_second_word() {
    let text = "Option Explicit\nOption Base 1\nOption Compare Text\nOption Private Module\n\
                Option VBASupport 1\nOption Foo\nOption\nOPTION explicit\n";
    let module = module_of(text);
    let option = DeclarationKind::Option;
    assert_eq!(
        kinds(&module),
        [
            (option(OptionKind::Explicit), 1),
            (option(OptionKind::Base), 2),
            (option(OptionKind::Compare), 3),
            (option(OptionKind::PrivateModule), 4),
            (option(OptionKind::VbaSupport), 5),
            (option(OptionKind::Other), 6),
            (option(OptionKind::Other), 7),
            (option(OptionKind::Explicit), 8),
        ]
    );
    assert!(module.procedures.is_empty() && module.recoveries.is_empty());
}

#[test]
fn a_declare_gives_the_name_of_the_module_and_not_the_alias() {
    let text = "Declare Function GetTick Lib \"kernel32\" Alias \"GetTickCount\" () As Long\n\
                Private Declare PtrSafe Sub Sleep Lib \"kernel32\" (ByVal ms As Long)\n\
                Public Declare PtrSafe Function [Odd Name] Lib \"x\" () As Long\n\
                Declare Function Wrapped Lib \"x\" ( _\n    ByVal a As Long) As Long\n\
                Declare\nDeclare Neither Lib \"x\" ()\n";
    let module = module_of(text);
    assert_eq!(
        kinds(&module),
        [
            (declare("GetTick", true), 1),
            (declare("Sleep", false), 2),
            (declare("Odd Name", true), 3),
            (declare("Wrapped", true), 4),
            (declare("", false), 6),
            (declare("", false), 7),
        ]
    );
}

#[test]
fn constants_and_variables_are_one_declaration_for_each_statement() {
    let text = "Const A = 1\nPublic Const B = 2, C = 3\nPrivate Const D As String = \"x\"\n\
                Dim a\nPublic b As Long\nPrivate c(1 To 3) As Long\nGlobal d\n\
                Public e, f As Long\nDim [Odd Name] As Long\n";
    let module = module_of(text);
    let expected: Vec<_> = (1..=9)
        .map(|line| (if line <= 3 { Const } else { Variable }, line))
        .collect();
    assert_eq!(kinds(&module), expected);
}

#[test]
fn a_variable_declared_with_events_gives_its_name() {
    let text = "Private WithEvents Emitter As EventListenerEmitter\n\
                Public WithEvents [Odd] As Foo\nDim WithEvents app As Application\n\
                Private WithEvents\n";
    let module = module_of(text);
    assert_eq!(
        kinds(&module),
        [
            (with_events("Emitter"), 1),
            (with_events("Odd"), 2),
            (with_events("app"), 3),
            (with_events(""), 4),
        ]
    );
}

#[test]
fn a_type_or_an_enum_runs_to_its_end_and_its_members_declare_nothing() {
    let text = "Private Type Point\n    X As Long\n    Y As Long\nEnd Type\n\
                Public Enum Color\n    Red = 1\n    Green\nEnd Enum\nDim after As Long\n";
    let module = module_of(text);
    assert_eq!(kinds(&module), [(Type, 1), (Enum, 5), (Variable, 9)]);
    assert_eq!(ranges(&module), [(0, 4), (4, 8), (8, 9)]);
    assert!(module.recoveries.is_empty());
}

#[test]
fn the_lines_of_a_block_that_declare_nothing_are_its_members_whatever_they_look_like() {
    // None of these is the header of a procedure, and none declares anything.
    let text = "Type T\n    Public As Long\n    x = 1\n    Static s\n    End Sub\n    #If X Then\n\
                End Type\nEnum E\n    End Type\n    A\nEnd Enum\n";
    let module = module_of(text);
    assert_eq!(kinds(&module), [(Type, 1), (Enum, 8)]);
    assert_eq!(ranges(&module), [(0, 7), (7, 11)]);
    assert!(module.recoveries.is_empty());
}

#[test]
fn a_member_that_reads_word_as_is_a_member_whatever_the_word() {
    // Excel's own export of the corpus sample 13-06 has `Type As Long` as a
    // member of a `Type` (lines 100 and 136 of its `.expected.cls` files). Each
    // of these would declare something, or open a procedure, anywhere else.
    let text = "Type T\n    Type As Long\n    Event As Long\n    Declare As Long\n\
                Const As Long\n    Sub As Long\n    Function As Long\nEnd Type\n\
                Sub S()\nEnd Sub\n";
    let module = module_of(text);
    assert_eq!(kinds(&module), [(Type, 1)]);
    assert_eq!(ranges(&module), [(0, 8)]);
    assert_eq!(spans(&module), [("S", Sub, 9, 10)]);
    assert!(!module.is_recovered());
}

#[test]
fn a_member_that_is_an_array_reads_word_and_its_subscripts_as_and_is_a_member_too() {
    // The subscripts come between the name and the `As`, and may nest.
    let text = "Type T\n    Type(0 To 3) As Byte\n    Event(0 To 1) As Byte\n\
                Declare(1) As Integer\n    Const(1) As Long\n    Sub(1) As Long\n\
                Function(1 To 2) As Long\n    Option((1)) As Long\n    Size As Long\n\
                End Type\nPrivate Const K = 1\n";
    let module = module_of(text);
    assert_eq!(kinds(&module), [(Type, 1), (Const, 11)]);
    assert_eq!(ranges(&module), [(0, 10), (10, 11)]);
    assert!(!module.is_recovered());
}

#[test]
fn an_array_member_stays_in_a_block_with_no_end_and_the_next_statement_still_ends_it() {
    let module =
        module_of("Type T\n    Type(0 To 3) As Byte\n    Function(1) As Long\nConst K = 1\n");
    assert_eq!(kinds(&module), [(Type, 1), (Const, 4)]);
    assert_eq!(ranges(&module), [(0, 3), (3, 4)]);
    assert_eq!(module.recoveries, [unclosed(1)]);
}

#[test]
fn parentheses_after_the_first_word_make_a_member_only_when_an_as_follows_them() {
    // Each of these ends a block with no end, and is read as what it is.
    let statements = [
        ("Const (1) = 2", Const),
        ("Event (1)", Event),
        ("Declare (1) Lib \"x\"", declare("", false)),
        ("Option (1) Foo", DeclarationKind::Option(OptionKind::Other)),
    ];
    for (statement, kind) in statements {
        let module = module_of(&format!("Type T\n    a As Long\n{statement}\n"));
        assert_eq!(kinds(&module), [(Type, 1), (kind, 3)], "{statement}");
        assert_eq!(module.recoveries, [unclosed(1)], "{statement}");
    }
    // The subscripts are never closed: the line opens a block of its own, and
    // is a statement that cannot be read.
    let module = module_of("Type T\n    a As Long\nType(0 To 3 As Byte\n");
    assert_eq!(kinds(&module), [(Type, 1), (Type, 3)]);
    assert_eq!(module.recoveries, [unclosed(1), unreadable(3), unclosed(3)]);
    // The header of a procedure that names nothing is one.
    let module = module_of("Type T\n    a As Long\nSub (a, b)\nEnd Sub\n");
    assert_eq!(kinds(&module), [(Type, 1)]);
    assert_eq!(spans(&module), [("", Sub, 3, 4)]);
    assert_eq!(module.recoveries, [unclosed(1)]);
}

#[test]
fn a_type_with_no_end_is_closed_by_the_next_procedure() {
    let module = module_of("Type T\n    a As Long\nSub S()\nEnd Sub\n");
    assert_eq!(kinds(&module), [(Type, 1)]);
    assert_eq!(ranges(&module), [(0, 2)]);
    assert_eq!(spans(&module), [("S", Sub, 3, 4)]);
    assert_eq!(module.recoveries, [unclosed(1)]);
    assert!(module.is_recovered());
    assert!(!module.procedures[0].is_recovered());
}

#[test]
fn a_type_or_an_enum_with_no_end_is_ended_by_each_other_statement_that_declares_something() {
    // A line that no `Type` or `Enum` can hold is read as what it is, and the
    // block stops at the line before it. A `Type` or an `Enum` is the case of
    // `a_block_opened_inside_a_block_with_no_end_ends_it`, and a directive is a
    // member, not a statement that ends a block.
    let statements = [
        (
            "Option Explicit",
            DeclarationKind::Option(OptionKind::Explicit),
        ),
        (
            "Private Declare Function F Lib \"x\" ()",
            declare("F", true),
        ),
        ("Public Const K = 1", Const),
        ("Dim v", Variable),
        ("Private WithEvents w As Foo", with_events("w")),
        ("Implements I", Implements),
        ("Public Event Ev()", Event),
    ];
    for (opener, block) in [("Type T", Type), ("Enum E", Enum)] {
        for (statement, kind) in &statements {
            let module = module_of(&format!("{opener}\n    a\n{statement}\n"));
            let why = format!("{opener}, {statement}");
            assert_eq!(
                kinds(&module),
                [(block.clone(), 1), (kind.clone(), 3)],
                "{why}"
            );
            assert_eq!(ranges(&module), [(0, 2), (2, 3)], "{why}");
            assert_eq!(module.recoveries, [unclosed(1)], "{why}");
        }
    }
}

#[test]
fn a_block_with_no_end_keeps_the_members_that_read_word_as() {
    let module = module_of("Type T\n    Type As Long\n    Const As Long\nConst K = 1\n");
    assert_eq!(kinds(&module), [(Type, 1), (Const, 4)]);
    assert_eq!(ranges(&module), [(0, 3), (3, 4)]);
    assert_eq!(module.recoveries, [unclosed(1)]);
}

#[test]
fn a_block_opened_inside_a_block_with_no_end_ends_it() {
    let module =
        module_of("Type T\n    a As Long\nType U\n    b As Long\nEnd Type\nEnum E\nEnum F\n");
    assert_eq!(kinds(&module), [(Type, 1), (Type, 3), (Enum, 6), (Enum, 7)]);
    assert_eq!(ranges(&module), [(0, 2), (2, 5), (5, 6), (6, 7)]);
    assert_eq!(module.recoveries, [unclosed(1), unclosed(6), unclosed(7)]);
}

#[test]
fn an_enum_with_no_end_runs_to_the_end_of_the_module() {
    // The `End Type` does not end an `Enum`, and what follows is taken as
    // members: the block is closed where the module stops.
    let module = module_of("Enum E\n    A\nEnd Type\n    B\n");
    assert_eq!(kinds(&module), [(Enum, 1)]);
    assert_eq!(ranges(&module), [(0, 4)]);
    assert_eq!(module.recoveries, [unclosed(1)]);
}

#[test]
fn a_conditional_inside_a_block_is_a_member_and_leaves_the_if_blocks_balanced() {
    let text = "Type T\n#If VBA7 Then\n    h As LongPtr\n#Else\n    h As Long\n#End If\nEnd Type\n\
                #If X Then\nSub S()\n#Else\nSub S()\n#End If\nEnd Sub\n";
    let module = module_of(text);
    assert_eq!(
        kinds(&module),
        [(Type, 1), (DeclarationKind::Directive(Directive::If), 8)]
    );
    assert_eq!(ranges(&module), [(0, 7), (7, 8)]);
    // `S` has two forms: the `#Else` was told to be its own.
    assert_eq!(spans(&module), [("S", Sub, 9, 13)]);
    assert!(!module.is_recovered());
}

#[test]
fn the_forms_of_the_opener_of_a_block_share_its_end() {
    let forms = [
        (
            "Private Type T",
            "a As LongPtr",
            "a As Long",
            "End Type",
            Type,
        ),
        ("Public Enum E", "A", "B", "End Enum", Enum),
    ];
    for (opener, first, second, end, block) in forms {
        let text = format!(
            "#If VBA7 Then\n{opener}\n    {first}\n#Else\n{opener}\n    {second}\n#End If\n\
             {end}\nDeclare Function F Lib \"k\" () As Long\n"
        );
        let module = module_of(&text);
        assert_eq!(
            kinds(&module),
            [
                (DeclarationKind::Directive(Directive::If), 1),
                (block, 2),
                (declare("F", true), 9)
            ],
            "{opener}"
        );
        assert_eq!(ranges(&module), [(0, 1), (1, 8), (8, 9)], "{opener}");
        assert!(!module.is_recovered(), "{opener}");
    }
}

#[test]
fn a_block_can_have_more_than_two_forms_of_its_opener() {
    let text = "#If A Then\nType T\n#ElseIf B Then\nType T\n#Else\nType T\n#End If\nEnd Type\n\
                Sub S()\nEnd Sub\n";
    let module = module_of(text);
    assert_eq!(
        kinds(&module),
        [(DeclarationKind::Directive(Directive::If), 1), (Type, 2)]
    );
    assert_eq!(ranges(&module), [(0, 1), (1, 8)]);
    assert_eq!(spans(&module), [("S", Sub, 9, 10)]);
    assert!(!module.is_recovered());
    // The `#If` of a form stands in the branch of another.
    let text = "#If A Then\nType T\n#Else\n#If B Then\nType T\n#Else\nType T\n#End If\n\
                #End If\nEnd Type\n";
    let module = module_of(text);
    assert_eq!(
        kinds(&module),
        [(DeclarationKind::Directive(Directive::If), 1), (Type, 2)]
    );
    assert_eq!(ranges(&module), [(0, 1), (1, 10)]);
    assert!(!module.is_recovered());
}

#[test]
fn the_forms_of_an_opener_that_each_have_their_end_are_blocks_of_their_own() {
    let text = "#If VBA7 Then\nPrivate Type T\n    a As LongPtr\nEnd Type\n#Else\n\
                Private Type T\n    a As Long\nEnd Type\n#End If\n";
    let module = module_of(text);
    assert_eq!(
        kinds(&module),
        [
            (DeclarationKind::Directive(Directive::If), 1),
            (Type, 2),
            (DeclarationKind::Directive(Directive::Else), 5),
            (Type, 6),
            (DeclarationKind::Directive(Directive::EndIf), 9),
        ]
    );
    assert_eq!(ranges(&module), [(0, 1), (1, 4), (4, 5), (5, 8), (8, 9)]);
    assert!(!module.is_recovered());
}

#[test]
fn the_forms_of_the_opener_of_a_block_with_no_end_are_one_block_and_one_recovery() {
    let text = "#If VBA7 Then\nPrivate Type T\n    a As LongPtr\n#Else\nPrivate Type T\n\
                    a As Long\n#End If\nDeclare Function F Lib \"k\" () As Long\n";
    let module = module_of(text);
    assert_eq!(
        kinds(&module),
        [
            (DeclarationKind::Directive(Directive::If), 1),
            (Type, 2),
            (declare("F", true), 8)
        ]
    );
    assert_eq!(ranges(&module), [(0, 1), (1, 7), (7, 8)]);
    assert_eq!(module.recoveries, [unclosed(2)]);
}

#[test]
fn a_conditional_inside_the_forms_of_an_opener_does_not_tell_another_one() {
    // The `#Else` of the inner `#If` is not the one that holds the opener.
    let text = "#If A Then\nType T\n#If B Then\n    x As Long\n#Else\n    y As Long\n#End If\n\
                #Else\nType T\n#End If\nEnd Type\n";
    let module = module_of(text);
    assert_eq!(
        kinds(&module),
        [(DeclarationKind::Directive(Directive::If), 1), (Type, 2)]
    );
    assert_eq!(ranges(&module), [(0, 1), (1, 11)]);
    assert!(!module.is_recovered());
}

#[test]
fn an_opener_that_is_not_another_form_of_the_one_before_it_ends_the_block() {
    let directive = DeclarationKind::Directive;
    let if_ = directive(Directive::If);
    // The text, the declarations with their first physical line, the range of
    // each, and the places where a block has no end.
    let cases = [
        // After the `#End If`, all the forms are read.
        (
            "#If X Then\nType T\n#Else\nType T\n#End If\nType U\nEnd Type\n",
            vec![(if_.clone(), 1), (Type, 2), (Type, 6)],
            vec![(0, 1), (1, 5), (5, 7)],
            vec![2],
        ),
        // Right after a form, when the `#If` has no more branches.
        (
            "#If X Then\nType T\n#Else\nType T\nType U\n#End If\nEnd Type\n",
            vec![(if_.clone(), 1), (Type, 2), (Type, 5)],
            vec![(0, 1), (1, 4), (4, 7)],
            vec![2],
        ),
        // Before the `#Else`, nothing says it is one.
        (
            "#If X Then\nType T\nType U\n#Else\n#End If\nEnd Type\n",
            vec![(if_.clone(), 1), (Type, 2), (Type, 3)],
            vec![(0, 1), (1, 2), (2, 6)],
            vec![2],
        ),
        // The `#If` of the `#Else` does not hold the opener.
        (
            "Type T\n#If X Then\n    a As Long\n#Else\nType U\n#End If\nEnd Type\n",
            vec![(Type, 1), (Type, 5)],
            vec![(0, 4), (4, 7)],
            vec![1],
        ),
        // Nor does one inside the `#If` that holds it.
        (
            "#If A Then\nType T\n#If B Then\n    a As Long\n#Else\nType U\n#End If\n\
             #End If\nEnd Type\n",
            vec![(if_.clone(), 1), (Type, 2), (Type, 6)],
            vec![(0, 1), (1, 5), (5, 9)],
            vec![2],
        ),
        // An `Enum` is not another form of a `Type`, and the `End Type` that
        // follows does not end it.
        (
            "#If X Then\nType T\n#Else\nEnum E\n#End If\nEnd Type\n",
            vec![(if_, 1), (Type, 2), (Enum, 4)],
            vec![(0, 1), (1, 3), (3, 6)],
            vec![2, 4],
        ),
    ];
    for (text, expected_kinds, expected_ranges, unclosed_at) in cases {
        let module = module_of(text);
        assert_eq!(kinds(&module), expected_kinds, "{text:?}");
        assert_eq!(ranges(&module), expected_ranges, "{text:?}");
        let expected: Vec<_> = unclosed_at.into_iter().map(unclosed).collect();
        assert_eq!(module.recoveries, expected, "{text:?}");
    }
}

#[test]
fn the_conditionals_of_a_block_with_no_end_still_tell_the_forms_of_the_procedure_after_it() {
    let text = "Enum E\n    A\n#If VBA7 Then\nPublic Sub S(h As LongPtr)\n#Else\n\
                Public Sub S(h As Long)\n#End If\nEnd Sub\n";
    let module = module_of(text);
    assert_eq!(kinds(&module), [(Enum, 1)]);
    assert_eq!(ranges(&module), [(0, 3)]);
    assert_eq!(spans(&module), [("S", Sub, 4, 8)]);
    assert!(!module.procedures[0].is_recovered());
    assert_eq!(module.recoveries, [unclosed(1)]);
}

#[test]
fn implements_and_events_are_declarations() {
    let text = "Implements IFoo\nPublic Event Changed(ByVal x As Long)\nEvent Done()\n";
    let module = module_of(text);
    assert_eq!(kinds(&module), [(Implements, 1), (Event, 2), (Event, 3)]);
}

#[test]
fn directives_outside_a_procedure_are_declarations_and_the_ones_inside_are_not() {
    let text = "#Const DEBUGGING = 1\n#If VBA7 Then\nDeclare PtrSafe Sub S Lib \"k\" ()\n\
                #ElseIf X Then\n#Else\nDeclare Sub S Lib \"k\" ()\n#End If\n\
                Sub A()\n#If X Then\n#End If\nEnd Sub\n";
    let module = module_of(text);
    let directive = DeclarationKind::Directive;
    assert_eq!(
        kinds(&module),
        [
            (directive(Directive::Const), 1),
            (directive(Directive::If), 2),
            (declare("S", false), 3),
            (directive(Directive::ElseIf), 4),
            (directive(Directive::Else), 5),
            (declare("S", false), 6),
            (directive(Directive::EndIf), 7),
        ]
    );
}

#[test]
fn the_directives_between_the_forms_of_a_procedure_are_lines_of_the_procedure() {
    let text = "#If VBA7 Then\nPublic Sub S(h As LongPtr)\n#Else\nPublic Sub S(h As Long)\n\
                #End If\nEnd Sub\n";
    let module = module_of(text);
    assert_eq!(
        kinds(&module),
        [(DeclarationKind::Directive(Directive::If), 1)]
    );
    assert_eq!(spans(&module), [("S", Sub, 2, 6)]);
}

#[test]
fn statements_outside_a_procedure_that_declare_nothing_are_left_alone() {
    let text = "x = 1\nSet a = b\nWith c\n    .d = 1\nEnd With\n\
                Worksheets(\"x\").Range(\"A1\").Value = 1\nCall Foo\nPublic\nPublic 1\n\
                Static s\nEnd\nEnd Type\nLabel:\n";
    let module = module_of(text);
    assert!(module.declarations.is_empty(), "{:?}", module.declarations);
    assert!(module.recoveries.is_empty());
}

#[test]
fn the_declarations_are_listed_around_the_procedures_and_none_is_found_inside_one() {
    let text = "Option Explicit\nSub A()\n    Dim x As Long\n    Const Y = 1\n    Option Base 1\n\
                End Sub\nPublic z As Long\nSub B()\nEnd Sub\nPrivate Const W = 2\n";
    let module = module_of(text);
    assert_eq!(
        kinds(&module),
        [
            (DeclarationKind::Option(OptionKind::Explicit), 1),
            (Variable, 7),
            (Const, 10),
        ]
    );
    assert_eq!(spans(&module), [("A", Sub, 2, 6), ("B", Sub, 8, 9)]);
}

#[test]
fn a_declaration_starts_at_its_first_physical_line() {
    let module = module_of("Public _\n    Const A = 1, _\n    B = 2\nDim x\n");
    assert_eq!(kinds(&module), [(Const, 1), (Variable, 4)]);
    assert_eq!(ranges(&module), [(0, 1), (1, 2)]);
}
