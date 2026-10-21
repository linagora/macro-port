// SPDX-License-Identifier: Apache-2.0
//! A1, extraction completeness (ACCEPTANCE): each fixture of
//! `corpus/fixtures/fixtures.yaml` gives the modules that Excel put in it, and
//! the source of each equals the export that Excel made of it. The fixtures
//! that expect something else, such as a status or macro sheets, have it.
//!
//! The fixtures are all saved in code page 1252. The ones for 1251 and 932
//! wait for their fixtures; `tests/ovba_source.rs` decodes both from bytes.

use std::collections::BTreeMap;

use encoding_rs::WINDOWS_1252;
use macroport_core::report::{Container, ModuleReport, Status, Xlm};
use macroport_core::rules::ModuleType;
use macroport_testkit::corpus;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::support::{codes, fixture};

/// The keys of `fixtures.yaml` that these tests use.
#[derive(Deserialize)]
struct Fixtures {
    fixtures: Vec<Fixture>,
}

#[derive(Deserialize)]
struct Fixture {
    file: String,
    code_page: Option<u16>,
    #[serde(default)]
    expected_protected: bool,
    expected_status: Option<Status>,
    expected_workbook_encrypted: Option<bool>,
    expected_module: Option<String>,
    expected_xlm: Option<Xlm>,
    #[serde(default)]
    modules: Vec<ExpectedModule>,
}

#[derive(Deserialize)]
struct ExpectedModule {
    name: String,
    #[serde(rename = "type")]
    module_type: ModuleType,
    expected_source: Option<String>,
}

fn fixtures() -> Vec<Fixture> {
    let text = String::from_utf8(corpus::read("fixtures/fixtures.yaml")).expect("UTF-8");
    // The file starts with a byte order mark.
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    serde_saphyr::from_str::<Fixtures>(text)
        .expect("the fixtures file reads")
        .fixtures
}

/// The lines of a source as A1 compares them: line endings and trailing
/// whitespace gone, the header lines of an export (`VERSION`, the
/// `BEGIN` ... `END` block, `Attribute VB_*`) ignored, and no blank line at
/// the end.
fn comparable(source: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut in_block = false;
    for line in source.lines().map(str::trim_end) {
        if in_block {
            in_block = line != "END";
        } else if line == "BEGIN" {
            in_block = true;
        } else if !line.starts_with("VERSION ") && !line.starts_with("Attribute VB_") {
            lines.push(line);
        }
    }
    while lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    lines
}

#[test]
fn the_fixtures_give_the_modules_and_the_sources_that_excel_wrote() {
    let mut compared = 0;
    for entry in fixtures().iter().filter(|entry| !entry.modules.is_empty()) {
        let file = &entry.file;
        let extraction = fixture(file);

        let report = &extraction.file;
        assert_eq!(report.status, Status::Ok, "{file}");
        assert_eq!(report.workbook_encrypted, Some(false), "{file}");
        let no_sheets = Xlm {
            present: false,
            macro_sheets: Some(0),
        };
        assert_eq!(report.xlm, Some(no_sheets), "{file}");
        let extension = file.rsplit('.').next().unwrap_or_default();
        let container = serde_json::to_value(report.container).expect("serialises");
        assert_eq!(container, extension, "{file}");
        let project = report.vba_project.as_ref().expect("a VBA project");
        assert_eq!(project.code_page, entry.code_page, "{file}");
        assert_eq!(project.protected, entry.expected_protected, "{file}");
        let sources = extraction.sources.iter().map(Option::as_deref);
        let found: BTreeMap<&str, (&ModuleReport, Option<&str>)> = (project.modules.iter())
            .zip(sources)
            .map(|(module, source)| (module.name.as_str(), (module, source)))
            .collect();
        assert_eq!(found.len(), entry.modules.len(), "{file}");
        for expected in &entry.modules {
            let name = expected.name.as_str();
            let (module, source) = found[name];
            let source = source.expect("a source");
            assert_eq!(module.module_type, expected.module_type, "{file}, {name}");
            let digest = Sha256::digest(source);
            let hash: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
            assert!(module.source_available, "{file}, {name}");
            assert_eq!(module.source_sha256.as_deref(), Some(hash.as_str()));
            assert_eq!(module.stream_name.as_deref(), Some(name), "{file}");
            let Some(export) = &expected.expected_source else {
                continue;
            };
            let export = corpus::read(&format!("fixtures/{export}"));
            let export = WINDOWS_1252.decode_without_bom_handling(&export).0;
            assert_eq!(comparable(source), comparable(&export), "{file}, {name}");
            compared += 1;
        }
    }
    // Every export is compared, none is skipped.
    let exports = corpus::files("fixtures/generated", &["bas", "cls"]);
    assert_eq!(compared, exports.len());
}

#[test]
fn the_fixtures_that_expect_something_else_have_it() {
    for entry in fixtures().iter().filter(|entry| entry.modules.is_empty()) {
        let file = &entry.file;
        let report = fixture(file).file;

        if let Some(status) = entry.expected_status {
            assert_eq!(report.status, status, "{file}");
        }
        if let Some(encrypted) = entry.expected_workbook_encrypted {
            assert_eq!(report.workbook_encrypted, Some(encrypted), "{file}");
        }
        if let Some(xlm) = &entry.expected_xlm {
            assert_eq!(report.xlm.as_ref(), Some(xlm), "{file}");
            assert_eq!(report.status, Status::Ok, "{file}: macro sheets");
        }
        if let Some(name) = &entry.expected_module {
            let project = report.vba_project.as_ref().expect("a VBA project");
            let module = project.modules.iter().find(|module| module.name == *name);
            assert!(
                module.is_some_and(|module| module.source_available),
                "{file}"
            );
        }
    }
}

#[test]
fn an_encrypted_package_is_reported_and_not_opened() {
    let report = fixture("generated/enc-01-open-password.xlsm").file;

    assert_eq!(report.status, Status::Encrypted);
    assert_eq!(report.container, Container::Xlsm);
    assert!(report.vba_project.is_none());
    assert_eq!((&report.xlm, report.workbook_encrypted), (&None, None));
    assert_eq!(codes(&report), ["file.encrypted_package"]);
}

#[test]
fn a_workbook_without_a_vba_project_has_no_macros() {
    let report = fixture("generated/none-01-no-macros.xlsm").file;
    let found = (report.status, report.vba_project, report.errors);
    assert_eq!(found, (Status::NoMacros, None, Some(Vec::new())));
}
