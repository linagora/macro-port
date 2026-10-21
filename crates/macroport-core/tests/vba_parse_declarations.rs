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
    // Excel exports `Type As Long` as a member of a `Type` (corpus sample 13-06,
    // line 105). Each of these would declare something, or open a procedure,
    // anywhere else.
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
fn a_block_with_no_end_is_closed_by_a_statement_that_declares_something() {
    // A line that no `Type` or `Enum` can hold is read as what it is, and the
    // block stops at the line before it.
    let text = "Type T\n    a As Long\nPrivate Declare Function F Lib \"x\" ()\nPublic Const K = 1\n\
                Dim v\nEnum E\n    A\nOption Explicit\nImplements I\nEvent Ev()\nType U\n\
                End Type\n";
    let module = module_of(text);
    assert_eq!(
        kinds(&module),
        [
            (Type, 1),
            (declare("F", true), 3),
            (Const, 4),
            (Variable, 5),
            (Enum, 6),
            (DeclarationKind::Option(OptionKind::Explicit), 8),
            (Implements, 9),
            (Event, 10),
            (Type, 11),
        ]
    );
    let expected = [
        (0, 2),
        (2, 3),
        (3, 4),
        (4, 5),
        (5, 7),
        (7, 8),
        (8, 9),
        (9, 10),
        (10, 12),
    ];
    assert_eq!(ranges(&module), expected);
    assert_eq!(module.recoveries, [unclosed(1), unclosed(6)]);
}

#[test]
fn every_kind_of_declaration_ends_a_type_or_an_enum_with_no_end() {
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
