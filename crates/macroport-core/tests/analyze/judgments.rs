// SPDX-License-Identifier: Apache-2.0
//! What the rule engine decides about the modules of a file, as the report
//! gives it (SPEC M3): the families, the signals, the traits and the level of
//! the procedures, of the modules and of the file, and the metrics that the
//! rules read. The modules come as text, through `analyze_modules`, or from a
//! workbook, through `analyze`.

use macroport_core::analyze::{analyze, analyze_modules};
use macroport_core::report::{FileReport, Level, ProcedureReport};
use macroport_core::rules::{ModuleType, Severity};
use macroport_core::vba::{MAX_LINES, ModuleSource};
use macroport_testkit::corpus;

use crate::support::{HELLO, checked, modules, options};

/// A procedure of five logical lines, of which one is a decision.
const BRANCHING: &str =
    "Sub Branching()\n    If x Then\n        y = 1\n    End If\n    z = 2\nEnd Sub\n";

fn source(name: &str, module_type: ModuleType, text: &str) -> ModuleSource {
    ModuleSource {
        name: name.to_owned(),
        module_type,
        text: text.to_owned(),
    }
}

/// The report of `modules` as a text file, once checked against the schema.
fn report_of_modules(modules: Vec<ModuleSource>) -> FileReport {
    checked(analyze_modules(modules, "Book.bas", &options()))
}

/// The report of `text` as the only module of a file.
fn report_of(text: &str) -> FileReport {
    report_of_modules(vec![source("Module1", ModuleType::Standard, text)])
}

/// The first procedure of the first module of a file.
fn first(file: &FileReport) -> &ProcedureReport {
    let module = modules(file).first().expect("a module");
    module.procedures.first().expect("a procedure")
}

/// The ids of the signals of a procedure.
fn signal_ids(procedure: &ProcedureReport) -> Vec<&str> {
    let signals = procedure.signals.iter();
    signals.map(|signal| signal.signal_id.as_str()).collect()
}

/// The ids of a list of traits, none if the report does not give one.
fn trait_ids(traits: &Option<Vec<String>>) -> Option<Vec<&str>> {
    let ids = traits.as_ref()?;
    Some(ids.iter().map(String::as_str).collect())
}

#[test]
fn a_procedure_that_nothing_flags_is_judged_all_the_same() {
    let file = report_of(HELLO);

    let hello = first(&file);
    assert_eq!(hello.primary_family, "unclassified");
    assert_eq!(hello.secondary_families, Some(Vec::new()));
    assert!(hello.families.is_empty());
    assert!(hello.signals.is_empty());
    // Three logical lines and nothing external.
    assert_eq!(trait_ids(&hello.traits), Some(vec!["trivial"]));
    assert_eq!(modules(&file)[0].signals, Some(Vec::new()));
    assert_eq!(file.signals, Some(Vec::new()));
    assert_eq!(file.max_severity, None);
    assert_eq!(file.complexity_level, Some(Level::Low));
    assert_eq!(file.primary_family.as_deref(), Some("unclassified"));
}

#[test]
fn the_auto_exec_procedure_raises_its_signal_and_gives_the_file_its_severity() {
    let file = report_of("Sub Auto_Open()\n    x = 1\n    y = 2\n    z = 3\nEnd Sub\n");

    let auto_open = first(&file);
    assert_eq!(signal_ids(auto_open), ["sec.auto-exec"]);
    let evidence = auto_open.signals[0].evidence.iter().flatten();
    assert_eq!(evidence.map(|item| item.line).collect::<Vec<_>>(), [1]);
    assert_eq!(file.max_severity, Some(Severity::Medium));
    // The signal belongs to the procedure: the file lists its own only.
    assert_eq!(file.signals, Some(Vec::new()));
    assert_eq!(trait_ids(&auto_open.traits), Some(Vec::new()));
}

#[test]
fn the_event_handler_of_a_document_module_is_an_auto_exec_procedure() {
    let handler = "Private Sub Workbook_Open()\n    x = 1\nEnd Sub\n";

    let document = report_of_modules(vec![source("ThisWorkbook", ModuleType::Document, handler)]);
    let standard = report_of(handler);

    assert_eq!(signal_ids(first(&document)), ["sec.auto-exec"]);
    assert!(signal_ids(first(&standard)).is_empty());
}

#[test]
fn a_function_that_a_formula_could_call_is_classified_as_a_udf() {
    let udf = "Function Twice(x)\n    Twice = x * 2\nEnd Function\n";

    let standard = report_of(udf);
    let sheet = report_of_modules(vec![source("Sheet1", ModuleType::Document, udf)]);

    let twice = first(&standard);
    assert_eq!(twice.primary_family, "08-udf");
    assert_eq!(twice.families.first().map(|found| found.score), Some(4.0));
    // The same function in a sheet cannot be called from a formula.
    assert_eq!(first(&sheet).primary_family, "unclassified");
}

#[test]
fn a_procedure_is_judged_on_its_own_measures() {
    let file = report_of(&format!("{BRANCHING}\n{HELLO}"));

    let procedures = &modules(&file)[0].procedures;
    // Five logical lines, and one that is not: only the second is trivial.
    assert_eq!(trait_ids(&procedures[0].traits), Some(Vec::new()));
    assert_eq!(trait_ids(&procedures[1].traits), Some(vec!["trivial"]));
    // Nothing external in either of them.
    assert_eq!(procedures[0].metrics.external_dependencies, Some(0));
    assert_eq!(procedures[1].metrics.external_dependencies, Some(0));
}

/// A procedure of three logical lines that creates the object `class`.
fn creating(class: &str) -> String {
    format!("Sub Create()\n    Set o = CreateObject({class})\nEnd Sub\n")
}

#[test]
fn a_procedure_that_creates_an_object_has_a_dependency_and_is_not_trivial() {
    let file = report_of(&creating("\"WScript.Shell\""));

    let create = first(&file);
    assert_eq!(create.metrics.external_dependencies, Some(1));
    assert_eq!(trait_ids(&create.traits), Some(Vec::new()));
    assert_eq!(
        file.metrics.as_ref().and_then(|m| m.external_dependencies),
        Some(1)
    );
}

#[test]
fn the_class_of_an_object_is_read_once_it_is_folded() {
    let obfuscated = "Sub Create()\n    Set a = CreateObject(\"WScript.Shell\")\n    \
                      Set b = CreateObject(\"wscr\" & \"ipt.\" & \"shell\")\n    \
                      Set c = CreateObject(Prog)\nEnd Sub\nPrivate Const Prog = \"Wscript.Shell\"\n";

    let file = report_of(obfuscated);

    // Three ways to spell the one class, which counts once.
    assert_eq!(first(&file).metrics.external_dependencies, Some(1));
}

#[test]
fn a_class_that_every_machine_has_is_no_dependency() {
    let dictionary = report_of(&creating("\"Scripting.Dictionary\""));
    let unknown = report_of(&creating("cls"));

    // The common classes are left out, and a class that is not a string that
    // folds is none.
    for file in [&dictionary, &unknown] {
        assert_eq!(first(file).metrics.external_dependencies, Some(0));
        assert_eq!(trait_ids(&first(file).traits), Some(vec!["trivial"]));
    }
}

#[test]
fn the_dependencies_of_a_file_count_the_distinct_classes_of_all_its_modules() {
    let outlook = creating("\"Outlook.Application\"");
    let again = "Sub Again()\n    Set o = CreateObject(\"outlook.application\")\n    \
                 Set w = CreateObject(\"Word.Application\")\nEnd Sub\n";
    let excel = creating("\"Excel.Sheet\"");

    let two = report_of_modules(vec![
        source("A", ModuleType::Standard, &outlook),
        source("B", ModuleType::Standard, again),
    ]);
    let three = report_of_modules(vec![
        source("A", ModuleType::Standard, &outlook),
        source("B", ModuleType::Standard, again),
        source("C", ModuleType::Standard, &excel),
    ]);

    let dependencies = |file: &FileReport| file.metrics.as_ref().map(|m| m.external_dependencies);
    // Outlook, once though it is spelled in two ways, and Word.
    assert_eq!(dependencies(&two), Some(Some(2)));
    assert_eq!(two.complexity_level, Some(Level::Medium));
    let procedures: Vec<_> = (modules(&two).iter())
        .map(|module| module.procedures[0].metrics.external_dependencies)
        .collect();
    assert_eq!(procedures, [Some(1), Some(2)]);
    // Three classes make the file high.
    assert_eq!(dependencies(&three), Some(Some(3)));
    assert_eq!(three.complexity_level, Some(Level::High));
}

#[test]
fn the_aggregates_of_modules_and_file_are_the_highest_and_the_sum_of_their_procedures() {
    let nested = "Sub Nested()\n    For i = 1 To 3\n        If x Then\n            y = 1\n        End If\n    \
                  Next\nEnd Sub\n";
    let file = report_of_modules(vec![
        source("A", ModuleType::Standard, BRANCHING),
        source("B", ModuleType::Standard, nested),
        source("C", ModuleType::Standard, "Option Explicit\n"),
    ]);

    // One decision in the first procedure, two in the second.
    let measured: Vec<_> = modules(&file)
        .iter()
        .map(|m| {
            let m = &m.metrics;
            (m.cyclomatic, m.cyclomatic_total, m.max_nesting)
        })
        .collect();
    assert_eq!(
        measured,
        [
            (Some(2), Some(2), Some(1)),
            (Some(3), Some(3), Some(2)),
            (Some(0), Some(0), Some(0))
        ]
    );
    let metrics = file.metrics.as_ref().expect("the metrics of the file");
    assert_eq!(metrics.procedures, Some(2));
    assert_eq!(metrics.cyclomatic, Some(3));
    assert_eq!(metrics.cyclomatic_total, Some(5));
    assert_eq!(metrics.max_nesting, Some(2));
}

#[test]
fn a_declare_statement_makes_the_file_high() {
    let declare = "Private Declare Function GetTickCount Lib \"kernel32\" () As Long\n";
    let file = report_of(&format!("{declare}{HELLO}"));

    let metrics = file.metrics.as_ref().expect("the metrics of the file");
    assert_eq!(metrics.declare_count, Some(1));
    // A `Declare` statement is one dependency, whatever the routines it names.
    assert_eq!(metrics.external_dependencies, Some(1));
    assert_eq!(file.complexity_level, Some(Level::High));
}

#[test]
fn a_procedure_that_calls_a_declared_routine_has_a_dependency_and_is_not_trivial() {
    let declare = "Private Declare Function GetTickCount Lib \"kernel32\" () As Long\n";
    let ticking = "Sub Tick()\n    x = GetTickCount()\nEnd Sub\n";
    let file = report_of(&format!("{declare}{HELLO}{ticking}"));

    let procedures = &modules(&file)[0].procedures;
    // Three logical lines each, and the second one calls a routine that the
    // module declares.
    assert_eq!(trait_ids(&procedures[0].traits), Some(vec!["trivial"]));
    assert_eq!(trait_ids(&procedures[1].traits), Some(Vec::new()));
    assert_eq!(procedures[0].metrics.external_dependencies, Some(0));
    assert_eq!(procedures[1].metrics.external_dependencies, Some(1));
}

#[test]
fn a_routine_that_a_later_module_declares_is_a_dependency_of_the_procedures_before_it() {
    let ticking = "Sub Tick()\n    x = GetTickCount()\n    y = GetTickCount()\nEnd Sub\n";
    let declare = "Private Declare Function GetTickCount Lib \"kernel32\" () As Long\n";

    let file = report_of_modules(vec![
        source("Caller", ModuleType::Standard, ticking),
        source("Api", ModuleType::Standard, declare),
    ]);

    // The routine counts once however many times it is called, and the module
    // that declares it was read after the one that calls it.
    let tick = &modules(&file)[0].procedures[0];
    assert_eq!(tick.metrics.external_dependencies, Some(1));
    assert_eq!(trait_ids(&tick.traits), Some(Vec::new()));
}

#[test]
fn a_method_of_an_object_is_not_a_call_of_the_declared_routine() {
    let declare = "Private Declare Function GetTickCount Lib \"kernel32\" () As Long\n";
    let timer = "Sub Tick()\n    x = clock.GetTickCount()\nEnd Sub\n";

    let file = report_of(&format!("{declare}{timer}"));

    assert_eq!(first(&file).metrics.external_dependencies, Some(0));
}

#[test]
fn a_form_makes_the_file_medium() {
    let file = report_of_modules(vec![source("Form1", ModuleType::Form, HELLO)]);

    assert_eq!(file.metrics.as_ref().map(|m| m.forms), Some(Some(1)));
    assert_eq!(file.complexity_level, Some(Level::Medium));
}

#[test]
fn the_modules_after_one_that_was_not_read_get_their_own_findings() {
    let big = "a\n".repeat(MAX_LINES as usize + 1);
    let file = report_of_modules(vec![
        source("First", ModuleType::Standard, "Sub Auto_Open()\nEnd Sub\n"),
        source("Big", ModuleType::Standard, &big),
        source("Last", ModuleType::Standard, BRANCHING),
    ]);

    let modules = modules(&file);
    // The module that was not read is not judged, and does not shift the others.
    assert_eq!(signal_ids(&modules[0].procedures[0]), ["sec.auto-exec"]);
    assert_eq!(modules[0].signals, Some(Vec::new()));
    assert_eq!(modules[1].signals, None);
    assert!(modules[1].procedures.is_empty());
    assert_eq!(modules[1].declarations, None);
    assert_eq!(modules[2].signals, Some(Vec::new()));
    let last = &modules[2].procedures[0];
    assert!(signal_ids(last).is_empty());
    assert_eq!(trait_ids(&last.traits), Some(Vec::new()));
    assert_eq!(last.metrics.cyclomatic, Some(2));
    // The file is judged on what was read.
    assert_eq!(file.max_severity, Some(Severity::Medium));
    assert!(file.complexity_level.is_some());
}

#[test]
fn a_module_that_was_not_read_does_not_shift_the_dependencies_of_the_others() {
    let big = "a\n".repeat(MAX_LINES as usize + 1);
    let outlook = creating("\"Outlook.Application\"");
    let word = "Sub Create()\n    Set o = CreateObject(\"Word.Application\")\n    \
                Set p = CreateObject(\"Excel.Sheet\")\nEnd Sub\n";

    let file = report_of_modules(vec![
        source("First", ModuleType::Standard, &outlook),
        source("Big", ModuleType::Standard, &big),
        source("Last", ModuleType::Standard, word),
    ]);

    let procedures: Vec<_> = (modules(&file).iter())
        .map(|module| module.procedures.first())
        .map(|procedure| procedure.map(|found| found.metrics.external_dependencies))
        .collect();
    assert_eq!(procedures, [Some(Some(1)), None, Some(Some(2))]);
    let metrics = file.metrics.as_ref().expect("the metrics of the file");
    assert_eq!(metrics.external_dependencies, Some(3));
}

#[test]
fn a_file_where_no_module_was_read_is_not_judged() {
    let big = "a\n".repeat(MAX_LINES as usize + 1);

    let file = report_of_modules(vec![source("Big", ModuleType::Standard, &big)]);

    assert_eq!(file.complexity_level, None);
    assert_eq!(file.primary_family, None);
    assert_eq!(file.families, None);
    assert_eq!(file.signals, None);
    assert_eq!(file.traits, None);
    assert_eq!(file.max_severity, None);
}

#[test]
fn a_workbook_is_judged_like_a_text() {
    let bytes = corpus::read("fixtures/generated/14-01-auto-open-shell.xlsm");

    let file = checked(analyze(&bytes, "14-01.xlsm", &options()));

    let procedures = modules(&file).iter().flat_map(|module| &module.procedures);
    let auto_exec: Vec<_> = procedures
        .filter(|procedure| signal_ids(procedure).contains(&"sec.auto-exec"))
        .map(|procedure| procedure.name.as_str())
        .collect();
    assert_eq!(auto_exec.len(), 1, "{auto_exec:?}");
    // The chain with the launch of a program is high once the calls are read.
    assert!(matches!(
        file.max_severity,
        Some(Severity::Medium | Severity::High)
    ));
    assert!(file.complexity_level.is_some());
    assert!(file.primary_family.is_some());
}
