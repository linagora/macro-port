// SPDX-License-Identifier: Apache-2.0
//! The analysis of a scan, through the library: what it says of a file it
//! cannot read or will not read, the progress it reports, and that the number
//! of threads has no effect on the result.

mod support;

use std::fs::File;
use std::sync::Mutex;

use macroport_cli::clock::Clock;
use macroport_cli::scan::{self, Progress, Settings};
use macroport_core::analyze::DEFAULT_MAX_FILE_SIZE;
use macroport_core::report::{Lang, Status};
use macroport_core::rules::{RuleSet, RulesSources};
use sha2::{Digest, Sha256};
use support::{put, scratch};

fn rules() -> RuleSet {
    RuleSet::load(&RulesSources::EMBEDDED).expect("the embedded rules load")
}

fn settings(rules: &RuleSet, jobs: usize) -> Settings<'_> {
    Settings {
        rules,
        max_file_size: DEFAULT_MAX_FILE_SIZE,
        jobs,
        lang: Lang::En,
        clock: Clock::new(true),
    }
}

#[cfg(unix)]
#[test]
fn a_file_that_cannot_be_read_is_an_error_of_that_file_only() {
    let dir = scratch("scan_unreadable");
    put(&dir, "tree/a.bas", b"Sub A()\nEnd Sub\n");
    let secret = put(&dir, "tree/b.bas", b"Sub B()\nEnd Sub\n");
    if !support::lock(&secret) {
        return;
    }
    let rules = rules();

    let report = scan::scan(&[dir.join("tree")], &settings(&rules, 2), &|_| {});
    support::unlock(&secret);
    let report = report.expect("the threads start");

    assert_eq!(report.scan.files_total, 2);
    assert_eq!(report.scan.files_failed, 1);
    let failed = &report.files[1];
    assert!(failed.path.ends_with("tree/b.bas"));
    assert_eq!(failed.status, Status::Error);
    let errors = failed.errors.as_deref().expect("the failure has its error");
    assert_eq!(errors[0].code, scan::UNREADABLE_CODE);
    assert_ne!(report.files[0].status, Status::Error);
}

#[test]
fn a_file_over_the_limit_is_hashed_and_reported_too_large() {
    let dir = scratch("scan_too_large");
    let size: u64 = 2 * 1024 * 1024;
    let path = dir.join("big.bas");
    File::create(&path)
        .and_then(|file| file.set_len(size))
        .expect("a sparse file");
    let rules = rules();
    let mut settings = settings(&rules, 1);
    settings.max_file_size = 1024 * 1024;

    let report = scan::scan(&[path], &settings, &|_| {}).expect("the threads start");

    let file = &report.files[0];
    assert_eq!(file.status, Status::TooLarge);
    assert_eq!(file.size, size);
    let zeros = vec![0_u8; usize::try_from(size).expect("a size that fits")];
    let digest: String = Sha256::digest(&zeros)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    assert_eq!(file.sha256, digest);
    assert_eq!(report.scan.files_failed, 1);
}

#[test]
fn the_report_is_the_same_whatever_the_number_of_threads() {
    let dir = scratch("scan_jobs");
    for n in 0..40 {
        let module = format!("Sub S{n}()\n    Range(\"A{n}\").Value = {n}\nEnd Sub\n");
        put(&dir, &format!("tree/m{n:02}.bas"), module.as_bytes());
    }
    let rules = rules();
    let json = |jobs| {
        scan::scan(&[dir.join("tree")], &settings(&rules, jobs), &|_| {})
            .expect("the threads start")
            .to_json()
            .expect("the report serialises")
    };

    let one = json(1);

    for jobs in [2, 3, 8, 64] {
        assert_eq!(json(jobs), one, "{jobs} threads");
    }
}

#[test]
fn progress_goes_from_nothing_done_to_everything_done() {
    let dir = scratch("scan_progress");
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

    let mut seen = seen.into_inner().expect("the lock is not poisoned");
    // The calls of different threads are not ordered, the counts are.
    seen.sort_by_key(|progress| progress.done);
    let expected: Vec<Progress> = (0..=25).map(|done| Progress { done, total: 25 }).collect();
    assert_eq!(seen, expected);
}

#[test]
fn a_scan_of_nothing_reports_no_file_and_no_progress_beyond_zero() {
    let dir = scratch("scan_nothing");
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
}
