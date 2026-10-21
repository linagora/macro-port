// SPDX-License-Identifier: Apache-2.0
//! The analysis of a scan, through the library: what it says of a file it
//! cannot read or will not read, the progress it reports, who made the report,
//! and that the number of threads has no effect on the result.

mod support;

use std::fs::File;
use std::sync::Mutex;

use macroport_cli::clock::{Clock, DETERMINISTIC_GENERATED_AT};
use macroport_cli::scan::{self, Progress, Settings, report_path};
use macroport_core::analyze::Options;
use macroport_core::report::{Build, Lang, Report, Status};
use macroport_core::rules::{RuleSet, RulesSources};
use macroport_testkit::report_problems;
use sha2::{Digest, Sha256};
use support::{put, scratch};

fn rules() -> RuleSet {
    RuleSet::load(&RulesSources::EMBEDDED).expect("the embedded rules load")
}

fn settings(rules: &RuleSet, jobs: usize) -> Settings<'_> {
    Settings {
        options: Options::new(rules),
        jobs,
        lang: Lang::En,
        clock: Clock::Fixed,
    }
}

/// The JSON of `report`, which the schema accepts.
fn valid_json(report: &Report) -> String {
    let json = report.to_json().expect("the report serialises");
    assert_eq!(report_problems(&json), Vec::<String>::new());
    json
}

#[cfg(unix)]
#[test]
fn a_file_that_cannot_be_read_is_an_error_of_that_file_only() {
    let dir = scratch("analyse_unreadable");
    put(&dir, "tree/a.bas", b"Sub A()\nEnd Sub\n");
    let secret = put(&dir, "tree/b.bas", b"Sub B()\nEnd Sub\n");
    let Some(_denied) = support::deny_access(&secret) else {
        return;
    };
    let rules = rules();

    let report =
        scan::scan(&[dir.join("tree")], &settings(&rules, 2), &|_| {}).expect("the threads start");

    assert_eq!(report.scan.files_total, 2);
    assert_eq!(report.scan.files_failed, 1);
    let failed = &report.files[1];
    assert!(failed.path.ends_with("tree/b.bas"));
    assert_eq!(failed.status, Status::Error);
    let errors = failed.errors.as_deref().expect("the failure has its error");
    assert_eq!(errors[0].code, scan::UNREADABLE_CODE);
    assert_ne!(report.files[0].status, Status::Error);
    valid_json(&report);
}

#[test]
fn a_file_over_the_limit_is_hashed_and_reported_too_large() {
    let dir = scratch("analyse_too_large");
    let size: u64 = 2 * 1024 * 1024;
    let path = dir.join("big.bas");
    File::create(&path)
        .and_then(|file| file.set_len(size))
        .expect("a sparse file");
    let rules = rules();
    let mut settings = settings(&rules, 1);
    settings.options.max_file_size = 1024 * 1024;

    let report = scan::scan(&[path], &settings, &|_| {}).expect("the threads start");

    let file = &report.files[0];
    assert_eq!(file.status, Status::TooLarge);
    assert_eq!(file.size, size);
    let zeros = vec![0_u8; usize::try_from(size).expect("a size that fits")];
    let digest: String = (Sha256::digest(&zeros).iter())
        .map(|byte| format!("{byte:02x}"))
        .collect();
    assert_eq!(file.sha256, digest);
    assert_eq!(report.scan.files_failed, 1);
    valid_json(&report);
}

#[test]
fn a_file_of_the_size_of_the_limit_is_read_and_one_byte_more_is_too_large() {
    let dir = scratch("analyse_limit");
    let limit = 1024_u64;
    let at_the_limit = put(&dir, "at.bas", &vec![b'\n'; 1024]);
    let over_the_limit = put(&dir, "over.bas", &vec![b'\n'; 1025]);
    let rules = rules();
    let mut settings = settings(&rules, 2);
    settings.options.max_file_size = limit;

    let report =
        scan::scan(&[at_the_limit, over_the_limit], &settings, &|_| {}).expect("the threads start");

    let status = |name: &str| {
        let found = report.files.iter().find(|file| file.path.ends_with(name));
        found.expect("the file is reported").status
    };
    assert_ne!(status("at.bas"), Status::TooLarge);
    assert_eq!(status("over.bas"), Status::TooLarge);
    assert_eq!(report.files.iter().map(|file| file.size).sum::<u64>(), 2049);
    valid_json(&report);
}

#[test]
fn the_report_is_the_same_whatever_the_number_of_threads() {
    let dir = scratch("analyse_jobs");
    for n in 0..40 {
        let module = format!("Sub S{n}()\n    Range(\"A{n}\").Value = {n}\nEnd Sub\n");
        put(&dir, &format!("tree/m{n:02}.bas"), module.as_bytes());
    }
    let rules = rules();
    let json = |jobs| {
        let report = scan::scan(&[dir.join("tree")], &settings(&rules, jobs), &|_| {})
            .expect("the threads start");
        valid_json(&report)
    };

    let one = json(1);

    for jobs in [2, 3, 8, 64] {
        assert_eq!(json(jobs), one, "{jobs} threads");
    }
}

#[test]
fn progress_goes_from_nothing_done_to_everything_done_in_order() {
    let dir = scratch("analyse_progress");
    for n in 0..25 {
        put(&dir, &format!("tree/m{n:02}.bas"), b"Sub S()\nEnd Sub\n");
    }
    let rules = rules();
    let seen = Mutex::new(Vec::new());

    scan::scan(&[dir.join("tree")], &settings(&rules, 4), &|progress| {
        seen.lock()
            .expect("the lock is not poisoned")
            .push(progress);
    })
    .expect("the threads start");

    let seen = seen.into_inner().expect("the lock is not poisoned");
    let expected: Vec<Progress> = (0..=25).map(|done| Progress { done, total: 25 }).collect();
    assert_eq!(seen, expected);
}

#[test]
fn a_scan_of_nothing_reports_no_file_and_no_progress_beyond_zero() {
    let dir = scratch("analyse_nothing");
    let rules = rules();
    let seen = Mutex::new(Vec::new());

    let report = scan::scan(&[dir], &settings(&rules, 4), &|progress| {
        seen.lock()
            .expect("the lock is not poisoned")
            .push(progress);
    })
    .expect("the threads start");

    assert!(report.files.is_empty());
    assert_eq!(report.scan.files_total, 0);
    assert_eq!(
        seen.into_inner().expect("the lock is not poisoned"),
        [Progress { done: 0, total: 0 }]
    );
    valid_json(&report);
}

#[test]
fn the_report_says_that_the_cli_made_it_and_from_which_roots() {
    let dir = scratch("analyse_roots");
    put(&dir, "tree/a.bas", b"Sub A()\nEnd Sub\n");
    let rules = rules();
    // A trailing separator and the same root twice: the report names the root
    // once as often as it was given, like the paths of the files.
    let roots = [dir.join("tree/"), dir.join("tree")];

    let report = scan::scan(&roots, &settings(&rules, 1), &|_| {}).expect("the threads start");

    assert_eq!(report.tool.build, Some(Build::Cli));
    let root = report_path(&dir.join("tree"));
    assert_eq!(report.scan.roots, Some(vec![root.clone(), root]));
    assert_eq!(report.scan.files_total, 1);
}

#[test]
fn a_deterministic_clock_fixes_the_date_and_leaves_out_the_duration() {
    let dir = scratch("analyse_clock");
    put(&dir, "a.bas", b"Sub A()\nEnd Sub\n");
    let rules = rules();
    let mut settings = settings(&rules, 1);

    let roots = [dir];

    let fixed = scan::scan(&roots, &settings, &|_| {}).expect("the threads start");
    settings.clock = Clock::System;
    let system = scan::scan(&roots, &settings, &|_| {}).expect("the threads start");

    assert_eq!(fixed.generated_at, DETERMINISTIC_GENERATED_AT);
    assert_eq!(fixed.scan.duration_ms, None);
    assert_ne!(system.generated_at, DETERMINISTIC_GENERATED_AT);
    assert!(system.scan.duration_ms.is_some());
    valid_json(&fixed);
    valid_json(&system);
}
