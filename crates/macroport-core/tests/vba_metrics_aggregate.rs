// SPDX-License-Identifier: Apache-2.0
//! The metrics of a module and of a file (SPEC M2 item 7): what the metrics of
//! the procedures add up to, and the external dependencies of a file, one test
//! for each rule.

// clippy.toml excuses `expect` in the `#[test]` functions only, not in helpers.
#![allow(clippy::expect_used)]

use macroport_core::report::{
    Declarations, Metrics, ModuleReport, Orientation, ProcedureKind, ProcedureReport, VbaProject,
    Visibility,
};
use macroport_core::rules::ModuleType;
use macroport_core::vba::{SourceLines, file_metrics, module_metrics, split_lines};

/// The lines of a module.
fn lines(text: &str) -> SourceLines {
    split_lines(text).expect("lines")
}

/// The metrics of a procedure with this cyclomatic complexity and this nesting.
fn measured(cyclomatic: u32, max_nesting: u32) -> Metrics {
    Metrics {
        loc: 1,
        logical_loc: 1,
        cyclomatic: Some(cyclomatic),
        max_nesting: Some(max_nesting),
        ..Metrics::default()
    }
}

fn procedure(metrics: Metrics) -> ProcedureReport {
    ProcedureReport {
        name: "Test".to_owned(),
        kind: ProcedureKind::Sub,
        visibility: Some(Visibility::Default),
        is_entry_point: None,
        is_event_handler: None,
        possibly_unused: None,
        start_line: 1,
        end_line: 1,
        metrics,
        calls: None,
        apis: None,
        primary_family: "unclassified".to_owned(),
        secondary_families: None,
        families: Vec::new(),
        signals: Vec::new(),
        traits: None,
        orientation: Orientation,
    }
}

/// A module of this type, of `loc` lines of which `logical_loc` are code, with
/// procedures that have these metrics.
fn module(kind: ModuleType, loc: u32, logical_loc: u32, procedures: &[Metrics]) -> ModuleReport {
    ModuleReport {
        name: "Module".to_owned(),
        module_type: kind,
        stream_name: None,
        source_available: true,
        source_missing_suspected: None,
        source_sha256: None,
        metrics: Metrics {
            loc,
            logical_loc,
            ..Metrics::default()
        },
        declarations: None,
        procedures: procedures.iter().cloned().map(procedure).collect(),
        signals: None,
    }
}

/// A standard module with these procedures.
fn standard(procedures: &[Metrics]) -> ModuleReport {
    module(ModuleType::Standard, 0, 0, procedures)
}

/// An empty standard module whose declarations section counts `declares`
/// statements, as `schema/examples/report.example.json` reports them.
fn with_section(declares: Option<u32>) -> ModuleReport {
    let mut module = standard(&[]);
    module.declarations = Some(Declarations {
        declare_count: declares,
        with_events_count: None,
        option_explicit: None,
        option_vba_support: None,
    });
    module
}

/// An empty standard module whose metrics count `declares` statements.
fn with_metric(declares: u32) -> ModuleReport {
    let mut module = standard(&[]);
    module.metrics.declare_count = Some(declares);
    module
}

fn project_with(modules: Vec<ModuleReport>, references: Option<&[&str]>) -> VbaProject {
    VbaProject {
        name: None,
        protected: false,
        code_page: None,
        references: references.map(|names| names.iter().map(|&name| name.to_owned()).collect()),
        modules,
    }
}

/// The metrics of a file with these modules, no reference and no ProgID.
fn of_modules(modules: Vec<ModuleReport>) -> Metrics {
    file_metrics(&project_with(modules, None), &[] as &[&str])
}

/// The external dependencies of a file with no module, these references, and
/// code that passes these ProgIDs.
fn dependencies(references: &[&str], progids: &[&str]) -> u32 {
    let project = project_with(Vec::new(), Some(references));
    let metrics = file_metrics(&project, progids);
    metrics.external_dependencies.expect("dependencies")
}

/// The external dependencies of a file whose modules have these counts of
/// `Declare` statements in their declarations sections.
fn declare_dependencies(counts: &[Option<u32>]) -> u32 {
    let modules = counts.iter().map(|&count| with_section(count)).collect();
    let metrics = of_modules(modules);
    metrics.external_dependencies.expect("dependencies")
}

// The metrics of a module.

#[test]
fn the_sizes_of_a_module_are_those_of_its_text() {
    // Seven physical lines, of which the `Attribute` line, the blank line and
    // the comment are not code.
    let text =
        "Attribute VB_Name = \"M\"\nOption Explicit\n\n' a comment\nSub A()\n x = 1\nEnd Sub";
    let metrics = module_metrics(&lines(text), &[]);
    assert_eq!((metrics.loc, metrics.logical_loc), (7, 4));

    // A label and its statement are one logical line, as in a procedure.
    let text = "Sub A()\nDone: x = 1\nCleanup:\nEnd Sub\n";
    assert_eq!(module_metrics(&lines(text), &[]).logical_loc, 4);
}

#[test]
fn a_module_adds_up_its_procedures() {
    let procedures = [measured(1, 0), measured(4, 2), measured(3, 1)];
    let metrics = module_metrics(&lines(""), &procedures);

    assert_eq!(metrics.procedures, Some(3));
    // The highest complexity, the sum of the three, and the deepest nesting.
    assert_eq!(metrics.cyclomatic, Some(4));
    assert_eq!(metrics.cyclomatic_total, Some(8));
    assert_eq!(metrics.max_nesting, Some(2));

    // A metric that a procedure leaves out counts as 0, and the procedure counts.
    let metrics = module_metrics(&lines(""), &[Metrics::default(), measured(2, 1)]);
    assert_eq!(metrics.procedures, Some(2));
    assert_eq!(metrics.cyclomatic, Some(2));
    assert_eq!(metrics.cyclomatic_total, Some(2));
}

#[test]
fn a_module_without_a_procedure_has_totals_of_zero() {
    let metrics = module_metrics(&lines("Option Explicit\n"), &[]);

    assert_eq!(metrics.procedures, Some(0));
    assert_eq!(metrics.cyclomatic, Some(0));
    assert_eq!(metrics.cyclomatic_total, Some(0));
    assert_eq!(metrics.max_nesting, Some(0));
    // The metrics of a procedure and those of a file are left out, and so is the
    // count of `Declare` statements, which the module reports in its section.
    assert_eq!(metrics.parameters, None);
    assert_eq!(metrics.goto_count, None);
    assert_eq!(metrics.forms, None);
    assert_eq!(metrics.declare_count, None);
    assert_eq!(metrics.external_dependencies, None);
}

// The metrics of a file.

#[test]
fn the_sizes_of_a_file_add_up_those_of_its_modules() {
    let metrics = of_modules(vec![
        module(ModuleType::Standard, 7, 5, &[]),
        module(ModuleType::Class, 20, 11, &[]),
        module(ModuleType::Document, 3, 0, &[]),
    ]);

    assert_eq!((metrics.loc, metrics.logical_loc), (30, 16));
}

#[test]
fn a_file_adds_up_the_procedures_of_all_its_modules() {
    let metrics = of_modules(vec![
        standard(&[measured(2, 1), measured(6, 3)]),
        standard(&[]),
        standard(&[measured(5, 4)]),
    ]);

    assert_eq!(metrics.procedures, Some(3));
    // The highest complexity of any procedure, the sum of all, and the deepest
    // nesting of any.
    assert_eq!(metrics.cyclomatic, Some(6));
    assert_eq!(metrics.cyclomatic_total, Some(13));
    assert_eq!(metrics.max_nesting, Some(4));
    // The metrics of a procedure are left out.
    assert_eq!((metrics.parameters, metrics.goto_count), (None, None));
}

#[test]
fn a_file_without_a_module_has_totals_of_zero() {
    let metrics = of_modules(Vec::new());

    assert_eq!((metrics.loc, metrics.logical_loc), (0, 0));
    assert_eq!(metrics.procedures, Some(0));
    assert_eq!(metrics.cyclomatic, Some(0));
    assert_eq!(metrics.cyclomatic_total, Some(0));
    assert_eq!(metrics.max_nesting, Some(0));
    assert_eq!(metrics.forms, Some(0));
    assert_eq!(metrics.declare_count, Some(0));
    assert_eq!(metrics.external_dependencies, Some(0));
}

#[test]
fn the_forms_of_a_file_are_its_modules_of_the_form_type() {
    let form = |kind| module(kind, 0, 0, &[]);
    let metrics = of_modules(vec![
        form(ModuleType::Standard),
        form(ModuleType::Form),
        form(ModuleType::Class),
        form(ModuleType::Form),
        form(ModuleType::Document),
    ]);

    assert_eq!(metrics.forms, Some(2));
}

#[test]
fn the_declare_count_of_a_file_adds_up_those_of_its_modules() {
    let metrics = of_modules(vec![
        with_section(Some(3)),
        with_section(Some(0)),
        with_section(Some(4)),
    ]);
    assert_eq!(metrics.declare_count, Some(7));

    // The report has the count of a module in its declarations section, and the
    // pipeline puts it in the metrics as well. The metric stands in for a
    // section that has none, and a module that has both is counted once, by its
    // section.
    let mut metric_too = with_section(None);
    metric_too.metrics.declare_count = Some(4);
    let mut both = with_section(Some(1));
    both.metrics.declare_count = Some(7);
    let metrics = of_modules(vec![with_metric(2), metric_too, both]);
    assert_eq!(metrics.declare_count, Some(7));
}

#[test]
fn a_module_that_does_not_say_has_no_declare_statement() {
    // A module that was not read has no declarations: it adds nothing, and does
    // not hide the count of the others.
    let metrics = of_modules(vec![standard(&[]), with_section(Some(2))]);
    assert_eq!(metrics.declare_count, Some(2));

    // A section that does not say counts for nothing either.
    assert_eq!(of_modules(vec![with_section(None)]).declare_count, Some(0));
}

#[test]
fn the_totals_stop_at_the_largest_count() {
    let metrics = module_metrics(&lines(""), &[measured(u32::MAX, 0), measured(7, 0)]);
    assert_eq!(metrics.cyclomatic_total, Some(u32::MAX));

    let metrics = of_modules(vec![
        module(ModuleType::Standard, u32::MAX, u32::MAX, &[]),
        module(
            ModuleType::Standard,
            5,
            5,
            &[measured(u32::MAX, 0), measured(7, 0)],
        ),
        with_section(Some(u32::MAX)),
        with_section(Some(1)),
    ]);
    assert_eq!((metrics.loc, metrics.logical_loc), (u32::MAX, u32::MAX));
    assert_eq!(metrics.cyclomatic_total, Some(u32::MAX));
    assert_eq!(metrics.declare_count, Some(u32::MAX));
}

// The external dependencies of a file.

#[test]
fn each_distinct_progid_is_a_dependency() {
    assert_eq!(dependencies(&[], &[]), 0);
    assert_eq!(dependencies(&[], &["Word.Application"]), 1);
    assert_eq!(
        dependencies(&[], &["Word.Application", "Outlook.Application"]),
        2
    );
    // However often, and in whatever case, it is passed.
    let progids = ["Word.Application", "word.application", "WORD.APPLICATION"];
    assert_eq!(dependencies(&[], &progids), 1);
}

#[test]
fn the_scripting_dictionary_and_the_regexp_are_no_dependency() {
    assert_eq!(
        dependencies(&[], &["Scripting.Dictionary", "VBScript.RegExp"]),
        0
    );
    assert_eq!(
        dependencies(&[], &["scripting.dictionary", "VBSCRIPT.REGEXP"]),
        0
    );
    // The other scripting objects are.
    assert_eq!(
        dependencies(&[], &["Scripting.FileSystemObject", "WScript.Shell"]),
        2
    );
}

#[test]
fn a_progid_is_a_dotted_name_that_starts_with_a_letter() {
    let counted = |text: &str| dependencies(&[], &[text]) == 1;
    for progid in [
        "Word.Application",
        "Excel.Application.16",
        "MSXML2.XMLHTTP.6.0",
        "My_Server.Class_1",
        "a.b",
        // The shape is all that is tested: a bare file name has it.
        "book.xlsx",
    ] {
        assert!(counted(progid), "{progid}");
    }
    for text in [
        "",
        "htmlfile",
        ".Application",
        "Word.",
        "1Word.Application",
        "_Word.Application",
        " Word.Application",
        "Word Application",
        "Word-Application.1",
        "Caf\u{e9}.Application",
        // What `GetObject` takes besides a ProgID: a path and a moniker.
        "C:\\Reports\\book.xlsx",
        "winmgmts:\\\\.\\root\\cimv2",
    ] {
        assert!(!counted(text), "{text:?}");
    }
}

#[test]
fn a_progid_has_at_most_255_bytes() {
    let long = |length: usize| format!("A.{}", "b".repeat(length.saturating_sub(2)));
    let counted = |text: String| dependencies(&[], &[text.as_str()]) == 1;

    assert!(counted(long(255)));
    assert!(!counted(long(256)));
    // A folded string can be megabytes long: it is turned down by its length.
    assert!(!counted(long(4 << 20)));
}

#[test]
fn a_reference_that_is_not_a_default_one_is_a_dependency() {
    assert_eq!(dependencies(&["Microsoft Scripting Runtime"], &[]), 1);
    let references = [
        "Microsoft Scripting Runtime",
        "Microsoft Outlook 16.0 Object Library",
    ];
    assert_eq!(dependencies(&references, &[]), 2);

    // Once, whatever the case and the margins; a blank name is none.
    let references = [
        "Microsoft Scripting Runtime",
        "microsoft scripting runtime",
        " MICROSOFT SCRIPTING RUNTIME ",
        "",
        "  ",
    ];
    assert_eq!(dependencies(&references, &[]), 1);
}

#[test]
fn the_references_that_every_project_of_excel_has_are_no_dependency() {
    let references = [
        "Visual Basic For Applications",
        "Microsoft Excel 16.0 Object Library",
        "OLE Automation",
        "Microsoft Office 16.0 Object Library",
    ];
    assert_eq!(dependencies(&references, &[]), 0);

    // Whatever the version of Office, the case and the margins.
    let references = [
        "Microsoft Excel 12.0 Object Library",
        "Microsoft Office 14.0 Object Library",
        "microsoft office 15.0 object library",
        "  OLE AUTOMATION ",
        "visual basic for applications",
    ];
    assert_eq!(dependencies(&references, &[]), 0);

    // The short names that `rules/complexity.yaml` gives them.
    assert_eq!(dependencies(&["VBA", "Excel", "stdole", "Office"], &[]), 0);
}

#[test]
fn a_library_that_only_looks_like_a_default_one_is_a_dependency() {
    let references = [
        // Another library of Office, with a version in its name.
        "Microsoft Office 16.0 Access database engine Object Library",
        // The forms of Excel, which `rules/complexity.yaml` does not exempt.
        "Microsoft Forms 2.0 Object Library",
        // A version that is none, and no version.
        "Microsoft Excel x.y Object Library",
        "Microsoft Excel Object Library",
        "Microsoft Excel 16.0",
    ];
    assert_eq!(dependencies(&references, &[]), 5);
}

#[test]
fn one_declare_statement_or_many_are_one_dependency() {
    assert_eq!(declare_dependencies(&[Some(1)]), 1);
    assert_eq!(declare_dependencies(&[Some(2), Some(0), Some(5)]), 1);

    assert_eq!(declare_dependencies(&[]), 0);
    assert_eq!(declare_dependencies(&[Some(0), Some(0)]), 0);
    assert_eq!(declare_dependencies(&[None]), 0);
}

#[test]
fn the_progids_the_references_and_the_declare_statements_add_up() {
    let references = [
        "Visual Basic For Applications",
        "Microsoft Scripting Runtime",
        "Microsoft Outlook 16.0 Object Library",
    ];
    let progids = [
        "Outlook.Application",
        "outlook.application",
        "Word.Application",
    ];
    let project = project_with(vec![with_section(Some(2))], Some(&references));
    let metrics = file_metrics(&project, progids);

    // Two ProgIDs, two references, and the `Declare` statements.
    assert_eq!(metrics.external_dependencies, Some(5));
}

#[test]
fn the_progids_can_be_given_in_any_collection_of_strings() {
    let owned: Vec<String> = vec![
        "Word.Application".to_owned(),
        "Excel.Application".to_owned(),
    ];
    let project = project_with(Vec::new(), None);
    let words = "Word.Application Excel.Application Word.Application";

    assert_eq!(
        file_metrics(&project, &owned).external_dependencies,
        Some(2)
    );
    assert_eq!(file_metrics(&project, owned).external_dependencies, Some(2));
    assert_eq!(
        file_metrics(&project, words.split(' ')).external_dependencies,
        Some(2)
    );
}
