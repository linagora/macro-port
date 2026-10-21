// SPDX-License-Identifier: Apache-2.0
//! What the front end makes of the modules of a file (SPEC M2): their
//! procedures with their spans and metrics, the status that follows from them,
//! and the trait of what did not parse cleanly. The modules come as text,
//! through `analyze_modules`, or from a workbook, through `analyze`.

use macroport_core::analyze::{Options, analyze, analyze_modules};
use macroport_core::report::{
    Container, FileError, FileReport, Metrics, ModuleReport, ProcedureKind, ProcedureReport,
    Status, Visibility,
};
use macroport_core::rules::{ModuleType, RuleSet, RulesSources};
use macroport_core::vba::{MAX_LINES, ModuleSource};
use macroport_testkit::corpus;

use crate::support::{
    HELLO, checked, marks_recovery, module, modules, options, procedures, sha256,
};

fn source(name: &str, module_type: ModuleType, text: &str) -> ModuleSource {
    ModuleSource {
        name: name.to_owned(),
        module_type,
        text: text.to_owned(),
    }
}

/// The report of `modules` as a text file.
fn report_of_modules(modules: Vec<ModuleSource>) -> FileReport {
    checked(analyze_modules(modules, "Book.bas", &options()))
}

/// The report of `text` as the only module of a file.
fn report_of(text: &str) -> FileReport {
    report_of_modules(vec![source("Module1", ModuleType::Standard, text)])
}

/// The complexity that `metrics` give: the highest cyclomatic complexity, their
/// sum, and the deepest nesting.
fn complexity(metrics: &Metrics) -> (Option<u32>, Option<u32>, Option<u32>) {
    (
        metrics.cyclomatic,
        metrics.cyclomatic_total,
        metrics.max_nesting,
    )
}

/// The names of the procedures of `module`, with whether each is suspected of
/// not compiling.
fn names(module: &ModuleReport) -> Vec<(&str, bool)> {
    let marked = |procedure: &ProcedureReport| marks_recovery(procedure.traits.as_deref());
    (module.procedures.iter())
        .map(|procedure| (procedure.name.as_str(), marked(procedure)))
        .collect()
}

#[test]
fn modules_handed_over_as_text_are_a_text_module_file() {
    let file = checked(analyze_modules(
        vec![source("Module1", ModuleType::Standard, HELLO)],
        "macros/Module1.bas",
        &options(),
    ));

    assert_eq!(file.path, "macros/Module1.bas");
    assert_eq!(file.size, HELLO.len() as u64);
    assert_eq!(file.sha256, sha256(HELLO.as_bytes()));
    assert_eq!(
        (file.container, file.status),
        (Container::TextModule, Status::Ok)
    );
    assert_eq!(file.errors, Some(Vec::new()));
    let project = file.vba_project.as_ref().expect("a VBA project");
    assert_eq!((project.protected, project.code_page), (false, None));
    let module = module(&file);
    assert_eq!(module.name, "Module1");
    assert_eq!(module.module_type, ModuleType::Standard);
    assert_eq!(module.stream_name, None);
    assert!(module.source_available);
    assert_eq!(module.source_sha256, Some(sha256(HELLO.as_bytes())));
}

#[test]
fn a_procedure_has_its_name_kind_visibility_span_and_size() {
    let file = report_of(
        "Public Function Add(a As Long, b As Long) As Long\n    ' sum\n\n    Add = a + b\nEnd Function\n\n\
         Private Property Get Name() As String\nEnd Property\n\nFriend Sub Hello()\nEnd Sub\n",
    );

    let found: Vec<_> = (module(&file).procedures.iter())
        .map(|p| {
            (
                p.name.as_str(),
                p.kind,
                p.visibility,
                p.start_line,
                p.end_line,
            )
        })
        .collect();
    assert_eq!(
        found,
        [
            (
                "Add",
                ProcedureKind::Function,
                Some(Visibility::Public),
                1,
                5
            ),
            (
                "Name",
                ProcedureKind::PropertyGet,
                Some(Visibility::Private),
                7,
                8
            ),
            (
                "Hello",
                ProcedureKind::Sub,
                Some(Visibility::Friend),
                10,
                11
            ),
        ]
    );
    let procedures = &module(&file).procedures;
    // Five physical lines, of which the comment and the blank are not code.
    let add = &procedures[0].metrics;
    assert_eq!((add.loc, add.logical_loc, add.parameters), (5, 3, Some(2)));
    assert_eq!(procedures[1].metrics.parameters, Some(0));
}

#[test]
fn the_metrics_of_a_module_and_of_its_file_sum_up_the_lines() {
    let file = report_of_modules(vec![
        source("Code", ModuleType::Standard, HELLO),
        source(
            "Form1",
            ModuleType::Form,
            "Private Sub Cancel_Click()\nEnd Sub\n",
        ),
        source("Form2", ModuleType::Form, ""),
    ]);

    // Five physical lines, and the four that are neither blank nor comment.
    let code = &modules(&file)[0];
    assert_eq!((code.metrics.loc, code.metrics.logical_loc), (5, 4));
    assert_eq!(code.metrics.procedures, Some(1));
    assert_eq!(code.procedures[0].metrics.procedures, None);
    let metrics = file.metrics.as_ref().expect("the metrics of the file");
    assert_eq!((metrics.loc, metrics.logical_loc), (7, 6));
    assert_eq!((metrics.procedures, metrics.forms), (Some(2), Some(2)));
}

/// A procedure with two parameters and seven decision points: an `If` with an
/// `And`, a loop, a one-line `If`, an `ElseIf` and two `Case` clauses (`Case
/// Else` is none). Its blocks nest twice, and it has one `GoTo`.
const BRANCHING: &str = "\
Sub Work(a As Long, b As Long)
    If a > 0 And b > 0 Then
        For i = 1 To a
            If i = 2 Then GoTo Done
        Next i
    ElseIf a = 0 Then
        Select Case b
            Case 1
                x = 1
            Case 2, 3
                x = 2
            Case Else
                x = 3
        End Select
    End If
Done:
End Sub
";

#[test]
fn a_procedure_reports_its_complexity_nesting_parameters_and_jumps() {
    let file = report_of(BRANCHING);

    let work = &module(&file).procedures[0].metrics;
    assert_eq!(work.cyclomatic, Some(8));
    assert_eq!(work.max_nesting, Some(2));
    assert_eq!((work.parameters, work.goto_count), (Some(2), Some(1)));
    assert_eq!((work.loc, work.logical_loc), (17, 17));
}

/// A procedure whose blocks nest three times, with three decision points.
const NESTED: &str = "\
Sub Deep()
    If a Then
        If b Then
            If c Then
                x = 1
            End If
        End If
    End If
End Sub
";

#[test]
fn the_complexity_of_a_module_and_of_its_file_follows_from_its_procedures() {
    let branching = format!("{BRANCHING}Sub Flat()\nEnd Sub\n");
    let file = report_of_modules(vec![
        source("Branching", ModuleType::Standard, &branching),
        source("Nested", ModuleType::Standard, NESTED),
        source("Plain", ModuleType::Standard, "Option Explicit\n"),
    ]);

    // The greatest complexity and nesting, and the complexities summed.
    let found: Vec<_> = (modules(&file).iter())
        .map(|m| complexity(&m.metrics))
        .collect();
    assert_eq!(
        found,
        [
            (Some(8), Some(9), Some(2)),
            (Some(4), Some(4), Some(3)),
            // A module with no procedure measures zero: it has no complexity to
            // add up.
            (Some(0), Some(0), Some(0)),
        ]
    );
    let metrics = file.metrics.as_ref().expect("the metrics of the file");
    assert_eq!(complexity(metrics), (Some(8), Some(13), Some(3)));
    // The jumps and the parameters belong to the procedures.
    assert_eq!((metrics.goto_count, metrics.parameters), (None, None));
}

#[test]
fn a_label_and_its_statement_on_one_line_count_once_in_a_procedure_and_in_its_module() {
    let file = report_of("Sub A()\nDone: x = 1\nEnd Sub\n");

    let module = module(&file);
    assert_eq!(module.procedures[0].metrics.logical_loc, 3);
    assert_eq!(module.metrics.logical_loc, 3);
}

#[test]
fn the_lines_of_a_module_count_as_the_file_presents_them() {
    let file = report_of(
        "VERSION 1.0 CLASS\nBEGIN\n  MultiUse = -1\nEND\nAttribute VB_Name = \"Helper\"\n\
         Option Explicit\n\nPublic Sub Run()\nEnd Sub\n",
    );

    // Nine lines, of which the export header and the attribute are no code.
    let module = module(&file);
    assert_eq!((module.metrics.loc, module.metrics.logical_loc), (9, 3));
    let run = &module.procedures[0];
    assert_eq!((run.start_line, run.end_line), (8, 9));
}

#[test]
fn a_file_without_a_procedure_has_no_macros_but_is_described() {
    for text in ["", "Option Explicit\nDim x As Long\n", "' a comment\n"] {
        let file = report_of(text);

        assert_eq!(file.status, Status::NoMacros, "{text:?}");
        assert_eq!(file.errors, Some(Vec::new()));
        assert!(module(&file).procedures.is_empty(), "{text:?}");
        assert_eq!(module(&file).metrics.procedures, Some(0), "{text:?}");
    }
    assert_eq!(report_of_modules(Vec::new()).status, Status::NoMacros);
}

#[test]
fn a_file_is_ok_when_any_of_its_modules_has_a_procedure() {
    let file = report_of_modules(vec![
        source("Empty", ModuleType::Document, "Option Explicit\n"),
        source("Code", ModuleType::Standard, HELLO),
    ]);

    assert_eq!(file.status, Status::Ok);
    let found: Vec<_> = (modules(&file).iter())
        .map(|module| (module.name.as_str(), module.procedures.len()))
        .collect();
    assert_eq!(found, [("Empty", 0), ("Code", 1)]);
}

#[test]
fn the_size_and_hash_of_several_modules_are_those_of_their_texts_end_to_end() {
    let file = report_of_modules(vec![
        source("A", ModuleType::Standard, "Sub A()\nEnd Sub\n"),
        source("B", ModuleType::Standard, "Sub B()\nEnd Sub\n"),
    ]);

    let joined = "Sub A()\nEnd Sub\nSub B()\nEnd Sub\n";
    assert_eq!(file.size, joined.len() as u64);
    assert_eq!(file.sha256, sha256(joined.as_bytes()));
    // Each module keeps its own hash.
    let hashes: Vec<_> = modules(&file)
        .iter()
        .map(|m| m.source_sha256.clone())
        .collect();
    assert_eq!(
        hashes,
        [
            Some(sha256(b"Sub A()\nEnd Sub\n")),
            Some(sha256(b"Sub B()\nEnd Sub\n"))
        ]
    );
}

#[test]
fn modules_above_the_size_limit_are_too_large_and_not_read() {
    let limited = Options {
        max_file_size: 10,
        ..options()
    };
    let sources = vec![source("Module1", ModuleType::Standard, HELLO)];

    let file = checked(analyze_modules(sources, "Module1.bas", &limited));

    assert_eq!(
        (file.container, file.status),
        (Container::TextModule, Status::TooLarge)
    );
    assert_eq!(file.size, HELLO.len() as u64);
    assert_eq!(file.sha256, sha256(HELLO.as_bytes()));
    assert!(file.vba_project.is_none());
    let errors = file.errors.expect("errors");
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "file.too_large");
}

#[test]
fn a_module_with_too_many_lines_is_too_large_and_the_others_are_still_read() {
    let big = "a\n".repeat(MAX_LINES as usize + 1);

    let file = report_of_modules(vec![
        source("Big", ModuleType::Standard, &big),
        source("Small", ModuleType::Standard, HELLO),
    ]);

    assert_eq!(file.status, Status::TooLarge);
    let errors = file.errors.as_ref().expect("errors");
    let expected = FileError {
        code: "vba.too_many_lines".to_owned(),
        message: format!("module Big: more than {MAX_LINES} lines"),
    };
    assert_eq!(errors, &[expected]);
    let [big, small] = modules(&file) else {
        panic!("two modules expected");
    };
    assert!(big.source_available);
    assert_eq!((big.procedures.len(), big.metrics.loc), (0, 0));
    assert_eq!(small.procedures.len(), 1);
}

#[test]
fn a_module_with_too_many_procedures_is_too_large_and_the_others_are_still_read() {
    // Each `Sub` line ends the procedure before it, so a line is a procedure.
    let many = "Sub A\n".repeat(50_001);

    let file = report_of_modules(vec![
        source("Many", ModuleType::Standard, &many),
        source("Small", ModuleType::Standard, HELLO),
    ]);

    assert_eq!(file.status, Status::TooLarge);
    let errors = file.errors.as_ref().expect("errors");
    let expected = FileError {
        code: "vba.too_many_procedures".to_owned(),
        message: "module Many: the modules of the file have more than 50000 procedures".to_owned(),
    };
    assert_eq!(errors, &[expected]);
    let [many, small] = modules(&file) else {
        panic!("two modules expected");
    };
    assert_eq!(many.procedures.len(), 0);
    assert_eq!(small.procedures.len(), 1);
}

#[test]
fn a_module_with_too_much_code_is_too_large_and_the_others_are_still_read() {
    // A statement of half a million names may take more than the rule engine
    // may read, whatever its text weighs, and it is refused before it is read.
    let dense = format!("x = {}a\n", "a.".repeat(250_000));

    let file = report_of_modules(vec![
        source("First", ModuleType::Standard, HELLO),
        source("Dense", ModuleType::Standard, &dense),
        source("Third", ModuleType::Standard, HELLO),
    ]);

    assert_eq!(file.status, Status::TooLarge);
    let errors = file.errors.as_ref().expect("errors");
    let expected = FileError {
        code: "vba.too_much_code".to_owned(),
        message:
            "module Dense: the statements of the modules of the file take more than 64 MiB to read"
                .to_owned(),
    };
    assert_eq!(errors, &[expected]);
    let [first, dense, third] = modules(&file) else {
        panic!("three modules expected");
    };
    // The dense module is listed as a module that no stage measured, and the
    // modules around it are read.
    assert_eq!((dense.procedures.len(), dense.metrics.loc), (0, 0));
    assert_eq!((first.procedures.len(), third.procedures.len()), (1, 1));
}

#[test]
fn the_modules_of_a_file_are_read_up_to_a_budget_of_lines() {
    // The rule engine keeps what it reads of every module until the last one is
    // read, so the lines of a file are bounded to bound its memory.
    let sixty_thousand = "a\n".repeat(60_000);

    let file = report_of_modules(vec![
        source("First", ModuleType::Standard, &sixty_thousand),
        source("Second", ModuleType::Standard, &sixty_thousand),
        source("Third", ModuleType::Standard, "Sub A()\nEnd Sub\n"),
    ]);

    // The second crosses the budget and is read whole, and the third is not read.
    assert_eq!(file.status, Status::TooLarge);
    let errors = file.errors.as_ref().expect("errors");
    let expected = FileError {
        code: "vba.too_many_lines".to_owned(),
        message: "module Third: the modules of the file have more than 100000 lines".to_owned(),
    };
    assert_eq!(errors, &[expected]);
    let loc: Vec<_> = modules(&file).iter().map(|m| m.metrics.loc).collect();
    assert_eq!(loc, [60_000, 60_000, 0]);
    assert!(modules(&file)[2].procedures.is_empty());
}

#[test]
fn a_procedure_that_did_not_parse_cleanly_gets_the_trait_compile_error_suspected() {
    for (text, expected) in [
        ("Sub Broken()\n    x = 1\n", true),
        ("Sub Broken()\n    x = 1\nEnd Function\n", true),
        ("Sub (a, b)\nEnd Sub\n", true),
        // The blocks do not match: an If has no End If.
        (
            "Sub Broken()\n    If x Then\n        y = 1\nEnd Sub\n",
            true,
        ),
        ("Sub Fine()\n    x = 1\nEnd Sub\n", false),
    ] {
        let file = report_of(text);

        assert_eq!(file.status, Status::Ok, "{text:?}");
        let first = &module(&file).procedures[0];
        assert_eq!(
            marks_recovery(first.traits.as_deref()),
            expected,
            "{text:?}"
        );
        assert_eq!(marks_recovery(file.traits.as_deref()), expected, "{text:?}");
    }
}

#[test]
fn a_procedure_whose_blocks_do_not_match_gets_the_trait_and_marks_the_file() {
    for (text, expected) in [
        // An `If` that nothing ends, an `End If` that nothing opened, an `Else`
        // outside any `If`.
        (
            "Sub Open_If()\n    If x Then\n        y = 1\nEnd Sub\n",
            true,
        ),
        ("Sub Spare_End()\n    y = 1\n    End If\nEnd Sub\n", true),
        ("Sub Spare_Else()\n    Else\nEnd Sub\n", true),
        (
            "Sub Fine()\n    If x Then\n        y = 1\n    End If\nEnd Sub\n",
            false,
        ),
    ] {
        let file = report_of(&format!("{text}Sub Other()\nEnd Sub\n"));

        let [first, other] = &module(&file).procedures[..] else {
            panic!("two procedures expected");
        };
        assert_eq!(
            marks_recovery(first.traits.as_deref()),
            expected,
            "{text:?}"
        );
        assert!(!marks_recovery(other.traits.as_deref()), "{text:?}");
        assert_eq!(marks_recovery(file.traits.as_deref()), expected, "{text:?}");
    }
}

#[test]
fn a_recovery_outside_any_procedure_marks_the_file_alone() {
    let file = report_of("Sub Fine()\nEnd Sub\nEnd Sub\nSub Later()\nEnd Sub\n");

    assert_eq!(names(module(&file)), [("Fine", false), ("Later", false)]);
    assert!(marks_recovery(file.traits.as_deref()));
}

#[test]
fn the_procedures_after_one_that_broke_are_read_on() {
    let file = report_of("Sub Broken()\n    x = 1\nSub Fine()\nEnd Sub\n");

    assert_eq!(names(module(&file)), [("Broken", true), ("Fine", false)]);
    assert_eq!(module(&file).procedures[0].end_line, 2);
}

#[test]
fn the_trait_is_the_one_that_the_rules_tie_to_the_parse_recovered_predicate() {
    let complexity = RulesSources::EMBEDDED
        .complexity
        .replace("compile_error_suspected", "does_not_parse");
    let sources = RulesSources {
        complexity: &complexity,
        ..RulesSources::EMBEDDED
    };
    let rules = RuleSet::load(&sources).expect("the changed rules load");
    let broken = vec![source("Module1", ModuleType::Standard, "Sub Broken()\n")];

    let file = checked(analyze_modules(
        broken,
        "Module1.bas",
        &Options::new(&rules),
    ));

    let has =
        |traits: &Option<Vec<String>>| (traits.iter().flatten()).any(|t| t == "does_not_parse");
    assert!(has(&module(&file).procedures[0].traits));
    assert!(has(&file.traits));
    assert!(!marks_recovery(file.traits.as_deref()));
}

#[test]
fn the_fields_that_no_stage_fills_yet_are_left_out_not_made_up() {
    let file = report_of(HELLO);

    let hello = &module(&file).procedures[0];
    assert_eq!(hello.is_entry_point, None);
    assert_eq!(hello.is_event_handler, None);
    assert_eq!(hello.possibly_unused, None);
    assert_eq!(hello.calls, None);
    assert_eq!(hello.apis, None);
}

/// A module that declares, outside any procedure, three `Declare` statements
/// (two forms of one routine, under `#If VBA7`, and another), two `WithEvents`
/// variables and both options. A `Declare` in a procedure is not a declaration.
const DECLARING: &str = "\
Option Explicit
Option VBASupport 1
#If VBA7 Then
Private Declare PtrSafe Function GetTickCount Lib \"kernel32\" () As Long
#Else
Private Declare Function GetTickCount Lib \"kernel32\" () As Long
#End If
Public Declare PtrSafe Sub Sleep Lib \"kernel32\" (ByVal ms As Long)
Private WithEvents App As Application
Public WithEvents Button As MSForms.CommandButton
Dim Total As Long

Sub Run()
    Declare Sub Inside Lib \"kernel32\" ()
End Sub
";

fn declarations(module: &ModuleReport) -> (Option<u32>, Option<u32>, Option<bool>, Option<bool>) {
    let declarations = module.declarations.as_ref().expect("the declarations");
    (
        declarations.declare_count,
        declarations.with_events_count,
        declarations.option_explicit,
        declarations.option_vba_support,
    )
}

#[test]
fn a_module_reports_the_declarations_outside_its_procedures() {
    let file = report_of(DECLARING);

    let module = module(&file);
    assert_eq!(
        declarations(module),
        (Some(3), Some(2), Some(true), Some(true))
    );
    // The metric of the module is the same count of `Declare` statements.
    assert_eq!(module.metrics.declare_count, Some(3));
}

#[test]
fn the_external_dependencies_of_a_file_are_left_out_whatever_its_declares_say() {
    let file = report_of(DECLARING);

    // A `Declare` statement is a dependency, but the ProgIDs that the statements
    // pass to `CreateObject` and `GetObject` are not read yet, so a count would
    // fall short.
    let metrics = file.metrics.as_ref().expect("the metrics of the file");
    assert_eq!(metrics.declare_count, Some(3));
    assert_eq!(metrics.external_dependencies, None);
}

#[test]
fn a_module_without_declarations_says_so_and_does_not_leave_them_out() {
    let file = report_of("Sub Run()\nEnd Sub\n");

    let module = module(&file);
    assert_eq!(
        declarations(module),
        (Some(0), Some(0), Some(false), Some(false))
    );
    assert_eq!(module.metrics.declare_count, Some(0));
}

#[test]
fn the_declare_count_of_a_file_sums_those_of_its_modules() {
    let file = report_of_modules(vec![
        source("Api", ModuleType::Standard, DECLARING),
        source("Hello", ModuleType::Standard, HELLO),
        source(
            "Other",
            ModuleType::Standard,
            "Declare Sub Beep Lib \"user32\" ()\n",
        ),
    ]);

    let counts: Vec<_> = (modules(&file).iter())
        .map(|module| module.metrics.declare_count)
        .collect();
    assert_eq!(counts, [Some(3), Some(0), Some(1)]);
    let metrics = file.metrics.as_ref().expect("the metrics of the file");
    assert_eq!(metrics.declare_count, Some(4));
}

#[test]
fn a_module_that_was_not_read_has_no_declarations() {
    let big = "a\n".repeat(MAX_LINES as usize + 1);

    let file = report_of_modules(vec![
        source("Big", ModuleType::Standard, &big),
        source("Small", ModuleType::Standard, HELLO),
    ]);

    let [big, small] = modules(&file) else {
        panic!("two modules expected");
    };
    assert_eq!(
        (&big.declarations, big.metrics.declare_count),
        (&None, None)
    );
    assert_eq!(
        declarations(small),
        (Some(0), Some(0), Some(true), Some(false))
    );
}

#[test]
fn analyze_reads_the_procedures_of_the_modules_of_a_workbook() {
    let bytes = corpus::read("fixtures/generated/01-01-hide-unhide-columns-find.xlsm");

    let file = checked(analyze(&bytes, "01-01.xlsm", &options()));

    assert_eq!((file.container, file.status), (Container::Xlsm, Status::Ok));
    assert_eq!(file.errors, Some(Vec::new()));
    let found: Vec<_> = procedures(&file)
        .map(|p| (p.name.as_str(), p.start_line))
        .collect();
    // Excel's export of the module has them at these lines.
    assert_eq!(found, [("Hide_Columns", 2), ("Unhide_Columns", 34)]);
    let metrics = file.metrics.expect("the metrics of the file");
    assert_eq!(metrics.procedures, Some(2));
    assert_eq!(metrics.forms, Some(0));
}

#[test]
fn the_status_of_a_workbook_follows_its_procedures_its_macro_sheets_and_its_encryption() {
    for (file, status) in [
        ("none-01-no-macros.xlsm", Status::NoMacros),
        ("xlm-01-macrosheet.xlsm", Status::Ok),
        ("xlm-01-macrosheet.xls", Status::Ok),
        ("enc-02-open-password.xls", Status::Ok),
        ("enc-01-open-password.xlsm", Status::Encrypted),
    ] {
        let bytes = corpus::read(&format!("fixtures/generated/{file}"));

        assert_eq!(
            checked(analyze(&bytes, file, &options())).status,
            status,
            "{file}"
        );
    }
}
