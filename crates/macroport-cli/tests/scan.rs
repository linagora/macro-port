// SPDX-License-Identifier: Apache-2.0
//! End-to-end tests of `macro-port scan`: the binary runs on the corpus and on
//! small trees, and the reports it writes are read back and validated.

mod support;

use std::fs::{self, File};
use std::io;
use std::path::Path;
use std::process::Command;

use macroport_cli::scan::EXTENSIONS;
use macroport_core::report::{Build, Status};
use macroport_testkit::corpus;
use serde_json::Value;
use support::{
    MODULE, as_arg, put, read_report, repo_root, report_json, run, run_in, scratch, text,
};

/// The `generated_at` of a deterministic report (SPEC M4).
const EPOCH: &str = "1970-01-01T00:00:00Z";

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

/// The columns of the CSV reports, in order (SPEC M4), by file.
const CSV_COLUMNS: [(&str, &str); 3] = [
    (
        "files.csv",
        "path,container,status,complexity_level,primary_family,max_severity,modules,procedures,\
         logical_loc,xlm_present,vba_project_protected",
    ),
    (
        "procedures.csv",
        "path,module,procedure,kind,primary_family,secondary_families,logical_loc,cyclomatic,\
         signals,traits",
    ),
    (
        "signals.csv",
        "path,module,procedure,signal_id,severity,line",
    ),
];

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
    // The text samples and the malformed files: the inputs with the most kinds
    // of outcome. At the base of this stack `analyze()` reads none of them and
    // every file comes out unsupported, so the CSV files of the procedures and
    // of the signals only hold their header; the comparison is the same.
    let inputs = ["corpus/families", "corpus/fixtures/malformed"];
    let mut runs: Vec<Vec<Vec<u8>>> = Vec::new();

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
        // Every report is written and compared, not only the JSON one.
        assert_eq!(written(&out), EVERY_REPORT, "{jobs} jobs");
        runs.push(
            EVERY_REPORT
                .iter()
                .map(|name| fs::read(out.join(name)).expect("a report can be read"))
                .collect(),
        );
    }

    for (n, pair) in runs.windows(2).enumerate() {
        for (name, (before, after)) in EVERY_REPORT.iter().zip(pair[0].iter().zip(&pair[1])) {
            assert!(
                before == after,
                "{name} differs between runs {n} and {}",
                n + 1
            );
        }
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
    // Every report is written all the same, not only the JSON one.
    assert_eq!(written(&dir.join("out")), EVERY_REPORT);
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
    let cases: [&[&str]; 10] = [
        &["scan"],
        &["scan", "missing.bas"],
        &["scan", "m.bas", "--jobs", "0"],
        &["scan", "m.bas", "--jobs", "many"],
        &["scan", "m.bas", "--max-file-size", "0"],
        &["scan", "m.bas", "--lang", "de"],
        &["scan", "m.bas", "--format", "json,xml"],
        &["scan", "m.bas", "--format", "csv,"],
        &["scan", "m.bas", "--format", ""],
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

#[test]
fn scan_writes_every_report_when_no_format_is_given() {
    let dir = scratch("scan_every_format");
    put(&dir, "m.bas", MODULE);

    let output = run_in(&dir, &["scan", "m.bas", "-o", "out"]);

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let out = dir.join("out");
    assert_eq!(written(&out), EVERY_REPORT);
    // The JSON passes the schema.
    read_report(&out);
}

#[test]
fn the_help_gives_the_default_formats_in_the_way_the_option_takes_them() {
    let output = run_in(&repo_root(), &["scan", "--help"]);

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(text(&output.stdout).contains("[default: json,html,csv]"));
}

#[test]
fn the_csv_reports_are_utf8_with_a_byte_order_mark_and_the_columns_of_the_spec() {
    let dir = scratch("scan_csv_columns");
    put(&dir, "m.bas", MODULE);

    let output = run_in(&dir, &["scan", "m.bas", "-o", "out", "--format", "csv"]);

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    for (name, columns) in CSV_COLUMNS {
        let csv = fs::read(dir.join("out").join(name)).expect("a CSV report can be read");
        assert!(csv.starts_with(BYTE_ORDER_MARK), "{name}");
        let text = std::str::from_utf8(&csv[BYTE_ORDER_MARK.len()..]).expect("the CSV is UTF-8");
        // RFC 4180: a line ends with CR LF.
        assert!(
            text.starts_with(&format!("{columns}\r\n")),
            "{name}: {text}"
        );
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
        if expected.contains(&"report.json") {
            // The JSON passes the schema.
            read_report(&dir.join(&out));
        }
    }
}

#[test]
fn the_html_report_is_one_page_in_the_language_asked_for() {
    let dir = scratch("scan_html_lang");
    put(&dir, "m.bas", MODULE);
    // English is the default.
    let cases: [(&[&str], &str); 2] = [(&[], "en"), (&["--lang", "fr"], "fr")];

    for (n, (options, lang)) in cases.into_iter().enumerate() {
        let out = format!("out-{n}");
        let mut args = vec!["scan", "m.bas", "-o", &out, "--format", "html"];
        args.extend(options);

        let output = run_in(&dir, &args);

        assert_eq!(output.status.code(), Some(0), "{lang}: {output:?}");
        let page = dir.join(&out).join("report.html");
        let html = fs::read_to_string(page).expect("report.html is UTF-8");
        assert!(html.starts_with("<!doctype html>"), "{lang}");
        assert!(html.contains(&format!("<html lang=\"{lang}\">")), "{lang}");
    }
}

#[test]
fn a_report_that_cannot_be_written_ends_the_writes_in_the_order_of_the_formats() {
    let dir = scratch("scan_write_order");
    put(&dir, "m.bas", MODULE);
    // A directory where the page goes. The order of the writes is json, html,
    // csv, whatever the order on the command line: the JSON is written, the CSV
    // files are not.
    fs::create_dir_all(dir.join("out/report.html")).expect("a directory named like the page");

    let output = run_in(
        &dir,
        &["scan", "m.bas", "-o", "out", "--format", "csv,html,json"],
    );

    assert_eq!(output.status.code(), Some(3), "{output:?}");
    let stderr = text(&output.stderr);
    assert!(
        stderr.contains("error: cannot write the file out/report.html: "),
        "{stderr}"
    );
    let out = dir.join("out");
    assert_eq!(written(&out), ["report.html", "report.json"]);
    read_report(&out);
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
    assert!(matches!(output.status.code(), Some(0 | 1)), "{output:?}");
    let report = read_report(&out);
    // The exit code follows the report; the test below imposes the 1 of A6.
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
