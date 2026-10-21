// SPDX-License-Identifier: Apache-2.0
//! End-to-end tests of `macro-port scan`: the binary runs on the corpus and on
//! small trees, and the reports it writes are read back and validated.

mod support;

use std::fs::{self, File};
use std::io;
use std::path::Path;
use std::process::Command;

use macroport_cli::scan::EXTENSIONS;
use macroport_core::report::{Build, Report, Status};
use macroport_testkit::{corpus, report_problems};
use serde_json::Value;
use support::{as_arg, put, run, run_in, scratch, text};

/// The `generated_at` of a deterministic report (SPEC M4).
const EPOCH: &str = "1970-01-01T00:00:00Z";

/// A module with a procedure, small enough to be analysed at once.
const MODULE: &[u8] = b"Sub Hello()\n    Range(\"A1\").Value = 1\nEnd Sub\n";

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

/// The names of the files in `out`, in order.
fn written(out: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(out)
        .expect("the output directory exists")
        .map(|entry| entry.expect("an entry").file_name())
        .map(|name| name.to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn scan_writes_the_report_of_the_files_under_a_directory() {
    let out = scratch("scan_writes").join("reports/nested");

    let output = run(&[
        "scan",
        "corpus/families/01-mise-en-forme/",
        "-o",
        as_arg(&out),
        "--format",
        "json",
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
    // Only the report that was asked for is written: no other file, no
    // temporary file.
    assert_eq!(written(&out), ["report.json"]);
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
    let mut runs = Vec::new();

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
        // Every report that is written, not only the JSON one.
        let files: Vec<(String, Vec<u8>)> = written(&out)
            .into_iter()
            .map(|name| {
                let bytes = fs::read(out.join(&name)).expect("a report can be read");
                (name, bytes)
            })
            .collect();
        runs.push(files);
    }

    for pair in runs.windows(2) {
        assert!(pair[0] == pair[1], "two runs differ");
    }
    let json = report_json(&base.join("run-0"));
    let value: Value = serde_json::from_str(&json).expect("report.json is JSON");
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
    // The output directory is made before the files are read.
    assert!(!stderr.contains("Files analysed"), "{stderr}");
    assert!(text(&output.stdout).is_empty());
}

#[test]
fn scan_exits_with_3_when_a_report_cannot_be_written() {
    let dir = scratch("scan_unwritable");
    put(&dir, "m.bas", MODULE);
    // A directory where the report goes: the output directory can be made, the
    // file cannot be written.
    fs::create_dir_all(dir.join("out/report.json")).expect("a directory named like the report");

    let output = run_in(&dir, &["scan", "m.bas", "-o", "out"]);

    assert_eq!(output.status.code(), Some(3), "{output:?}");
    let stderr = text(&output.stderr);
    assert!(
        stderr.contains("error: cannot write the file out/report.json: "),
        "{stderr}"
    );
    assert!(text(&output.stdout).is_empty());
}

#[test]
fn the_errors_follow_the_language() {
    let dir = scratch("scan_fatal_fr");
    put(&dir, "m.bas", MODULE);
    put(&dir, "blocker", b"a file, not a directory");

    let output = run_in(
        &dir,
        &["scan", "m.bas", "-o", "blocker/out", "--lang", "fr"],
    );

    assert_eq!(output.status.code(), Some(3), "{output:?}");
    let stderr = text(&output.stderr);
    // French puts a space before the colon, in the message and in the reason.
    assert!(
        stderr.starts_with("erreur : impossible de créer le répertoire blocker/out : "),
        "{stderr}"
    );
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

    // The reader of the pipe is closed before the command starts: nobody reads
    // the progress or the summary, and every line that it prints fails.
    let (reader, writer) = io::pipe().expect("a pipe");
    drop(reader);
    let status = Command::new(env!("CARGO_BIN_EXE_macro-port"))
        .current_dir(&dir)
        .args(["scan", "tree", "-o", "out"])
        .stdout(writer.try_clone().expect("the pipe is cloned"))
        .stderr(writer)
        .status()
        .expect("the macro-port binary runs");

    assert_eq!(status.code(), Some(0));
    assert_eq!(read_report(&dir.join("out")).files.len(), 30);
}

/// Every file that `scan` writes when it is asked for every format.
const EVERY_REPORT: [&str; 5] = [
    "files.csv",
    "procedures.csv",
    "report.html",
    "report.json",
    "signals.csv",
];

/// The UTF-8 byte order mark that makes Excel read a CSV file as UTF-8.
const BYTE_ORDER_MARK: &[u8] = b"\xEF\xBB\xBF";

#[test]
fn scan_writes_every_report_when_no_format_is_given() {
    let dir = scratch("scan_every_format");
    put(&dir, "m.bas", MODULE);

    let output = run_in(&dir, &["scan", "m.bas", "-o", "out"]);

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let out = dir.join("out");
    assert_eq!(written(&out), EVERY_REPORT);
    read_report(&out);
    let html = fs::read_to_string(out.join("report.html")).expect("report.html is UTF-8");
    assert!(html.starts_with("<!doctype html>"));
    assert!(html.contains("<html lang=\"en\">"));
    for (name, header) in [
        ("files.csv", "path,container,status,"),
        ("procedures.csv", "path,module,procedure,"),
        ("signals.csv", "path,module,procedure,signal_id,"),
    ] {
        let csv = fs::read(out.join(name)).expect("a CSV report can be read");
        assert!(csv.starts_with(BYTE_ORDER_MARK), "{name}");
        let text = String::from_utf8_lossy(&csv[BYTE_ORDER_MARK.len()..]).into_owned();
        assert!(text.starts_with(header), "{name}: {text}");
    }
}

#[test]
fn scan_writes_only_the_formats_it_is_asked_for() {
    let dir = scratch("scan_formats");
    put(&dir, "m.bas", MODULE);
    let cases: [(&str, &[&str]); 5] = [
        ("json", &["report.json"]),
        ("html", &["report.html"]),
        ("csv", &["files.csv", "procedures.csv", "signals.csv"]),
        (
            "csv,json",
            &["files.csv", "procedures.csv", "report.json", "signals.csv"],
        ),
        // A format that is named twice is written once.
        ("html,html", &["report.html"]),
    ];

    for (n, (format, expected)) in cases.into_iter().enumerate() {
        let out = format!("out-{n}");

        let output = run_in(&dir, &["scan", "m.bas", "-o", &out, "--format", format]);

        assert_eq!(output.status.code(), Some(0), "{format}: {output:?}");
        assert_eq!(written(&dir.join(&out)), expected, "{format}");
    }
}

#[test]
fn the_html_report_follows_the_language() {
    let dir = scratch("scan_html_lang");
    put(&dir, "m.bas", MODULE);

    let output = run_in(
        &dir,
        &[
            "scan", "m.bas", "-o", "out", "--format", "html", "--lang", "fr",
        ],
    );

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let html = fs::read_to_string(dir.join("out/report.html")).expect("report.html is UTF-8");
    assert!(html.contains("<html lang=\"fr\">"));
}

#[test]
fn scan_exits_with_2_on_an_unknown_format_and_writes_nothing() {
    let dir = scratch("scan_unknown_format");
    put(&dir, "m.bas", MODULE);

    let output = run_in(
        &dir,
        &["scan", "m.bas", "-o", "out", "--format", "json,xml"],
    );

    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(!dir.join("out").exists());
}

#[test]
fn the_corpus_is_scanned_into_every_report() {
    let out = scratch("scan_corpus").join("out");

    let output = run(&[
        "scan",
        "corpus/",
        "-o",
        as_arg(&out),
        "--format",
        "json,html,csv",
    ]);

    // SPEC M4 and A6: 0, or 1 when some of the files failed.
    let report = read_report(&out);
    let expected_code = i32::from(report.scan.files_failed > 0);
    assert_eq!(output.status.code(), Some(expected_code), "{output:?}");
    assert_eq!(written(&out), EVERY_REPORT);
    let expected = corpus::files("", &EXTENSIONS).len();
    assert_eq!(usize::try_from(report.scan.files_total), Ok(expected));
    // A row for each file, and the header: no cell of these files has a line break.
    let csv = fs::read_to_string(out.join("files.csv")).expect("files.csv is UTF-8");
    assert_eq!(
        csv.lines().count(),
        usize::try_from(report.scan.files_total).expect("a small count") + 1
    );
}

/// A6 takes the malformed fixtures to end the scan with code 1. Until
/// `analyze()` reads the containers (ticket #36), it reports each file as
/// unsupported, none as corrupted, so the scan ends with 0.
#[test]
#[ignore = "needs analyze() to read the containers (ticket #36)"]
fn the_malformed_fixtures_end_the_scan_of_the_corpus_with_code_1() {
    let out = scratch("scan_corpus_malformed").join("out");

    let output = run(&[
        "scan",
        "corpus/",
        "-o",
        as_arg(&out),
        "--format",
        "json,html,csv",
    ]);

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(read_report(&out).scan.files_failed > 0);
}
