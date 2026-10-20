// SPDX-License-Identifier: Apache-2.0
//! Tests of the structural predicates.

use crate::engine::testing::{lines_of, module, places, procedure, project, snippets, structurals};
use crate::engine::{
    Evaluator, Hit, ModuleInput, NameSet, ProjectInput, is_auto_exec, is_event_handler,
};
use crate::rules::{ModuleType, StructuralPredicate, TextMode};

/// The hits of these predicates over the whole project.
fn project_hits<'p>(project: &'p ProjectInput, predicates: &[StructuralPredicate]) -> Vec<Hit<'p>> {
    let matcher = structurals(predicates);
    Evaluator::new(project).project_hits(&matcher, TextMode::Code)
}

/// A module of this type with an empty `Sub` for each name of the table, the
/// first on line 10, the next on line 20, and so on.
fn module_of(module_type: ModuleType, table: &[(&str, bool)]) -> ModuleInput {
    let starts = (10u32..).step_by(10);
    let procedures = table
        .iter()
        .zip(starts)
        .map(|((name, _), start)| procedure(name, start, Vec::new()))
        .collect();
    module("Module1", module_type, procedures)
}

/// The names of the procedures of `module` at whose declaration `predicate`
/// holds.
fn holding(module: ModuleInput, predicate: StructuralPredicate) -> Vec<String> {
    let declarations: Vec<(u32, String)> = module
        .procedures
        .iter()
        .map(|procedure| (procedure.start_line, procedure.name.clone()))
        .collect();
    let project = project(vec![module]);
    let hits = project_hits(&project, &[predicate]);
    lines_of(&hits)
        .into_iter()
        .flat_map(|cited| {
            declarations
                .iter()
                .filter(move |(start, _)| *start == cited)
                .map(|(_, name)| name.clone())
        })
        .collect()
}

/// Checks that `predicate` holds in `module` for the procedures of the table
/// that are marked `true`, and for no other.
fn assert_holds_for(module: ModuleInput, predicate: StructuralPredicate, table: &[(&str, bool)]) {
    let expected: Vec<String> = table
        .iter()
        .filter(|(_, held)| *held)
        .map(|(name, _)| (*name).to_owned())
        .collect();
    assert_eq!(holding(module, predicate), expected);
}

#[test]
fn auto_exec_matches_the_auto_macros_in_any_module() {
    let table = [
        ("Auto_Open", true),
        ("AUTO_CLOSE", true),
        ("AutoOpen", true),
        ("autoclose", true),
        ("AutoExec", true),
        ("AutoExec$", true),
        ("Auto_Open2", false),
        ("AutoOpener", false),
        ("Helper", false),
    ];
    for module_type in ModuleType::ALL {
        let module = module_of(module_type, &table);
        assert_holds_for(module, StructuralPredicate::AutoExec, &table);
    }
}

#[test]
fn auto_exec_matches_the_workbook_events_in_a_document_module_only() {
    let events = [
        ("Workbook_Open", true),
        ("workbook_activate", true),
        ("Workbook_BeforeClose", true),
        ("Workbook_BeforeSave", false),
        ("Worksheet_Activate", false),
    ];
    for module_type in ModuleType::ALL {
        let in_document = module_type == ModuleType::Document;
        let table = events.map(|(name, event)| (name, event && in_document));
        let module = module_of(module_type, &table);
        assert_holds_for(module, StructuralPredicate::AutoExec, &table);
    }
}

#[test]
fn event_handler_in_a_document_module_handles_the_workbook_a_worksheet_or_a_chart() {
    let table = [
        ("Workbook_Open", true),
        ("WORKSHEET_Change", true),
        ("chart_Activate", true),
        ("Workbook_Open$", true),
        // The object is the part that names an object, whatever the event holds.
        ("Workbook_Foo_Bar", true),
        ("Worksheet_Calc_Helper", true),
        // `App` is declared `WithEvents`.
        ("App_WorkbookOpen", true),
        ("Sheet1_Click", false),
        ("Application_NewWorkbook", false),
        // The form and its controls are not the business of a document module.
        ("UserForm_Click", false),
        ("Button_Click", false),
        // An object and an event are both needed.
        ("Workbook", false),
        ("Workbook_", false),
        ("Workbook_$", false),
        ("_Open", false),
        ("Helper", false),
    ];
    let module = module_of(ModuleType::Document, &table)
        .with_events(&["App"])
        .with_controls(&["Button"]);
    assert_holds_for(module, StructuralPredicate::EventHandler, &table);
}

#[test]
fn event_handler_in_a_class_module_handles_a_with_events_variable() {
    let table = [
        ("App_WorkbookOpen", true),
        ("app_sheetchange", true),
        // The object `my_Tool` holds an underscore.
        ("my_Tool_Done", true),
        ("Tool_Done", false),
        // A class module handles nothing it does not declare.
        ("Workbook_Open", false),
        ("Worksheet_Change", false),
        ("UserForm_Click", false),
        ("Button_Click", false),
    ];
    let module = module_of(ModuleType::Class, &table)
        .with_events(&["App", "my_Tool"])
        .with_controls(&["Button", "UserForm"]);
    assert_holds_for(module, StructuralPredicate::EventHandler, &table);
}

#[test]
fn event_handler_in_a_form_module_handles_the_form_and_its_controls() {
    // SPEC section 7 gives a form its own events and those of its controls,
    // not those of a `WithEvents` variable.
    let table = [
        ("UserForm_Initialize", true),
        ("userform_Click", true),
        ("CommandButton1_Click", true),
        ("txt_Name_Change", true),
        // A name with letters that take more than one byte.
        ("ÉTIQUETTE1_Click", true),
        ("CommandButton2_Click", false),
        ("Workbook_Open", false),
        ("App_Click", false),
    ];
    let module = module_of(ModuleType::Form, &table)
        .with_controls(&["CommandButton1", "txt_Name", "Étiquette1"])
        .with_events(&["App"]);
    assert_holds_for(module, StructuralPredicate::EventHandler, &table);
}

#[test]
fn event_handler_is_never_in_a_standard_module() {
    let table = [
        ("Workbook_Open", false),
        ("Worksheet_Change", false),
        ("App_WorkbookOpen", false),
        ("UserForm_Click", false),
        ("Button_Click", false),
    ];
    let module = module_of(ModuleType::Standard, &table)
        .with_events(&["App"])
        .with_controls(&["UserForm", "Button"]);
    assert_holds_for(module, StructuralPredicate::EventHandler, &table);
}

#[test]
fn a_name_longer_than_vba_allows_is_no_event_handler() {
    // The limit counts characters: the names of `wide` take two bytes for each.
    let longest = format!("Workbook_{}", "A".repeat(246));
    let too_long = format!("Workbook_{}", "A".repeat(247));
    let wide_longest = format!("Workbook_{}", "é".repeat(246));
    let wide_too_long = format!("Workbook_{}", "é".repeat(247));
    assert_eq!(longest.chars().count(), 255);
    assert_eq!(wide_longest.chars().count(), 255);
    assert!(wide_longest.len() > 255);
    let table = [
        (longest.as_str(), true),
        (too_long.as_str(), false),
        (wide_longest.as_str(), true),
        (wide_too_long.as_str(), false),
    ];
    let module = module_of(ModuleType::Document, &table);
    assert_holds_for(module, StructuralPredicate::EventHandler, &table);
    // The search for the object does not run through a name of megabytes. This
    // one is a handler of the workbook but for its length.
    let none = NameSet::default();
    let hostile = format!("Workbook_{}", "A".repeat(5_000_000));
    let handler = is_event_handler(ModuleType::Document, &hostile, &none, &none);
    assert!(!handler);
}

#[test]
fn the_decisions_of_the_engine_are_open_to_the_callers_that_need_them() {
    let none = NameSet::default();
    let tool: NameSet = ["Tool"].into_iter().collect();
    assert!(is_auto_exec(ModuleType::Document, "workbook_open"));
    assert!(!is_auto_exec(ModuleType::Standard, "Workbook_Open"));
    assert!(is_auto_exec(ModuleType::Standard, "Auto_Open$"));
    let class_with_events = is_event_handler(ModuleType::Class, "tool_Done", &tool, &none);
    let class_with_controls = is_event_handler(ModuleType::Class, "tool_Done", &none, &tool);
    let form_with_controls = is_event_handler(ModuleType::Form, "tool_Done", &none, &tool);
    assert!(class_with_events);
    assert!(!class_with_controls);
    assert!(form_with_controls);
}

#[test]
fn a_list_of_predicates_holds_where_any_of_them_does() {
    let table = [
        ("Workbook_Open", true),
        ("Auto_Open", true),
        ("Worksheet_Change", true),
        ("Helper", false),
    ];
    let project = project(vec![module_of(ModuleType::Document, &table)]);
    let both = [
        StructuralPredicate::AutoExec,
        StructuralPredicate::EventHandler,
    ];
    // `Workbook_Open` is both an auto-exec procedure and an event handler: it
    // gives one hit.
    let hits = project_hits(&project, &both);
    assert_eq!(places(&hits), [(0, 10), (0, 20), (0, 30)]);
    // A predicate twice in the list changes nothing.
    let twice = [StructuralPredicate::AutoExec; 2];
    let hits = project_hits(&project, &twice);
    assert_eq!(places(&hits), [(0, 10), (0, 20)]);
}

#[test]
fn the_other_predicates_give_no_hit_yet() {
    // `Workbook_Open` is an auto-exec procedure and an event handler.
    let table = [("Workbook_Open", true), ("Helper", false)];
    let project = project(vec![module_of(ModuleType::Document, &table)]);
    for predicate in StructuralPredicate::ALL {
        let decided = matches!(
            predicate,
            StructuralPredicate::AutoExec | StructuralPredicate::EventHandler
        );
        let hits = project_hits(&project, &[predicate]);
        assert_eq!(!hits.is_empty(), decided, "{predicate:?}");
    }
}

#[test]
fn a_predicate_about_a_procedure_is_read_at_the_scope_of_the_signal() {
    let table = [("Auto_Open", true), ("Helper", false)];
    let project = project(vec![
        module_of(ModuleType::Standard, &table),
        module_of(ModuleType::Standard, &[("AutoClose", true)]),
    ]);
    let evaluator = Evaluator::new(&project);
    let matcher = structurals(&[StructuralPredicate::AutoExec]);
    let text = TextMode::Code;
    let hits = evaluator.procedure_hits(0, 0, &matcher, text);
    assert_eq!(places(&hits), [(0, 10)]);
    assert!(evaluator.procedure_hits(0, 1, &matcher, text).is_empty());
    let hits = evaluator.module_hits(1, &matcher, text);
    assert_eq!(places(&hits), [(1, 10)]);
    let hits = evaluator.project_hits(&matcher, text);
    assert_eq!(places(&hits), [(0, 10), (1, 10)]);
    // The hit cites the declaration and quotes it.
    assert_eq!(snippets(&hits), ["Sub Auto_Open()", "Sub AutoClose()"]);
}
