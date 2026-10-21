// SPDX-License-Identifier: Apache-2.0
//! What every report says about its file, whatever the file holds: the path,
//! the size, the hash, the container that the extension names, and the limit
//! on the size.

use macroport_core::analyze::{DEFAULT_MAX_FILE_SIZE, Options, too_large};
use macroport_core::report::{Container, Status};

use crate::support::{RULES, extraction, extraction_with, report_problems_of};

/// The SHA-256 of the three bytes `abc`.
const ABC_SHA256: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

#[test]
fn a_report_gives_the_path_the_size_and_the_hash_of_the_file() {
    let report = extraction(b"abc", "dir/Book.xlsm").file;

    assert_eq!(report.path, "dir/Book.xlsm");
    assert_eq!(report.size, 3);
    assert_eq!(report.sha256, ABC_SHA256);
}

#[test]
fn the_core_exposes_analyze_at_its_root() {
    let options = Options::new(&RULES);

    let extracted = extraction(b"abc", "a.xlsm").file;

    assert_eq!(
        macroport_core::analyze(b"abc", "a.xlsm", &options),
        extracted
    );
}

#[test]
fn the_default_limit_is_200_mebibytes() {
    assert_eq!(DEFAULT_MAX_FILE_SIZE, 200 * 1024 * 1024);
    assert_eq!(Options::new(&RULES).max_file_size, DEFAULT_MAX_FILE_SIZE);
}

#[test]
fn a_file_above_the_limit_is_too_large_and_not_read() {
    let options = Options {
        max_file_size: 9,
        ..Options::new(&RULES)
    };

    let extraction = extraction_with(b"abc0123456", "big.xlsm", &options);
    let report = extraction.file;

    assert_eq!(report.status, Status::TooLarge);
    assert_eq!(report.container, Container::Xlsm);
    assert_eq!(report.size, 10);
    assert_eq!(
        report.sha256,
        "a04f250c2de706e0853e438b4aab335f4345dff069e46d5be432b5382317223d"
    );
    assert!(report.vba_project.is_none());
    assert!(extraction.sources.is_empty());
    let errors = report.errors.unwrap_or_default();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "file.too_large");
    assert!(
        errors[0].message.contains("9 bytes"),
        "{}",
        errors[0].message
    );
}

#[test]
fn a_file_of_the_size_of_the_limit_is_read() {
    let options = Options {
        max_file_size: 10,
        ..Options::new(&RULES)
    };

    let report = extraction_with(b"0123456789", "a.xlsm", &options).file;

    assert_ne!(report.status, Status::TooLarge);
}

#[test]
fn a_caller_that_did_not_read_a_file_can_report_it_as_too_large() {
    let options = Options::new(&RULES);
    let sha256 = "ab".repeat(32);

    let report = too_large("dir/huge.xlsb", 300 * 1024 * 1024, sha256.clone(), &options);

    assert_eq!(report_problems_of(&report), Vec::<String>::new());
    assert_eq!(report.status, Status::TooLarge);
    assert_eq!(report.container, Container::Xlsb);
    assert_eq!(report.size, 300 * 1024 * 1024);
    assert_eq!(report.sha256, sha256);
    let errors = report.errors.unwrap_or_default();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "file.too_large");
    assert!(
        errors[0].message.contains("209715200 bytes"),
        "{}",
        errors[0].message
    );
}

#[test]
fn the_extension_names_the_container_in_any_case() {
    for (path, container) in [
        ("a.XLSM", Container::Xlsm),
        ("a.xltm", Container::Xltm),
        ("dir.d/a.xlam", Container::Xlam),
        ("a.xlsb", Container::Xlsb),
        ("a.xls", Container::Xls),
        ("a.xlt", Container::Xlt),
        ("a.xla", Container::Xla),
        ("a.xlsx", Container::Xlsx),
        ("a.xltx", Container::Xltx),
        ("a.txt", Container::Unknown),
        ("a", Container::Unknown),
    ] {
        assert_eq!(extraction(b"", path).file.container, container, "{path}");
    }
}

#[test]
fn text_with_the_extension_of_a_workbook_is_unsupported() {
    let report = extraction(b"Sub A()\nEnd Sub\n", "a.xlsm").file;

    assert_eq!(report.status, Status::Unsupported);
    assert_eq!(report.container, Container::Xlsm);
    let errors = report.errors.unwrap_or_default();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "file.unsupported_format");
}
