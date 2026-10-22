// SPDX-License-Identifier: Apache-2.0
//! What the statements of the modules add to the report of a file (SPEC M2
//! items 3 and 4): the external dependencies that their ProgIDs make, and the
//! statements that nest too deep to be read in full.

use macroport_core::report::{FileReport, Status};
use macroport_core::rules::ModuleType;
use macroport_core::vba::MAX_NESTING;

use crate::support::{
    HELLO, codes, module, procedures, recovery_traits, report_of_modules, source,
};

/// The report of a text file made of the standard modules `(name, text)`.
fn report_of(modules: &[(&str, &str)]) -> FileReport {
    let modules = (modules.iter())
        .map(|&(name, text)| source(name, ModuleType::Standard, text))
        .collect();
    report_of_modules(modules)
}

/// The external dependencies that the metrics of `file` count.
fn dependencies(file: &FileReport) -> Option<u32> {
    (file.metrics.as_ref()).and_then(|metrics| metrics.external_dependencies)
}

/// A statement that nests `levels` groups.
fn nested(levels: usize) -> String {
    format!("x = {}1{}", "(".repeat(levels), ")".repeat(levels))
}

/// A statement that nests groups past what the reader follows.
fn too_deep() -> String {
    nested(MAX_NESTING + 1)
}

/// The text of a module whose procedures are too many for a file, followed by
/// `rest`. Each `Sub` line ends the procedure before it, so a line is one.
fn too_many_procedures(rest: &str) -> String {
    format!("{}{rest}", "Sub A\n".repeat(50_001))
}

/// The messages of the errors of `file`.
fn messages(file: &FileReport) -> Vec<&str> {
    let errors = file.errors.iter().flatten();
    errors.map(|error| error.message.as_str()).collect()
}

#[test]
fn the_progids_of_all_the_modules_count_once_each_for_the_file() {
    let file = report_of(&[
        (
            "First",
            "Sub A()\n    Set s = CreateObject(\"WScript.Shell\")\n\
             \x20   Set w = GetObject(, \"Word.Application\")\nEnd Sub\n",
        ),
        (
            "Second",
            "Sub B()\n    Set s = CreateObject(\"wscript.SHELL\")\n\
             \x20   Set x = CreateObject(\"MSXML2.XMLHTTP\")\nEnd Sub\n",
        ),
    ]);

    // The shell, met in both modules whatever its letter case, once; the
    // word processor, in the first; the HTTP client, in the second.
    assert_eq!(dependencies(&file), Some(3));
}

#[test]
fn a_progid_that_folds_from_a_constant_counts() {
    let file = report_of(&[(
        "Folded",
        "Private Const Suite = \"Outlook.\"\n\
         Sub A()\n    Set o = CreateObject(Suite & \"Application\")\nEnd Sub\n",
    )]);

    assert_eq!(dependencies(&file), Some(1));
}

#[test]
fn what_is_no_external_class_counts_for_nothing() {
    let file = report_of(&[(
        "Common",
        "Sub A()\n    Set d = CreateObject(\"Scripting.Dictionary\")\n\
         \x20   Set r = CreateObject(\"VBScript.RegExp\")\n\
         \x20   Set h = CreateObject(\"htmlfile\")\n\
         \x20   Set b = GetObject(\"C:\\Reports\\book.xlsx\")\nEnd Sub\n",
    )]);

    assert_eq!(dependencies(&file), Some(0));
}

#[test]
fn the_progids_and_the_declares_add_up_to_the_dependencies_of_the_file() {
    let file = report_of(&[
        (
            "Native",
            "Declare Sub Sleep Lib \"kernel32\" (ByVal ms As Long)\n\
             Declare Function GetTickCount Lib \"kernel32\" () As Long\n",
        ),
        (
            "Late",
            "Sub A()\n    Set s = CreateObject(\"WScript.Shell\")\n\
             \x20   Set x = CreateObject(\"MSXML2.XMLHTTP\")\nEnd Sub\n",
        ),
    ]);

    // Two classes, and one for the `Declare` statements, however many.
    assert_eq!(dependencies(&file), Some(3));
}

#[test]
fn a_file_with_no_class_and_no_declare_has_no_dependency() {
    assert_eq!(dependencies(&report_of(&[("Hello", HELLO)])), Some(0));
}

#[test]
fn a_statement_nested_too_deep_is_told_once_by_its_module_and_changes_no_status() {
    let deep = too_deep();
    let file = report_of(&[
        (
            "Deep",
            &format!("Sub A()\n    {deep}\n    {deep}\nEnd Sub\n"),
        ),
        ("Fine", HELLO),
        ("Later", &format!("Sub B()\nEnd Sub\n{deep}\n")),
    ]);

    assert_eq!(file.status, Status::Ok);
    assert_eq!(codes(&file), ["vba.statement.too_deep"; 2]);
    let past = format!("expressions deeper than {MAX_NESTING} levels");
    let [deep, later] = messages(&file)[..] else {
        panic!("two messages expected");
    };
    assert!(
        deep.starts_with(&format!(
            "module Deep: 2 statements nest {past}, the first at line 2;"
        )),
        "{deep}"
    );
    assert!(
        later.starts_with(&format!(
            "module Later: 1 statement nests {past}, the first at line 3;"
        )),
        "{later}"
    );
}

#[test]
fn a_procedure_with_a_statement_nested_too_deep_gets_the_trait_and_marks_the_file() {
    let file = report_of(&[(
        "Deep",
        &format!(
            "Sub Fine()\nEnd Sub\nSub Deep()\n    {}\nEnd Sub\n",
            too_deep()
        ),
    )]);

    let traits: Vec<_> = procedures(&file)
        .map(|procedure| (procedure.name.as_str(), procedure.traits.clone()))
        .collect();
    assert_eq!(traits, [("Fine", None), ("Deep", Some(recovery_traits()))]);
    assert_eq!(file.traits, Some(recovery_traits()));
}

#[test]
fn a_statement_nested_too_deep_outside_the_procedures_marks_the_file_alone() {
    let file = report_of(&[("Deep", &format!("Sub Fine()\nEnd Sub\n{}\n", too_deep()))]);

    assert!(procedures(&file).all(|procedure| procedure.traits.is_none()));
    assert_eq!(module(&file).procedures.len(), 1);
    assert_eq!(file.traits, Some(recovery_traits()));
    assert_eq!(codes(&file), ["vba.statement.too_deep"]);
}

#[test]
fn a_file_with_nothing_but_a_statement_nested_too_deep_is_still_without_macros() {
    let file = report_of(&[("Deep", &format!("{}\n", too_deep()))]);

    assert_eq!(file.status, Status::NoMacros);
    assert_eq!(codes(&file), ["vba.statement.too_deep"]);
}

#[test]
fn groups_that_nest_less_than_the_limit_are_no_error() {
    // The statement and its right operand are open too, so a few levels below
    // the limit are well within it.
    let statement = nested(MAX_NESTING - 4);

    let file = report_of(&[("Nested", &format!("Sub A()\n    {statement}\nEnd Sub\n"))]);

    assert_eq!(codes(&file), Vec::<&str>::new());
    assert_eq!(file.traits, None);
}

#[test]
fn the_failures_of_a_file_come_before_its_warnings_and_give_its_status() {
    let deep = too_deep();

    let file = report_of(&[
        ("Deep", &format!("Sub A()\n    {deep}\nEnd Sub\n")),
        ("Many", &too_many_procedures("")),
    ]);

    assert_eq!(file.status, Status::TooLarge);
    assert_eq!(
        codes(&file),
        ["vba.too_many_procedures", "vba.statement.too_deep"]
    );
}

#[test]
fn a_module_that_is_not_read_tells_nothing_of_its_statements() {
    let many = too_many_procedures(&too_deep());

    let file = report_of(&[("Many", &many), ("Fine", HELLO)]);

    assert_eq!(file.status, Status::TooLarge);
    assert_eq!(codes(&file), ["vba.too_many_procedures"]);
    assert_eq!(file.traits, None);
}
