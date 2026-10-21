// SPDX-License-Identifier: Apache-2.0
//! End-to-end tests of `macro-port scan`: the binary runs on the corpus and on
//! small trees, and the reports it writes are read back and validated.

mod support;

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use macroport_cli::scan::EXTENSIONS;
use macroport_core::report::{Build, Report, Status};
use macroport_testkit::{corpus, report_problems};
use serde_json::Value;
use support::{put, scratch};

/// The `generated_at` of a deterministic report (SPEC M4).
const EPOCH: &str = "1970-01-01T00:00:00Z";

/// A module with a procedure, small enough to be analysed at once.
const MODULE: &[u8] = b"Sub Hello()\n    Range(\"A1\").Value = 1\nEnd Sub\n";

/// The root of the repository.
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Runs `macro-port` in `dir` with `args`.
fn run_in(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_macro-port"))
        .current_dir(dir)
        .args(args)
        .output()
        .expect("the macro-port binary runs")
}

/// Runs `macro-port` at the root of the repository, so that the paths in its
/// reports are the relative paths `corpus/...`.
fn run(args: &[&str]) -> Output {
    run_in(&repo_root(), args)
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn as_arg(path: &Path) -> &str {
    path.to_str()
        .expect("a path of the target directory is UTF-8")
}

/// The text of the `report.json` in `out`, once it has passed the schema: every
/// JSON that the tests read is validated (ACCEPTANCE A3).
fn report_json(out: &Path) -> String {
    let path = out.join("report.json");
    let json = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
    assert_eq!(report_problems(&json), Vec::<String>::new());
    json
}

fn read_report(out: &Path) -> Report {
    serde_json::from_str(&report_json(out)).expect("report.json is a report")
}

#[test]
fn scan_writes_the_report_of_the_files_under_a_directory() {
    let out = scratch("scan_writes").join("reports/nested");

    let output = run(&[
        "scan",
        "corpus/families/01-mise-en-forme/",
        "-o",
        as_arg(&out),
    ]);

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let report = read_report(&out);
    let expected: Vec<String> = corpus::files("families/01-mise-en-forme", &EXTENSIONS)
        .into_iter()
        .map(|file| format!("corpus/{file}"))
        .collect();
    let found: Vec<&str> = report.files.iter().map(|file| file.path.as_str()).collect();
    assert_eq!(found, expected);
    assert!(!expected.is_empty());
    assert_eq!(usize::try_from(report.scan.files_total), Ok(expected.len()));
    // The root is written like the paths under it: no trailing separator.
    assert_eq!(
        report.scan.roots,
        Some(vec!["corpus/families/01-mise-en-forme".to_owned()])
    );
    assert_eq!(report.tool.build, Some(Build::Cli));
    assert!(!report.redacted);
    // Only the report is written: no other file, no temporary file.
    let written: Vec<_> = fs::read_dir(&out)
        .expect("the output directory exists")
        .map(|entry| entry.expect("an entry").file_name())
        .collect();
    assert_eq!(written, ["report.json"]);
}

#[test]
fn scan_writes_to_macro_port_report_when_no_output_is_given() {
    let dir = scratch("scan_default_output");
    put(&dir, "m.bas", MODULE);

    let output = run_in(&dir, &["scan", "m.bas"]);

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let report = read_report(&dir.join("macro-port-report"));
    assert_eq!(report.files.len(), 1);
    assert_eq!(report.files[0].path, "m.bas");
}

#[test]
fn scan_takes_several_paths_and_reports_each_file_once() {
    let dir = scratch("scan_several");
    put(&dir, "one/a.bas", MODULE);
    put(&dir, "two/b.bas", MODULE);
    put(&dir, "two/notes.txt", b"not covered");

    let output = run_in(
        &dir,
        &[
            "scan",
            "two",
            "one",
            "two/b.bas",
            "two/notes.txt",
            "-o",
            "out",
        ],
    );

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let report = read_report(&dir.join("out"));
    let paths: Vec<&str> = report.files.iter().map(|file| file.path.as_str()).collect();
    assert_eq!(paths, ["one/a.bas", "two/b.bas", "two/notes.txt"]);
    // The roots keep the order in which they were given.
    assert_eq!(
        report.scan.roots,
        Some(vec![
            "two".to_owned(),
            "one".to_owned(),
            "two/b.bas".to_owned(),
            "two/notes.txt".to_owned(),
        ])
    );
}

#[test]
fn a_deterministic_scan_gives_the_same_bytes_whatever_the_number_of_jobs() {
    let base = scratch("scan_deterministic");
    // The text samples, and the malformed files: every kind of outcome.
    let inputs = ["corpus/families", "corpus/fixtures/malformed"];
    let mut reports = Vec::new();

    for (n, jobs) in ["1", "2", "4", "8", "8"].into_iter().enumerate() {
        let out = base.join(format!("run-{n}"));
        let mut args = vec!["scan"];
        args.extend(inputs);
        args.extend(["-o", as_arg(&out), "--deterministic", "--jobs", jobs]);
        let output = run(&args);
        assert!(
            matches!(output.status.code(), Some(0 | 1)),
            "{jobs} jobs: {output:?}"
        );
        reports.push(report_json(&out));
    }

    for pair in reports.windows(2) {
        assert!(pair[0] == pair[1], "two runs differ");
    }
    let value: Value = serde_json::from_str(&reports[0]).expect("report.json is JSON");
    assert_eq!(value["generated_at"], EPOCH);
    assert!(value["scan"].get("duration_ms").is_none());
}

#[test]
fn a_scan_that_is_not_deterministic_dates_the_report_and_times_the_scan() {
    let dir = scratch("scan_clock");
    put(&dir, "m.bas", MODULE);

    let output = run_in(&dir, &["scan", "m.bas", "-o", "out"]);

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let report = read_report(&dir.join("out"));
    // `report_problems` already checked that it is an RFC 3339 date-time.
    assert_ne!(report.generated_at, EPOCH);
    assert!(
        report.generated_at.ends_with('Z'),
        "{}",
        report.generated_at
    );
    assert!(report.scan.duration_ms.is_some());
}

#[test]
fn the_summary_goes_to_stdout_and_the_progress_to_stderr() {
    let dir = scratch("scan_streams");
    put(&dir, "m.bas", MODULE);

    let output = run_in(&dir, &["scan", "m.bas", "-o", "out"]);

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let stdout = text(&output.stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 2, "{stdout}");
    assert!(
        lines[0].starts_with("Files: 1 (with macros: "),
        "{}",
        lines[0]
    );
    assert!(lines[0].ends_with(", failed: 0)"), "{}", lines[0]);
    assert_eq!(lines[1], "Reports written to out");
    let stderr = text(&output.stderr);
    assert!(stderr.contains("Files analysed: 0 / 1"), "{stderr}");
    assert!(stderr.contains("Files analysed: 1 / 1"), "{stderr}");
}

#[test]
fn the_messages_follow_the_language() {
    let dir = scratch("scan_lang");
    put(&dir, "m.bas", MODULE);

    let output = run_in(&dir, &["scan", "m.bas", "-o", "out", "--lang", "fr"]);

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let stdout = text(&output.stdout);
    assert!(
        stdout.starts_with("Fichiers : 1 (avec macros : "),
        "{stdout}"
    );
    assert!(stdout.contains(", en échec : 0)"), "{stdout}");
    assert!(stdout.contains("Rapports écrits dans out"), "{stdout}");
    assert!(text(&output.stderr).contains("Fichiers analysés : 1 / 1"));
    let report = read_report(&dir.join("out"));
    assert_eq!(serde_json::to_value(report.lang).expect("a language"), "fr");
}

#[test]
fn a_scan_of_a_directory_without_a_covered_file_reports_no_file() {
    let dir = scratch("scan_nothing");
    put(&dir, "tree/notes.txt", b"not covered");

    let output = run_in(&dir, &["scan", "tree", "-o", "out"]);

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let report = read_report(&dir.join("out"));
    assert!(report.files.is_empty());
    assert_eq!(report.scan.files_total, 0);
    assert!(text(&output.stdout).starts_with("Files: 0 (with macros: 0, failed: 0)"));
}

#[test]
fn scan_exits_with_1_when_a_file_failed_and_still_writes_the_report() {
    let dir = scratch("scan_failed");
    put(&dir, "tree/ok.bas", MODULE);
    File::create(dir.join("tree/big.bas"))
        .and_then(|file| file.set_len(2 * 1024 * 1024))
        .expect("a sparse file");

    let output = run_in(&dir, &["scan", "tree", "-o", "out", "--max-file-size", "1"]);

    // SPEC M4: 1 is a scan that completed with a file in a failure status.
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let report = read_report(&dir.join("out"));
    assert_eq!(report.scan.files_total, 2);
    assert_eq!(report.scan.files_failed, 1);
    assert_eq!(report.files[0].path, "tree/big.bas");
    assert_eq!(report.files[0].status, Status::TooLarge);
    assert_ne!(report.files[1].status, Status::TooLarge);
    assert!(text(&output.stdout).contains("failed: 1)"));
}

#[test]
fn scan_exits_with_2_on_a_usage_error() {
    let dir = scratch("scan_usage");
    put(&dir, "m.bas", MODULE);
    let cases: [&[&str]; 7] = [
        &["scan"],
        &["scan", "missing.bas"],
        &["scan", "m.bas", "--jobs", "0"],
        &["scan", "m.bas", "--jobs", "many"],
        &["scan", "m.bas", "--max-file-size", "0"],
        &["scan", "m.bas", "--lang", "de"],
        &["scan", "m.bas", "--no-such-option"],
    ];

    for args in cases {
        let output = run_in(&dir, args);

        assert_eq!(output.status.code(), Some(2), "{args:?}: {output:?}");
        // A usage error writes nothing.
        assert!(!dir.join("macro-port-report").exists(), "{args:?}");
    }
}

#[test]
fn scan_says_which_path_does_not_exist() {
    let dir = scratch("scan_missing");

    let output = run_in(&dir, &["scan", "missing.bas"]);

    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(text(&output.stderr).contains("missing.bas"));
}

#[test]
fn scan_exits_with_3_when_the_reports_cannot_be_written_and_scans_nothing() {
    let dir = scratch("scan_fatal");
    put(&dir, "m.bas", MODULE);
    put(&dir, "blocker", b"a file, not a directory");

    let output = run_in(&dir, &["scan", "m.bas", "-o", "blocker/out"]);

    assert_eq!(output.status.code(), Some(3), "{output:?}");
    let stderr = text(&output.stderr);
    assert!(
        stderr.starts_with("error: cannot create the directory blocker/out"),
        "{stderr}"
    );
    // The output is checked before the files are read.
    assert!(!stderr.contains("Files analysed"), "{stderr}");
    assert!(text(&output.stdout).is_empty());
}

#[cfg(unix)]
#[test]
fn scan_exits_with_1_when_a_file_cannot_be_read() {
    let dir = scratch("scan_unreadable");
    put(&dir, "tree/a.bas", MODULE);
    let secret = put(&dir, "tree/b.bas", MODULE);
    let Some(_denied) = support::deny_access(&secret) else {
        return;
    };

    let output = run_in(&dir, &["scan", "tree", "-o", "out"]);

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let report = read_report(&dir.join("out"));
    assert_eq!(report.files[1].status, Status::Error);
    assert_eq!(report.scan.files_failed, 1);
}

#[test]
fn a_console_that_is_gone_does_not_stop_the_scan() {
    let dir = scratch("scan_console_gone");
    for n in 0..30 {
        put(&dir, &format!("tree/m{n:02}.bas"), MODULE);
    }

    let mut child = Command::new(env!("CARGO_BIN_EXE_macro-port"))
        .current_dir(&dir)
        .args(["scan", "tree", "-o", "out"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the macro-port binary runs");
    // Nobody reads the progress or the summary: writing to them fails.
    drop(child.stdout.take());
    drop(child.stderr.take());
    let status = child.wait().expect("the command ends");

    assert_eq!(status.code(), Some(0));
    assert_eq!(read_report(&dir.join("out")).files.len(), 30);
}
