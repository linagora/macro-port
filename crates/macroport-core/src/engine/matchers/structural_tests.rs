// SPDX-License-Identifier: Apache-2.0
//! Tests of the structural predicates.

use crate::engine::testing::{
    line, lines_hit, lines_of, module, places, procedure, project, snippets, structurals,
};
use crate::engine::{
    CodeLine, Evaluator, Hit, ModuleInput, NameSet, ProjectInput, is_auto_exec, is_event_handler,
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

/// A statement that assigns the string literal `text`.
fn assigning(number: u32, text: &str) -> CodeLine {
    line(number, &format!("s = \"{text}\""))
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

#[test]
fn long_literal_is_two_hundred_characters_of_the_hex_or_base64_alphabet() {
    let hex = "4D5A9000".repeat(25);
    let short_hex = format!("{}4D5A900", "4D5A9000".repeat(24));
    let padded = format!("{}==", "Qm9v+/".repeat(33));
    let padded_by_one = format!("{}=", "A".repeat(199));
    let over_padded = format!("{}===", "Qm9v+/".repeat(33));
    let equal_inside = format!("{}={}", "A".repeat(100), "A".repeat(99));
    let with_space = format!("{} {}", "A".repeat(100), "A".repeat(99));
    let url_safe = format!("{}-", "A".repeat(199));
    let not_ascii = format!("{}é", "A".repeat(199));
    let body = [
        assigning(2, &hex),
        assigning(3, &short_hex),
        assigning(4, &padded),
        assigning(5, &over_padded),
        assigning(6, &equal_inside),
        assigning(7, &with_space),
        assigning(8, &url_safe),
        assigning(9, &not_ascii),
        // One hit for each literal.
        line(10, &format!("Call Use(\"{hex}\", \"{hex}\")")),
        // The parser folds the pieces into the value that is read.
        line(11, "s = \"AAAA\" & \"BBBB\"").with_strings(&[&hex]),
        // Without folding, each piece is short.
        line(
            12,
            &format!("s = \"{}\" & \"{}\"", "A".repeat(100), "B".repeat(100)),
        ),
        // Only the alphabet counts: plain letters make a long literal.
        assigning(13, &"x".repeat(200)),
        assigning(14, &padded_by_one),
    ];
    let matcher = structurals(&[StructuralPredicate::LongHexOrBase64Literal]);
    assert_eq!(lines_hit(&body, &matcher), [2, 4, 10, 10, 11, 13, 14]);
}

#[test]
fn long_literal_is_read_in_the_declarations_at_module_scope_only() {
    let hex = "4D5A9000".repeat(25);
    let main = procedure("Main", 5, vec![line(6, "x = 1")]);
    let declarations = vec![assigning(2, &hex)];
    let module = module("Module1", ModuleType::Standard, vec![main]);
    let project = project(vec![module.with_declarations(declarations)]);
    let evaluator = Evaluator::new(&project);
    let matcher = structurals(&[StructuralPredicate::LongHexOrBase64Literal]);
    let text = TextMode::Code;
    assert!(evaluator.procedure_hits(0, 0, &matcher, text).is_empty());
    let hits = evaluator.module_hits(0, &matcher, text);
    assert_eq!(places(&hits), [(0, 2)]);
    // The statement is longer than the 200 characters of a snippet.
    assert_eq!(snippets(&hits)[0].chars().count(), 200);
}

#[test]
fn the_literals_are_read_for_long_hex_or_base64_literal_only() {
    let body = [assigning(2, &"4D5A9000".repeat(25))];
    let others: Vec<_> = StructuralPredicate::ALL
        .into_iter()
        .filter(|&predicate| predicate != StructuralPredicate::LongHexOrBase64Literal)
        .collect();
    assert!(lines_hit(&body, &structurals(&others)).is_empty());
}

#[test]
fn the_predicates_of_the_parser_cite_the_declaration_or_the_statements_given() {
    // The lines of `Flagged`: the declaration, the two statements, `End Sub`.
    let body = vec![line(11, "x = 1"), line(12, "y = Chr(1) & Chr(2) & Chr(3)")];
    let mut flagged = procedure("Flagged", 10, body);
    flagged.predicates.udf_candidate = true;
    flagged.predicates.chr_obfuscation = vec![2];
    flagged.predicates.nested_loop_compare = vec![1];
    flagged.predicates.possibly_unused = true;
    flagged.predicates.parse_recovered = true;
    let plain = procedure("Plain", 20, vec![line(21, "x = 1")]);
    let modules = vec![module(
        "Module1",
        ModuleType::Standard,
        vec![flagged, plain],
    )];
    let project = project(modules);
    for predicate in [
        StructuralPredicate::UdfCandidate,
        StructuralPredicate::PossiblyUnused,
        StructuralPredicate::ParseRecovered,
    ] {
        let hits = project_hits(&project, &[predicate]);
        assert_eq!(places(&hits), [(0, 10)], "{predicate:?}");
        assert_eq!(snippets(&hits), ["Sub Flagged()"], "{predicate:?}");
    }
    let hits = project_hits(&project, &[StructuralPredicate::ChrObfuscation]);
    assert_eq!(places(&hits), [(0, 12)]);
    assert_eq!(snippets(&hits), ["y = Chr(1) & Chr(2) & Chr(3)"]);
    let hits = project_hits(&project, &[StructuralPredicate::NestedLoopCompare]);
    assert_eq!(places(&hits), [(0, 11)]);
    assert_eq!(snippets(&hits), ["x = 1"]);
}

/// A project whose second module holds a procedure `Flagged` with these
/// statements, for which the parser cites these positions for
/// `chr_obfuscation`. The first module has a procedure that the parser cites
/// too, to show that a statement is looked for in its own module.
fn cited_by_chr_obfuscation(body: Vec<CodeLine>, positions: Vec<usize>) -> ProjectInput {
    let mut other = procedure("Other", 1, vec![line(2, "o = 1")]);
    other.predicates.chr_obfuscation = vec![1];
    let mut flagged = procedure("Flagged", 10, body);
    flagged.predicates.chr_obfuscation = positions;
    project(vec![
        module("Module1", ModuleType::Standard, vec![other]),
        module("Module2", ModuleType::Standard, vec![flagged]),
    ])
}

#[test]
fn a_cited_position_tells_apart_the_statements_that_share_a_line() {
    // `x = 1: y = Chr(1) & Chr(2) & Chr(3)` on line 11: the lines of the
    // procedure are the declaration, the two statements and `End Sub`.
    let body = vec![line(11, "x = 1"), line(11, "y = Chr(1) & Chr(2) & Chr(3)")];
    let project = cited_by_chr_obfuscation(body.clone(), vec![2]);
    let hits = project_hits(&project, &[StructuralPredicate::ChrObfuscation]);
    assert_eq!(places(&hits), [(0, 2), (1, 11)]);
    assert_eq!(snippets(&hits)[1], "y = Chr(1) & Chr(2) & Chr(3)");
    // Both cited, in any order: the hits come in the order of the statements.
    let project = cited_by_chr_obfuscation(body, vec![2, 1]);
    let hits = project_hits(&project, &[StructuralPredicate::ChrObfuscation]);
    let shown = snippets(&hits);
    assert_eq!(shown[1..], ["x = 1", "y = Chr(1) & Chr(2) & Chr(3)"]);
}

#[test]
fn a_cited_position_outside_the_lines_cites_nothing() {
    // Four lines with the declaration and `End Sub`: positions 0 to 3.
    let body = vec![line(11, "x = 1"), line(12, "y = 2")];
    let project = cited_by_chr_obfuscation(body, vec![4, usize::MAX, 1]);
    let hits = project_hits(&project, &[StructuralPredicate::ChrObfuscation]);
    assert_eq!(places(&hits), [(0, 2), (1, 11)]);
    assert_eq!(snippets(&hits)[1], "x = 1");
}

#[test]
fn nested_loop_compare_gives_one_hit_at_the_last_comparison_among_the_lines() {
    let body = vec![line(11, "x = 1"), line(12, "y = 2"), line(13, "z = 3")];
    let cited_lines = |positions: Vec<usize>| {
        let mut flagged = procedure("Flagged", 10, body.clone());
        flagged.predicates.nested_loop_compare = positions;
        let modules = vec![module("Module1", ModuleType::Standard, vec![flagged])];
        let project = project(modules);
        lines_of(&project_hits(
            &project,
            &[StructuralPredicate::NestedLoopCompare],
        ))
    };
    // The positions come in any order: the last comparison is the last line.
    assert_eq!(cited_lines(vec![2, 3, 1]), [13]);
    assert_eq!(cited_lines(vec![3, 1, 2]), [13]);
    assert_eq!(cited_lines(vec![1]), [11]);
    // A position outside the lines is a comparison that the procedure lacks.
    // The procedure has five lines: `End Sub` at position 4 is the last of them.
    assert_eq!(cited_lines(vec![2, 4]), [14]);
    assert_eq!(cited_lines(vec![2, 5]), [12]);
    assert_eq!(cited_lines(vec![2, 9, usize::MAX]), [12]);
    assert!(cited_lines(vec![9]).is_empty());
    assert!(cited_lines(Vec::new()).is_empty());
}

#[test]
fn a_statement_that_several_predicates_cite_gives_one_hit() {
    let body = vec![line(11, "x = 1"), line(12, "y = 2"), line(13, "z = 3")];
    let mut flagged = procedure("Flagged", 10, body);
    flagged.predicates.chr_obfuscation = vec![3, 1, 3];
    flagged.predicates.nested_loop_compare = vec![3];
    let project = project(vec![module("Module1", ModuleType::Standard, vec![flagged])]);
    let both = [
        StructuralPredicate::NestedLoopCompare,
        StructuralPredicate::ChrObfuscation,
    ];
    // The statements come in the order of the procedure.
    let hits = project_hits(&project, &both);
    assert_eq!(places(&hits), [(0, 11), (0, 13)]);
    // Without the predicate in the list, its positions are not read.
    let hits = project_hits(&project, &both[..1]);
    assert_eq!(places(&hits), [(0, 13)]);
}

/// A standard module whose one procedure is flagged by the parser for `set`
/// and for nothing else, or `None` when the engine decides `set` itself.
fn flagged_for(set: StructuralPredicate) -> Option<ModuleInput> {
    let mut flagged = procedure("Flagged", 10, vec![line(11, "x = 1")]);
    let flags = &mut flagged.predicates;
    match set {
        StructuralPredicate::UdfCandidate => flags.udf_candidate = true,
        StructuralPredicate::ChrObfuscation => flags.chr_obfuscation = vec![1],
        StructuralPredicate::NestedLoopCompare => flags.nested_loop_compare = vec![1],
        StructuralPredicate::PossiblyUnused => flags.possibly_unused = true,
        StructuralPredicate::ParseRecovered => flags.parse_recovered = true,
        StructuralPredicate::EventHandler
        | StructuralPredicate::AutoExec
        | StructuralPredicate::LongHexOrBase64Literal => return None,
    }
    Some(module("Module1", ModuleType::Standard, vec![flagged]))
}

#[test]
fn each_predicate_of_the_parser_reads_its_own_flag_and_no_other() {
    for set in StructuralPredicate::ALL {
        let Some(flagged) = flagged_for(set) else {
            continue;
        };
        let project = project(vec![flagged]);
        for read in StructuralPredicate::ALL {
            let hits = project_hits(&project, &[read]);
            assert_eq!(
                !hits.is_empty(),
                read == set,
                "{read:?} with only {set:?} flagged"
            );
        }
    }
}
