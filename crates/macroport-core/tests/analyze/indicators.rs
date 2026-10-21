// SPDX-License-Identifier: Apache-2.0
//! The container indicators that extraction raises, and the signals they make
//! in the report with the severities of the rules. The indicator of a module
//! that lost its source is in `failures.rs`, which makes such a module.

use macroport_core::analyze::Options;
use macroport_core::report::{Signal, Status};
use macroport_core::rules::Severity;
use macroport_testkit::corpus;

use crate::support::{RULES, extraction, extraction_with, fixture};

fn signal(id: &str, severity: Severity) -> Signal {
    Signal {
        signal_id: id.to_owned(),
        severity,
        module: None,
        procedure: None,
        evidence: None,
    }
}

#[test]
fn macro_sheets_raise_their_indicator_in_a_package_and_in_a_workbook() {
    for file in [
        "generated/xlm-01-macrosheet.xlsm",
        "generated/xlm-01-macrosheet.xls",
    ] {
        let extraction = fixture(file);

        assert_eq!(extraction.indicators, ["sec.xlm-macrosheet"], "{file}");
        let report = &extraction.file;
        let signals = vec![signal("sec.xlm-macrosheet", Severity::Medium)];
        assert_eq!(report.signals, Some(signals), "{file}");
        assert_eq!(report.max_severity, Some(Severity::Medium), "{file}");
    }
}

#[test]
fn a_vba_project_in_a_package_declared_without_macros_raises_its_indicator() {
    let bytes = corpus::read("fixtures/malformed/macros-in-xlsx.xlsx");

    for path in ["a.xlsx", "a.XLTX"] {
        let extraction = extraction(&bytes, path);

        assert_eq!(extraction.file.status, Status::Ok, "{path}");
        assert_eq!(extraction.indicators, ["sec.vbaproject-in-xlsx"], "{path}");
        let signals = vec![signal("sec.vbaproject-in-xlsx", Severity::High)];
        assert_eq!(extraction.file.signals, Some(signals), "{path}");
        assert_eq!(extraction.file.max_severity, Some(Severity::High), "{path}");
    }
}

#[test]
fn the_same_package_declared_with_macros_raises_nothing() {
    let bytes = corpus::read("fixtures/malformed/macros-in-xlsx.xlsx");

    for path in ["a.xlsm", "a.xltm", "a.xlam", "a.xlsb", "a"] {
        let extraction = extraction(&bytes, path);

        let report = &extraction.file;
        assert!(extraction.indicators.is_empty(), "{path}");
        assert_eq!((&report.signals, report.max_severity), (&None, None));
    }
}

#[test]
fn a_zip_bomb_raises_the_indicator_of_a_suspicious_archive() {
    let bytes = corpus::read("fixtures/malformed/zip-bomb.xlsm");

    let extraction = extraction(&bytes, "zip-bomb.xlsm");

    let report = &extraction.file;
    assert_eq!(report.status, Status::SuspiciousArchive);
    assert_eq!(extraction.indicators, ["sec.suspicious-archive"]);
    let signals = vec![signal("sec.suspicious-archive", Severity::Medium)];
    assert_eq!(report.signals, Some(signals));
    assert_eq!(report.max_severity, Some(Severity::Medium));
}

#[test]
fn the_files_that_cannot_be_read_for_another_reason_raise_nothing() {
    for file in [
        "empty.xlsm",
        "random-bytes.xls",
        "text-renamed.xlsm",
        "truncated-half.xlsm",
        "cfb-bad-fat.xls",
    ] {
        let bytes = corpus::read(&format!("fixtures/malformed/{file}"));

        let extraction = extraction(&bytes, file);

        let report = &extraction.file;
        assert_ne!(report.status, Status::Ok, "{file}");
        assert_ne!(report.status, Status::SuspiciousArchive, "{file}");
        assert!(extraction.indicators.is_empty(), "{file}");
        assert_eq!((&report.signals, report.max_severity), (&None, None));
    }
}

#[test]
fn the_rules_give_the_severity_of_a_signal_and_can_switch_it_off() {
    let bytes = corpus::read("fixtures/generated/xlm-01-macrosheet.xlsm");
    let mut rules = RULES.clone();
    for indicator in &mut rules.security.container_indicators {
        indicator.severity = Severity::Low;
    }

    let raised = extraction_with(&bytes, "a.xlsm", &Options::new(&rules));

    let signals = vec![signal("sec.xlm-macrosheet", Severity::Low)];
    assert_eq!(raised.file.signals, Some(signals));
    assert_eq!(raised.file.max_severity, Some(Severity::Low));

    rules.security.container_indicators.clear();
    let silent = extraction_with(&bytes, "a.xlsm", &Options::new(&rules));

    // The extraction still raises the indicator, and the rules make no signal of it.
    assert_eq!(silent.indicators, ["sec.xlm-macrosheet"]);
    assert_eq!(
        (&silent.file.signals, silent.file.max_severity),
        (&None, None)
    );
}
