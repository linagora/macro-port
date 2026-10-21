// SPDX-License-Identifier: Apache-2.0
//! Running `analyze()` on bytes, and checking what it reports.

use std::sync::LazyLock;

use macroport_core::analyze::{Extraction, Options, extract};
use macroport_core::report::{FileReport, Lang, Report};
use macroport_core::rules::{RuleSet, RulesSources};
use macroport_testkit::report_problems;

/// The rules that ship with the tool, loaded once.
pub static RULES: LazyLock<RuleSet> = LazyLock::new(|| {
    RuleSet::load(&RulesSources::EMBEDDED).expect("the rules that ship with the tool load")
});

/// The extraction of `bytes`, a file named `path`, with the default options.
pub fn extraction(bytes: &[u8], path: &str) -> Extraction {
    extraction_with(bytes, path, &Options::new(&RULES))
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
