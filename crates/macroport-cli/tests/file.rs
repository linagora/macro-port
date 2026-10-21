// SPDX-License-Identifier: Apache-2.0
//! End-to-end tests of `macro-port file`: one file is analysed and its report
//! is the whole of stdout.

mod support;

use std::fs::{self, File};

use macroport_core::report::{Build, Status};
use support::{
    MODULE, OTHER_VERSION, as_arg, printed_report, put, run_in, run_with_stdout_closed, scratch,
    shipped_rules_with_version, text, use_fixture, valid_rules,
};

#[test]
fn file_prints_the_report_of_the_file_on_stdout_and_nothing_else() {
    let dir = scratch("file_json");
    put(&dir, "m.bas", MODULE);

    let output = run_in(&dir, &["file", "m.bas"]);

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let report = printed_report(&output.stdout);
    assert_eq!(report.files.len(), 1);
    assert_eq!(report.files[0].path, "m.bas");
    assert_eq!(report.scan.roots, Some(vec!["m.bas".to_owned()]));
    assert_eq!(report.tool.build, Some(Build::Cli));
    // No progress, no summary, and no file written.
    assert!(output.stderr.is_empty(), "{output:?}");
    let entries = fs::read_dir(&dir).expect("the directory is read").count();
    assert_eq!(entries, 1);
}

#[test]
fn file_prints_an_html_page_in_the_language_asked_for() {
    let dir = scratch("file_html");
    put(&dir, "m.bas", MODULE);

    for (lang, tag) in [("en", "<html lang=\"en\">"), ("fr", "<html lang=\"fr\">")] {
        let output = run_in(&dir, &["file", "m.bas", "--format", "html", "--lang", lang]);

        assert_eq!(output.status.code(), Some(0), "{lang}: {output:?}");
        let page = text(&output.stdout);
        assert!(page.starts_with("<!doctype html>"), "{lang}");
        assert!(page.contains(tag), "{lang}");
        assert!(output.stderr.is_empty(), "{lang}: {output:?}");
    }
}

#[test]
fn file_exits_with_1_when_the_file_failed_and_still_prints_the_report() {
    let dir = scratch("file_failed");
    File::create(dir.join("big.bas"))
        .and_then(|file| file.set_len(2 * 1024 * 1024))
        .expect("a sparse file");

    let output = run_in(&dir, &["file", "big.bas", "--max-file-size", "1"]);

    // SPEC M4: 1 is an analysis that completed with a file in a failure status.
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let report = printed_report(&output.stdout);
    assert_eq!(report.files[0].status, Status::TooLarge);
    assert_eq!(report.scan.files_failed, 1);
}

#[test]
fn file_takes_a_file_whatever_its_extension() {
    let dir = scratch("file_extension");
    put(&dir, "notes.txt", b"not an Excel file");

    let output = run_in(&dir, &["file", "notes.txt"]);

    let report = printed_report(&output.stdout);
    assert_eq!(report.files.len(), 1);
    assert_eq!(report.files[0].path, "notes.txt");
    // SPEC M4: 1 when a file is `corrupted`, `error`, `suspicious_archive` or
    // `too_large`, whatever the status of this one is.
    let failed = matches!(
        report.files[0].status,
        Status::Corrupted | Status::Error | Status::SuspiciousArchive | Status::TooLarge
    );
    assert_eq!(output.status.code(), Some(i32::from(failed)), "{output:?}");
}

#[test]
fn file_exits_with_2_on_a_usage_error() {
    let dir = scratch("file_usage");
    put(&dir, "m.bas", MODULE);
    put(&dir, "tree/n.bas", MODULE);
    let cases: [&[&str]; 10] = [
        &["file"],
        &["file", "missing.bas"],
        &["file", "tree"],
        &["file", "m.bas", "tree/n.bas"],
        &["file", "m.bas", "--format", "csv"],
        &["file", "m.bas", "--lang", "de"],
        &["file", "m.bas", "--jobs", "2"],
        &["file", "m.bas", "--rules", "no-such-rules"],
        // The reports of a scan are the ones to share: `file` has no privacy
        // options.
        &["file", "m.bas", "--no-snippets"],
        &["file", "m.bas", "--redact"],
    ];

    for args in cases {
        let output = run_in(&dir, args);

        assert_eq!(output.status.code(), Some(2), "{args:?}: {output:?}");
        assert!(output.stdout.is_empty(), "{args:?}: {output:?}");
    }
}

#[test]
fn a_deterministic_report_of_a_file_is_the_same_bytes_each_time() {
    let dir = scratch("file_deterministic");
    put(&dir, "m.bas", MODULE);

    let first = run_in(&dir, &["file", "m.bas", "--deterministic"]);
    let second = run_in(&dir, &["file", "m.bas", "--deterministic"]);

    assert_eq!(first.status.code(), Some(0), "{first:?}");
    assert_eq!(first.stdout, second.stdout);
    let report = printed_report(&first.stdout);
    assert_eq!(report.generated_at, "1970-01-01T00:00:00Z");
    assert_eq!(report.scan.duration_ms, None);
}

#[test]
fn file_uses_the_rules_of_the_directory() {
    let dir = scratch("file_rules");
    put(&dir, "m.bas", MODULE);
    let rules = shipped_rules_with_version(&dir, OTHER_VERSION);

    let own = run_in(&dir, &["file", "m.bas", "--rules", as_arg(&rules)]);
    let embedded = run_in(&dir, &["file", "m.bas"]);

    assert_eq!(own.status.code(), Some(0), "{own:?}");
    assert_eq!(
        printed_report(&own.stdout).tool.rules_version,
        OTHER_VERSION
    );
    let embedded = printed_report(&embedded.stdout);
    assert_ne!(embedded.tool.rules_version, OTHER_VERSION);
}

#[test]
fn file_stops_with_3_on_rules_that_do_not_pass_the_check() {
    let dir = scratch("file_rules_invalid");
    put(&dir, "m.bas", MODULE);
    let rules = valid_rules(&dir);
    use_fixture(&rules, "families", "invalid-regex");

    let output = run_in(&dir, &["file", "m.bas", "--rules", as_arg(&rules)]);

    assert_eq!(output.status.code(), Some(3), "{output:?}");
    assert!(output.stdout.is_empty(), "{output:?}");
    let stderr = text(&output.stderr);
    assert!(
        stderr.starts_with("families.yaml: fmt.loop-cells: "),
        "{stderr}"
    );
    assert!(stderr.ends_with("2 problems found\n"), "{stderr}");
}

#[test]
fn a_reader_that_went_away_is_not_an_error() {
    let dir = scratch("file_reader_gone");
    put(&dir, "m.bas", MODULE);

    let output = run_with_stdout_closed(&dir, &["file", "m.bas"]);

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
}

/// Stdout is `/dev/full`, where every write fails with "no space left on
/// device": the reader is still there and the document is lost. Linux only,
/// since it needs that device.
#[cfg(target_os = "linux")]
#[test]
fn a_document_that_cannot_be_written_is_a_fatal_error_in_the_language_asked_for() {
    use std::fs::OpenOptions;
    use std::process::Command;

    let dir = scratch("file_stdout_full");
    put(&dir, "m.bas", MODULE);

    for (lang, start) in [
        ("en", "error: cannot print on the standard output: "),
        (
            "fr",
            "erreur : impossible d'écrire sur la sortie standard : ",
        ),
    ] {
        let full = OpenOptions::new()
            .write(true)
            .open("/dev/full")
            .expect("/dev/full opens");
        let output = Command::new(env!("CARGO_BIN_EXE_macro-port"))
            .current_dir(&dir)
            .args(["file", "m.bas", "--lang", lang])
            .stdout(full)
            .output()
            .expect("the macro-port binary runs");

        assert_eq!(output.status.code(), Some(3), "{lang}: {output:?}");
        let stderr = text(&output.stderr);
        assert!(stderr.starts_with(start), "{lang}: {stderr}");
        assert_eq!(stderr.lines().count(), 1, "{lang}: {stderr}");
    }
}

#[test]
fn the_report_of_a_file_names_no_other_file() {
    // Another file next to it does not matter: only the file is read.
    let dir = scratch("file_alone");
    put(&dir, "m.bas", MODULE);
    put(&dir, "other.bas", MODULE);

    let output = run_in(&dir, &["file", "m.bas"]);

    let report = printed_report(&output.stdout);
    assert_eq!(report.files.len(), 1);
    assert_eq!(report.scan.files_total, 1);
}
