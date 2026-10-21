// SPDX-License-Identifier: Apache-2.0
//! A1, extraction completeness (ACCEPTANCE): each OOXML fixture of
//! `corpus/fixtures/fixtures.yaml` gives the modules that Excel put in it, and
//! the source of each equals the export that Excel made of it. The `.xls`
//! fixtures wait for the reading of compound workbooks.
//!
//! The fixtures are all saved in code page 1252. The ones for 1251 and 932
//! wait for their fixtures; `tests/ovba_source.rs` decodes both from bytes.

use std::collections::BTreeMap;

use encoding_rs::WINDOWS_1252;
use macroport_core::report::{ModuleReport, Status};
use macroport_core::rules::ModuleType;
use macroport_testkit::corpus;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::support::extraction;

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
    let is_package = |fixture: &&Fixture| !fixture.file.ends_with(".xls");
    for fixture in fixtures()
        .iter()
        .filter(|fixture| !fixture.modules.is_empty())
        .filter(is_package)
    {
        let file = &fixture.file;
        let extraction = extraction(&corpus::read(&format!("fixtures/{file}")), file);

        let report = &extraction.file;
        assert_eq!(report.status, Status::Ok, "{file}");
        let extension = file.rsplit('.').next().unwrap_or_default();
        let container = serde_json::to_value(report.container).expect("serialises");
        assert_eq!(container, extension, "{file}");
        let project = report.vba_project.as_ref().expect("a VBA project");
        assert_eq!(project.code_page, fixture.code_page, "{file}");
        assert_eq!(project.protected, fixture.expected_protected, "{file}");
        let sources = extraction.sources.iter().map(Option::as_deref);
        let found: BTreeMap<&str, (&ModuleReport, Option<&str>)> = (project.modules.iter())
            .zip(sources)
            .map(|(module, source)| (module.name.as_str(), (module, source)))
            .collect();
        assert_eq!(found.len(), fixture.modules.len(), "{file}");
        for expected in &fixture.modules {
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
    // Every export of an OOXML fixture is compared, none is skipped.
    let exports = corpus::files("fixtures/generated", &["bas", "cls"]);
    let packages = exports
        .iter()
        .filter(|export| !export.contains(".xls.expected."));
    assert_eq!(compared, packages.count());
}

#[test]
fn a_workbook_without_a_vba_project_has_no_macros() {
    let file = "generated/none-01-no-macros.xlsm";
    let report = extraction(&corpus::read(&format!("fixtures/{file}")), file).file;
    let found = (report.status, report.vba_project, report.errors);
    assert_eq!(found, (Status::NoMacros, None, Some(Vec::new())));
}
