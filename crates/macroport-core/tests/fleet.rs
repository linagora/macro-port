// SPDX-License-Identifier: Apache-2.0
//! The fleet report that `Report::from_files` builds: the files in the order of
//! their paths, the counts of the scan and the summary.

// The helpers are test code: a panic in one is a failed test. `clippy.toml`
// only exempts the bodies of `#[test]` functions.
#![allow(clippy::expect_used)]

use macroport_core::report::{FileReport, Lang, Report, Status};
use macroport_core::rules::{RuleSet, RulesSources};
use macroport_testkit::report_problems;

/// The fleet report that the renderer tests of the CLI use. Its summary was
/// written by hand from its files: eleven files, a status each, and a VBA
/// project in three of them, so it is the oracle of the aggregation.
const FLEET: &str = include_str!("../../macroport-cli/tests/fixtures/reports/fleet.json");

const GENERATED_AT: &str = "2026-10-20T08:00:00Z";

fn fleet() -> Report {
    serde_json::from_str(FLEET).expect("the fleet fixture is a report")
}

fn rules() -> RuleSet {
    RuleSet::load(&RulesSources::EMBEDDED).expect("the shipped rules load")
}

fn from_files(files: Vec<FileReport>) -> Report {
    Report::from_files(&rules(), GENERATED_AT.to_owned(), Lang::En, files)
}

/// The paths of the files of `report`, in the order of the report.
fn paths(report: &Report) -> Vec<&str> {
    report.files.iter().map(|file| file.path.as_str()).collect()
}

/// A file that was not read, only its path and status.
fn bare(path: &str, status: Status) -> FileReport {
    let mut file = fleet().files.remove(0);
    file.path = path.to_owned();
    file.status = status;
    file.vba_project = None;
    file.metrics = None;
    file.complexity_level = None;
    file.primary_family = None;
    file.families = None;
    file.max_severity = None;
    file.signals = None;
    file.traits = None;
    file
}

#[test]
fn the_counts_and_the_summary_follow_from_the_files() {
    let expected = fleet();

    let report = from_files(expected.files.clone());

    assert_eq!(report.scan.files_total, expected.scan.files_total);
    assert_eq!(
        report.scan.files_with_macros,
        expected.scan.files_with_macros
    );
    assert_eq!(report.scan.files_failed, expected.scan.files_failed);
    assert_eq!(report.summary, expected.summary);
    // What the caller knows and the core does not.
    assert_eq!(report.scan.roots, None);
    assert_eq!(report.scan.duration_ms, None);
    assert_eq!(report.tool.build, None);
}

#[test]
fn the_report_carries_what_the_caller_injects() {
    let rules = rules();

    let report = Report::from_files(
        &rules,
        "1999-12-31T23:59:59+01:00".to_owned(),
        Lang::Fr,
        vec![],
    );

    assert_eq!(report.generated_at, "1999-12-31T23:59:59+01:00");
    assert_eq!(report.lang, Some(Lang::Fr));
    assert_eq!(report.tool.rules_version, rules.version());
    assert!(!report.redacted);
}

#[test]
fn the_files_are_in_the_order_of_their_paths_whatever_the_order_they_came_in() {
    let files = fleet().files;
    let mut expected: Vec<String> = files.iter().map(|file| file.path.clone()).collect();
    expected.sort();
    let mut reversed = files.clone();
    reversed.reverse();
    let mut rotated = files.clone();
    rotated.rotate_left(4);

    let sorted = from_files(files);

    assert_eq!(paths(&sorted), expected);
    let json = sorted.to_json().expect("the report serialises");
    assert_eq!(report_problems(&json), Vec::<String>::new());
    for shuffled in [reversed, rotated] {
        let report = from_files(shuffled);
        assert_eq!(report, sorted);
        assert_eq!(report.to_json().expect("the report serialises"), json);
    }
}

#[test]
fn paths_compare_as_bytes_not_as_words() {
    let files = ["b.xls", "B.xls", "a/z.xls", "a.xls", "é.xls", "a-b.xls"]
        .map(|path| bare(path, Status::NoMacros))
        .to_vec();

    let report = from_files(files);

    assert_eq!(
        paths(&report),
        ["B.xls", "a-b.xls", "a.xls", "a/z.xls", "b.xls", "é.xls"]
    );
}

#[test]
fn files_of_one_path_keep_a_fixed_order_too() {
    let mut one = bare("same.xls", Status::NoMacros);
    one.sha256 = "1".repeat(64);
    let mut other = bare("same.xls", Status::NoMacros);
    other.sha256 = "2".repeat(64);

    let forward = from_files(vec![one.clone(), other.clone()]);
    let backward = from_files(vec![other, one]);

    assert_eq!(forward, backward);
}

#[test]
fn the_fleet_report_is_valid_against_the_schema() {
    let json = from_files(fleet().files)
        .to_json()
        .expect("the report serialises");

    assert_eq!(report_problems(&json), Vec::<String>::new());
}

#[test]
fn only_the_statuses_that_occur_are_counted() {
    let report = from_files(vec![
        bare("a.xls", Status::NoMacros),
        bare("b.xls", Status::NoMacros),
    ]);

    let statuses: Vec<(Status, u32)> = report.summary.by_status.into_iter().collect();
    assert_eq!(statuses, [(Status::NoMacros, 2)]);
    assert!(report.summary.by_family.is_empty());
    assert!(report.summary.by_complexity.is_empty());
    assert!(report.summary.by_severity.is_empty());
}

#[test]
fn only_a_damaged_oversized_suspicious_or_broken_file_is_a_failure() {
    let failures: Vec<Status> = [
        Status::Ok,
        Status::NoMacros,
        Status::Encrypted,
        Status::Corrupted,
        Status::Unsupported,
        Status::TooLarge,
        Status::SuspiciousArchive,
        Status::Error,
    ]
    .into_iter()
    .filter(|status| status.is_failure())
    .collect();

    assert_eq!(
        failures,
        [
            Status::Corrupted,
            Status::TooLarge,
            Status::SuspiciousArchive,
            Status::Error
        ]
    );
}

#[test]
fn the_files_that_failed_are_counted_by_status() {
    let report = from_files(vec![
        bare("a.xls", Status::Corrupted),
        bare("b.xls", Status::Encrypted),
        bare("c.xls", Status::Unsupported),
        bare("d.xls", Status::TooLarge),
        bare("e.xls", Status::Ok),
    ]);

    assert_eq!(report.scan.files_total, 5);
    assert_eq!(report.scan.files_with_macros, 1);
    assert_eq!(report.scan.files_failed, 2);
}
