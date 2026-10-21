// SPDX-License-Identifier: Apache-2.0
//! What the front end makes of the modules of a file (SPEC M2): their
//! procedures with spans and sizes, the status that follows from them, and the
//! trait of what did not parse cleanly. The modules come as text, through
//! `analyze_modules`, or from a workbook, through `analyze`.

use macroport_core::analyze::{Options, analyze, analyze_modules};
use macroport_core::report::{
    Container, FileError, FileReport, ModuleReport, ProcedureKind, Status, Visibility,
};
use macroport_core::rules::{ModuleType, RuleSet, RulesSources};
use macroport_core::vba::{MAX_LINES, ModuleSource};
use macroport_testkit::corpus;
use sha2::{Digest, Sha256};

use crate::support::{RULES, report_problems_of};

const HELLO: &str = "Option Explicit\n\nSub Hello()\n    MsgBox \"hi\"\nEnd Sub\n";

fn options() -> Options<'static> {
    Options::new(&RULES)
}

fn sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn source(name: &str, module_type: ModuleType, text: &str) -> ModuleSource {
    ModuleSource {
        name: name.to_owned(),
        module_type,
        text: text.to_owned(),
    }
}

/// `file`, once checked against the report schema.
fn checked(file: FileReport) -> FileReport {
    assert_eq!(report_problems_of(&file), Vec::<String>::new());
    file
}

/// The report of `modules` as a text file.
fn report_of_modules(modules: Vec<ModuleSource>) -> FileReport {
    checked(analyze_modules(modules, "Book.bas", &options()))
}

/// The report of `text` as the only module of a file.
fn report_of(text: &str) -> FileReport {
    report_of_modules(vec![source("Module1", ModuleType::Standard, text)])
}

fn modules(file: &FileReport) -> &[ModuleReport] {
    &file.vba_project.as_ref().expect("a VBA project").modules
}

/// The only module of a file.
fn module(file: &FileReport) -> &ModuleReport {
    let modules = modules(file);
    assert_eq!(modules.len(), 1);
    modules.first().expect("one module")
}

/// The names of the procedures of `module`, with whether each has a trait.
fn names(module: &ModuleReport) -> Vec<(&str, bool)> {
    (module.procedures.iter())
        .map(|procedure| (procedure.name.as_str(), procedure.traits.is_some()))
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
    let modules = vec![source("Module1", ModuleType::Standard, HELLO)];

    let file = checked(analyze_modules(modules, "Module1.bas", &limited));

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
    let modules = modules(&file);
    assert!(modules[0].source_available);
    assert_eq!(
        (modules[0].procedures.len(), modules[0].metrics.loc),
        (0, 0)
    );
    assert_eq!(modules[1].procedures.len(), 1);
}

#[test]
fn a_procedure_that_did_not_parse_cleanly_gets_the_trait_compile_error_suspected() {
    for (text, expected) in [
        ("Sub Broken()\n    x = 1\n", true),
        ("Sub Broken()\n    x = 1\nEnd Function\n", true),
        ("Sub (a, b)\nEnd Sub\n", true),
        ("Sub Fine()\n    x = 1\nEnd Sub\n", false),
    ] {
        let file = report_of(text);

        assert_eq!(file.status, Status::Ok, "{text:?}");
        let first = &module(&file).procedures[0];
        let traits = expected.then(|| vec!["compile_error_suspected".to_owned()]);
        assert_eq!(first.traits, traits, "{text:?}");
        assert_eq!(file.traits, traits, "{text:?}");
    }
}

#[test]
fn a_recovery_outside_any_procedure_marks_the_file_alone() {
    let file = report_of("Sub Fine()\nEnd Sub\nEnd Sub\nSub Later()\nEnd Sub\n");

    assert_eq!(names(module(&file)), [("Fine", false), ("Later", false)]);
    assert_eq!(
        file.traits,
        Some(vec!["compile_error_suspected".to_owned()])
    );
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
    let modules = vec![source("Module1", ModuleType::Standard, "Sub Broken()\n")];

    let file = analyze_modules(modules, "Module1.bas", &Options::new(&rules));

    let expected = Some(vec!["does_not_parse".to_owned()]);
    assert_eq!(module(&file).procedures[0].traits, expected);
    assert_eq!(file.traits, expected);
}

#[test]
fn the_fields_that_later_stages_fill_are_left_out_not_made_up() {
    let file = report_of(HELLO);

    let hello = &module(&file).procedures[0];
    assert_eq!(hello.is_entry_point, None);
    assert_eq!(hello.possibly_unused, None);
    assert_eq!(hello.calls, None);
    assert_eq!(hello.apis, None);
    assert_eq!(hello.traits, None);
    assert_eq!(hello.primary_family, "unclassified");
    assert!(hello.families.is_empty() && hello.signals.is_empty());
    assert_eq!(module(&file).declarations, None);
    assert_eq!(file.traits, None);
}

#[test]
fn the_modules_of_a_workbook_are_read_like_those_of_a_text() {
    let bytes = corpus::read("fixtures/generated/01-01-hide-unhide-columns-find.xlsm");

    let file = checked(analyze(&bytes, "01-01.xlsm", &options()));

    assert_eq!((file.container, file.status), (Container::Xlsm, Status::Ok));
    assert_eq!(file.errors, Some(Vec::new()));
    let found: Vec<_> = (modules(&file).iter())
        .flat_map(|module| &module.procedures)
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
