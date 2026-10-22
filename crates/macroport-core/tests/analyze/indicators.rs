// SPDX-License-Identifier: Apache-2.0
//! The container indicators that extraction raises, and the signals that the
//! rule engine makes of them in the report with the severities of the rules.
//! Extraction gives the ids and leaves the signals out. The indicator of a
//! module that lost its source is in `failures.rs`, which makes such a module.

use macroport_core::analyze::{Options, analyze};
use macroport_core::report::{FileReport, Status};
use macroport_core::rules::Severity;
use macroport_testkit::corpus;

use crate::support::{RULES, checked, extraction, extraction_with, file_signal, options};

/// The report of `bytes`, a file named `path`, once checked against the schema.
fn analysed(bytes: &[u8], path: &str) -> FileReport {
    checked(analyze(bytes, path, &options()))
}

/// The ids of the signals of the file `report`.
fn signal_ids(report: &FileReport) -> Vec<&str> {
    let signals = report.signals.iter().flatten();
    signals.map(|signal| signal.signal_id.as_str()).collect()
}

#[test]
fn macro_sheets_raise_their_indicator_in_a_package_and_in_a_workbook() {
    for file in [
        "generated/xlm-01-macrosheet.xlsm",
        "generated/xlm-01-macrosheet.xls",
    ] {
        let bytes = corpus::read(&format!("fixtures/{file}"));

        let extraction = extraction(&bytes, file);

        assert_eq!(extraction.indicators, ["sec.xlm-macrosheet"], "{file}");
        // The security stage makes the signal: extraction does not.
        let report = &extraction.file;
        assert_eq!(
            (&report.signals, report.max_severity),
            (&None, None),
            "{file}"
        );
        let report = analysed(&bytes, file);
        let signals = vec![file_signal("sec.xlm-macrosheet", Severity::Medium)];
        assert_eq!(report.signals, Some(signals), "{file}");
        assert_eq!(report.max_severity, Some(Severity::Medium), "{file}");
    }
}

#[test]
fn a_file_with_macro_sheets_and_no_module_has_the_signal_alone() {
    let bytes = corpus::read("fixtures/generated/xlm-01-macrosheet.xlsm");

    let report = analysed(&bytes, "xlm.xlsm");

    // No module was read: the families, the level and the traits of an empty
    // project would be made up, so they are left out.
    assert!(report.vba_project.is_none());
    assert_eq!(report.status, Status::Ok);
    assert_eq!(
        (
            &report.complexity_level,
            &report.primary_family,
            &report.families,
            &report.traits,
            &report.metrics
        ),
        (&None, &None, &None, &None, &None)
    );
}

#[test]
fn a_vba_project_in_a_package_declared_without_macros_raises_its_indicator() {
    let bytes = corpus::read("fixtures/malformed/macros-in-xlsx.xlsx");

    for path in ["a.xlsx", "a.XLTX"] {
        let extraction = extraction(&bytes, path);

        assert_eq!(extraction.file.status, Status::Ok, "{path}");
        assert_eq!(extraction.indicators, ["sec.vbaproject-in-xlsx"], "{path}");
        let report = &extraction.file;
        assert_eq!(
            (&report.signals, report.max_severity),
            (&None, None),
            "{path}"
        );
        let report = analysed(&bytes, path);
        let signals = vec![file_signal("sec.vbaproject-in-xlsx", Severity::High)];
        assert_eq!(report.signals, Some(signals), "{path}");
        assert_eq!(report.max_severity, Some(Severity::High), "{path}");
    }
}

#[test]
fn the_same_package_declared_with_macros_raises_nothing() {
    let bytes = corpus::read("fixtures/malformed/macros-in-xlsx.xlsx");

    for path in ["a.xlsm", "a.xltm", "a.xlam", "a.xlsb", "a"] {
        let extraction = extraction(&bytes, path);

        let report = &extraction.file;
        assert!(extraction.indicators.is_empty(), "{path}");
        assert_eq!(
            (&report.signals, report.max_severity),
            (&None, None),
            "{path}"
        );
        let report = analysed(&bytes, path);
        assert!(
            !signal_ids(&report).contains(&"sec.vbaproject-in-xlsx"),
            "{path}"
        );
    }
}

#[test]
fn a_zip_bomb_raises_the_indicator_of_a_suspicious_archive() {
    let bytes = corpus::read("fixtures/malformed/zip-bomb.xlsm");

    let extraction = extraction(&bytes, "zip-bomb.xlsm");

    assert_eq!(extraction.file.status, Status::SuspiciousArchive);
    assert_eq!(extraction.indicators, ["sec.suspicious-archive"]);
    let report = &extraction.file;
    assert_eq!((&report.signals, report.max_severity), (&None, None));
    let report = analysed(&bytes, "zip-bomb.xlsm");
    assert_eq!(report.status, Status::SuspiciousArchive);
    let signals = vec![file_signal("sec.suspicious-archive", Severity::Medium)];
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
        assert_eq!(
            (&report.signals, report.max_severity),
            (&None, None),
            "{file}"
        );
        // Nothing was judged either: no signal is not an empty list of signals.
        let report = analysed(&bytes, file);
        assert_eq!(
            (&report.signals, report.max_severity),
            (&None, None),
            "{file}"
        );
    }
}

#[test]
fn the_rules_give_the_severity_of_a_signal_and_an_unlisted_indicator_makes_none() {
    let bytes = corpus::read("fixtures/generated/xlm-01-macrosheet.xlsm");
    let mut rules = RULES.clone();
    for indicator in &mut rules.security.container_indicators {
        indicator.severity = Severity::Low;
    }

    let raised = checked(analyze(&bytes, "a.xlsm", &Options::new(&rules)));

    let signals = vec![file_signal("sec.xlm-macrosheet", Severity::Low)];
    assert_eq!(raised.signals, Some(signals));
    assert_eq!(raised.max_severity, Some(Severity::Low));

    rules.security.container_indicators.clear();
    let silent = checked(analyze(&bytes, "a.xlsm", &Options::new(&rules)));

    // The extraction still raises the indicator, and the rules give it no severity:
    // the engine ran, and found no signal.
    let extraction = extraction_with(&bytes, "a.xlsm", &Options::new(&rules));
    assert_eq!(extraction.indicators, ["sec.xlm-macrosheet"]);
    assert_eq!(
        (&silent.signals, silent.max_severity),
        (&Some(Vec::new()), None)
    );
}
