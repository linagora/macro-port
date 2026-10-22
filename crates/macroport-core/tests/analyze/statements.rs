// SPDX-License-Identifier: Apache-2.0
//! What the statements of the modules add to the report of a file (SPEC M2
//! items 3 and 4): the external dependencies that their ProgIDs make.

use macroport_core::report::FileReport;
use macroport_core::rules::ModuleType;

use crate::support::{HELLO, report_of_modules, source};

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
