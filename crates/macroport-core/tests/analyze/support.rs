// SPDX-License-Identifier: Apache-2.0
//! Running `analyze()` on bytes, and checking what it reports.

use std::sync::LazyLock;

use macroport_core::analyze::{Extraction, Options, extract};
use macroport_core::report::{FileReport, Lang, ModuleReport, Report, Signal};
use macroport_core::rules::{RuleSet, RulesSources, Severity};
use macroport_testkit::{corpus, report_problems};
use sha2::{Digest, Sha256};

/// The rules that ship with the tool, loaded once.
pub static RULES: LazyLock<RuleSet> = LazyLock::new(|| {
    RuleSet::load(&RulesSources::EMBEDDED).expect("the rules that ship with the tool load")
});

/// A module of one procedure.
pub const HELLO: &str = "Option Explicit\n\nSub Hello()\n    MsgBox \"hi\"\nEnd Sub\n";

/// The default options, with the rules that ship with the tool.
pub fn options() -> Options<'static> {
    Options::new(&RULES)
}

/// The SHA-256 of `bytes` as lowercase hexadecimal digits.
pub fn sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// The extraction of `bytes`, a file named `path`, with the default options.
pub fn extraction(bytes: &[u8], path: &str) -> Extraction {
    extraction_with(bytes, path, &Options::new(&RULES))
}

/// The extraction of the file `file` of `corpus/fixtures/`.
pub fn fixture(file: &str) -> Extraction {
    extraction(&corpus::read(&format!("fixtures/{file}")), file)
}

/// The codes of the errors of `report`, in order.
pub fn codes(report: &FileReport) -> Vec<&str> {
    let errors = report.errors.iter().flatten();
    errors.map(|error| error.code.as_str()).collect()
}

/// Whether `traits` has the trait that the shipped rules give to what did not
/// parse cleanly, the one tied to the predicate `parse_recovered`. The rule
/// engine gives every trait of a procedure or a file, so the others come with
/// it.
pub fn suspected(traits: &Option<Vec<String>>) -> bool {
    (traits.iter().flatten()).any(|id| id == "compile_error_suspected")
}

/// The signal `id` of `severity` that a container indicator makes: it is the
/// file's, with no module and no evidence.
pub fn file_signal(id: &str, severity: Severity) -> Signal {
    Signal {
        signal_id: id.to_owned(),
        severity,
        module: None,
        procedure: None,
        evidence: None,
    }
}

/// The extraction of `bytes`, a file named `path`, with these options.
///
/// Fails the test unless the report is valid against the report schema and
/// the sources go with the modules.
pub fn extraction_with(bytes: &[u8], path: &str, options: &Options<'_>) -> Extraction {
    let extraction = extract(bytes, path, options);
    assert_eq!(report_problems_of(&extraction.file), Vec::<String>::new());
    let modules = extraction
        .file
        .vba_project
        .as_ref()
        .map_or(0, |project| project.modules.len());
    assert_eq!(extraction.sources.len(), modules, "one source per module");
    extraction
}

/// The problems of `file` against the report schema, as the first file of a
/// fleet report.
pub fn report_problems_of(file: &FileReport) -> Vec<String> {
    let mut report = Report::empty(&RULES, "1970-01-01T00:00:00Z".to_owned(), Lang::En);
    report.files = vec![file.clone()];
    report_problems(&report.to_json().expect("the report serialises"))
}

/// `file`, once checked against the report schema.
pub fn checked(file: FileReport) -> FileReport {
    assert_eq!(report_problems_of(&file), Vec::<String>::new());
    file
}

/// The modules of the VBA project of `file`.
pub fn modules(file: &FileReport) -> &[ModuleReport] {
    &file.vba_project.as_ref().expect("a VBA project").modules
}

/// The only module of `file`.
pub fn module(file: &FileReport) -> &ModuleReport {
    let modules = modules(file);
    assert_eq!(modules.len(), 1);
    modules.first().expect("one module")
}
