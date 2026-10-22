// SPDX-License-Identifier: Apache-2.0
//! Tests of the `procedure_name`, `module_type` and `reference` matchers, which
//! read the procedures, the modules and the references of the project rather
//! than its statements. The regexes of these tests are case-sensitive unless
//! they say `(?i)`: the rules loader makes the real ones case-insensitive.

use crate::engine::testing::{
    line, module, places, procedure, procedure_names, project, project_of, references, snippets,
};
use crate::engine::{Evaluator, ProjectInput};
use crate::rules::{Matcher, ModuleType, TextMode};

#[test]
fn procedure_name_matches_the_name_and_cites_the_declaration() {
    let project = project(vec![
        module(
            "Sheet1",
            ModuleType::Document,
            vec![
                procedure("Worksheet_Change", 3, vec![line(4, "x = 1")]),
                procedure("Helper", 10, vec![]),
                procedure("WORKSHEET_Activate", 20, vec![]),
            ],
        )
        .with_declarations(vec![line(1, "Option Explicit")]),
        module(
            "Module1",
            ModuleType::Standard,
            vec![procedure("Worksheet_Calculate", 2, vec![])],
        ),
    ]);
    let matcher = procedure_names(&[r"(?i)^worksheet_\w+$"]);
    let evaluator = Evaluator::new(&project);
    let text = TextMode::Code;
    let hits = evaluator.procedure_hits(0, 0, &matcher, text);
    assert_eq!(places(&hits), [(0, 3)]);
    assert_eq!(snippets(&hits), ["Sub Worksheet_Change()"]);
    assert!(evaluator.procedure_hits(0, 1, &matcher, text).is_empty());
    // A wider scope finds each procedure.
    let hits = evaluator.module_hits(0, &matcher, text);
    assert_eq!(places(&hits), [(0, 3), (0, 20)]);
    let hits = evaluator.project_hits(&matcher, text);
    assert_eq!(places(&hits), [(0, 3), (0, 20), (1, 2)]);
}

#[test]
fn procedure_name_finds_procedures_and_never_the_declarations() {
    let project = project(vec![
        module(
            "Module1",
            ModuleType::Standard,
            vec![procedure("A", 3, vec![]), procedure("B", 6, vec![])],
        )
        .with_declarations(vec![line(1, "Option Explicit"), line(8, "Dim x")]),
    ]);
    // `.*` matches any name, even none: the declarations are not offered to it.
    let matcher = procedure_names(&[".*"]);
    let hits = Evaluator::new(&project).module_hits(0, &matcher, TextMode::Code);
    assert_eq!(places(&hits), [(0, 3), (0, 6)]);
}

#[test]
fn procedure_name_matches_any_of_its_patterns() {
    let procedures = vec![
        procedure("Auto_Open", 2, vec![]),
        procedure("Helper", 5, vec![]),
        procedure("Auto_Close", 8, vec![]),
    ];
    let project = project(vec![module("Module1", ModuleType::Standard, procedures)]);
    let evaluator = Evaluator::new(&project);
    let text = TextMode::Code;
    // Each pattern matches a different procedure.
    let matcher = procedure_names(&["^Auto_Open$", "^Auto_Close$"]);
    let hits = evaluator.project_hits(&matcher, text);
    assert_eq!(places(&hits), [(0, 2), (0, 8)]);
    // A procedure that several patterns match is one hit.
    let matcher = procedure_names(&["^Auto_", "_Open$"]);
    let hits = evaluator.project_hits(&matcher, text);
    assert_eq!(places(&hits), [(0, 2), (0, 8)]);
}

#[test]
fn procedure_name_ignores_the_type_character() {
    // The parser gives the name as written, type character included.
    let procedures = vec![procedure("Total$", 1, vec![])];
    let project = project(vec![module("Module1", ModuleType::Standard, procedures)]);
    let evaluator = Evaluator::new(&project);
    let text = TextMode::Code;
    let hits = evaluator.project_hits(&procedure_names(&["^Total$"]), text);
    assert_eq!(places(&hits), [(0, 1)]);
    // The regex does not see the type character.
    let hits = evaluator.project_hits(&procedure_names(&[r"\$"]), text);
    assert!(hits.is_empty());
}

#[test]
fn procedure_name_quotes_the_name_when_the_parser_gave_no_line() {
    let mut main = procedure("Main", 4, vec![]);
    main.lines.clear();
    let project = project(vec![module("Module1", ModuleType::Standard, vec![main])]);
    let matcher = procedure_names(&["^Main$"]);
    let hits = Evaluator::new(&project).procedure_hits(0, 0, &matcher, TextMode::Code);
    assert_eq!(places(&hits), [(0, 4)]);
    assert_eq!(snippets(&hits), ["Main"]);
}

/// A standard module, a form with declarations and two procedures, a class,
/// and a form with nothing in it.
fn modules_of_each_type() -> ProjectInput {
    project(vec![
        module(
            "Module1",
            ModuleType::Standard,
            vec![procedure("A", 2, vec![])],
        ),
        module(
            "UserForm1",
            ModuleType::Form,
            vec![procedure("B", 3, vec![]), procedure("C", 6, vec![])],
        )
        .with_declarations(vec![line(1, "Option Explicit")]),
        module("Class1", ModuleType::Class, vec![procedure("D", 2, vec![])]),
        module("UserForm2", ModuleType::Form, vec![]),
    ])
}

#[test]
fn module_type_gives_one_hit_per_module_whatever_covers_it() {
    let project = modules_of_each_type();
    let evaluator = Evaluator::new(&project);
    let text = TextMode::Code;
    let forms = Matcher::ModuleType(vec![ModuleType::Form]);
    // The declarations and two procedures cover the form: one hit, at its first
    // line, which quotes its name.
    let hits = evaluator.module_hits(1, &forms, text);
    assert_eq!(places(&hits), [(1, 1)]);
    assert_eq!(snippets(&hits), ["UserForm1"]);
    // A module of another type has none.
    assert!(evaluator.module_hits(0, &forms, text).is_empty());
}

#[test]
fn module_type_over_a_project_finds_each_module_of_the_types() {
    let project = modules_of_each_type();
    let evaluator = Evaluator::new(&project);
    let text = TextMode::Code;
    // A module with no procedure is found as well.
    let forms = Matcher::ModuleType(vec![ModuleType::Form]);
    let hits = evaluator.project_hits(&forms, text);
    assert_eq!(places(&hits), [(1, 1), (3, 1)]);
    // Any of the types counts.
    let several = Matcher::ModuleType(vec![ModuleType::Form, ModuleType::Class]);
    let hits = evaluator.project_hits(&several, text);
    assert_eq!(places(&hits), [(1, 1), (2, 1), (3, 1)]);
}

#[test]
fn module_type_over_a_procedure_cites_its_declaration() {
    let project = modules_of_each_type();
    let evaluator = Evaluator::new(&project);
    let text = TextMode::Code;
    let forms = Matcher::ModuleType(vec![ModuleType::Form]);
    // The hit is in the procedure, not at the first line of the module.
    let hits = evaluator.procedure_hits(1, 1, &forms, text);
    assert_eq!(places(&hits), [(1, 6)]);
    assert_eq!(snippets(&hits), ["Sub C()"]);
    assert!(evaluator.procedure_hits(0, 0, &forms, text).is_empty());
}

#[test]
fn reference_matches_the_display_names_of_the_project_at_any_scope() {
    let project = project_of(&[line(2, "x = 1")]).with_references(&[
        "Visual Basic For Applications",
        "Microsoft Excel 16.0 Object Library",
        "Microsoft Outlook 16.0 Object Library",
        "Microsoft Word 16.0 Object Library",
    ]);
    let matcher = references(&["Microsoft (Outlook|Word) [0-9.]+ Object Library"]);
    let evaluator = Evaluator::new(&project);
    let text = TextMode::Code;
    let all = [
        evaluator.procedure_hits(0, 0, &matcher, text),
        evaluator.module_hits(0, &matcher, text),
        evaluator.project_hits(&matcher, text),
    ];
    for hits in all {
        // A reference is not in the source of a module: no hit has a place.
        assert!(places(&hits).is_empty());
        assert_eq!(
            snippets(&hits),
            [
                "Microsoft Outlook 16.0 Object Library",
                "Microsoft Word 16.0 Object Library"
            ]
        );
    }
}

#[test]
fn reference_matches_any_of_its_patterns() {
    let project = project_of(&[]).with_references(&[
        "Visual Basic For Applications",
        "Microsoft ActiveX Data Objects 6.1 Library",
        "Microsoft WinHTTP Services, version 5.1",
    ]);
    let evaluator = Evaluator::new(&project);
    let text = TextMode::Code;
    // Each pattern matches a different reference.
    let matcher = references(&["ActiveX Data Objects", "WinHTTP"]);
    let hits = evaluator.project_hits(&matcher, text);
    assert_eq!(
        snippets(&hits),
        [
            "Microsoft ActiveX Data Objects 6.1 Library",
            "Microsoft WinHTTP Services, version 5.1"
        ]
    );
    // A reference that several patterns match is one hit.
    let matcher = references(&["Data", "Objects"]);
    let hits = evaluator.project_hits(&matcher, text);
    assert_eq!(
        snippets(&hits),
        ["Microsoft ActiveX Data Objects 6.1 Library"]
    );
}
