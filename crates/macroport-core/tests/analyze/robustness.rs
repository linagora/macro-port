// SPDX-License-Identifier: Apache-2.0
//! A2, robustness (ACCEPTANCE): every file of `corpus/` goes through
//! `extract()` and through `analyze()` without a panic, and every malformed
//! file gets a status that `fixtures/malformed/expected.yaml` allows, with an
//! error code.
//!
//! A hang or a memory blow-up shows as a test that does not end or a process
//! that is killed: the limits of SPEC M1 are what prevents both, and the unit
//! tests of the container and MS-OVBA layers check each one.

use std::collections::BTreeSet;

use macroport_core::report::{FileReport, Status};
use macroport_testkit::corpus;
use serde::Deserialize;

use crate::support::{corpus_report, extraction};

/// The keys of `malformed/expected.yaml` that these tests use.
#[derive(Deserialize)]
struct Malformed {
    fixtures: Vec<MalformedFile>,
}

#[derive(Deserialize)]
struct MalformedFile {
    /// Relative to `corpus/fixtures/`.
    file: String,
    /// Any one of these is acceptable.
    expected_status: Vec<Status>,
}

fn malformed() -> Vec<MalformedFile> {
    let text = String::from_utf8(corpus::read("fixtures/malformed/expected.yaml")).expect("UTF-8");
    let list: Malformed = serde_saphyr::from_str(&text).expect("the list of malformed files reads");
    list.fixtures
}

#[test]
fn the_list_names_every_malformed_file_and_no_other() {
    let present = corpus::files("fixtures/malformed", &[]);
    let present: BTreeSet<_> = (present.iter())
        .filter_map(|path| path.strip_prefix("fixtures/"))
        .filter(|path| !path.ends_with("expected.yaml"))
        .collect();

    let listed = malformed();
    let listed: BTreeSet<_> = listed.iter().map(|file| file.file.as_str()).collect();

    assert_eq!(listed, present);
}

/// The report of the file `path` of `corpus/` from `extract()`.
fn extracted(path: &str) -> FileReport {
    extraction(&corpus::read(path), path).file
}

/// Each malformed file gets from `read` a status that the list allows, and an
/// error code exactly when it could not be read in full.
fn assert_each_malformed_file_gets_a_status_that_the_list_allows(read: fn(&str) -> FileReport) {
    for MalformedFile {
        file,
        expected_status,
    } in malformed()
    {
        let report = read(&format!("fixtures/{file}"));

        assert!(
            expected_status.contains(&report.status),
            "{file}: {:?}, allowed {expected_status:?}",
            report.status
        );
        let errors = report.errors.unwrap_or_default();
        let read_in_full = matches!(report.status, Status::Ok | Status::NoMacros);
        assert_eq!(errors.is_empty(), read_in_full, "{file}: {errors:?}");
    }
}

/// Every file of the corpus gets from `read` a report that is no internal
/// error, and an error code exactly when it could not be read in full.
fn assert_every_file_of_the_corpus_gets_a_report(read: fn(&str) -> FileReport) {
    let paths = corpus::files("", &[]);
    assert!(paths.len() > 500, "{} files", paths.len());
    for path in paths {
        let report = read(&path);

        // SPEC M1: an unexpected internal error "should not happen; tested".
        // `expected.yaml` tolerates `error` for three malformed files, but
        // nothing in the core gives it, and this keeps it so.
        assert_ne!(report.status, Status::Error, "{path}");
        // A file has an error code exactly when it could not be read in full.
        let read_in_full = matches!(report.status, Status::Ok | Status::NoMacros);
        let errors = report.errors.unwrap_or_default();
        assert_eq!(errors.is_empty(), read_in_full, "{path}: {errors:?}");
    }
}

#[test]
fn each_malformed_file_gets_a_status_that_the_list_allows_and_an_error_code_when_extracted() {
    assert_each_malformed_file_gets_a_status_that_the_list_allows(extracted);
}

#[test]
fn each_malformed_file_gets_a_status_that_the_list_allows_and_an_error_code_when_analysed() {
    assert_each_malformed_file_gets_a_status_that_the_list_allows(corpus_report);
}

#[test]
fn every_file_of_the_corpus_is_extracted_without_an_internal_error() {
    assert_every_file_of_the_corpus_gets_a_report(extracted);
}

#[test]
fn every_file_of_the_corpus_is_analysed_without_an_internal_error() {
    assert_every_file_of_the_corpus_gets_a_report(corpus_report);
}
