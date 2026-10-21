// SPDX-License-Identifier: Apache-2.0
//! `macro-port scan`: the files under the given paths, analysed in parallel and
//! reported together (SPEC M4).
//!
//! Everything impure is here, since the core is pure: the walk of the
//! directories, the reading of the files, the threads and the clock.

mod analyse;
mod walk;

use std::path::{Path, PathBuf};
use std::time::Instant;

use macroport_core::report::{Build, Lang, Report};
use macroport_core::rules::RuleSet;

use crate::clock::Clock;

pub use analyse::{PANIC_CODE, Progress, UNREADABLE_CODE};
pub use walk::{Candidate, EXTENSIONS, report_path, walk};

/// What a scan needs to know besides the paths.
#[derive(Debug, Clone, Copy)]
pub struct Settings<'a> {
    pub rules: &'a RuleSet,
    /// The largest file, in bytes, that is read.
    pub max_file_size: u64,
    /// The most threads that analyse files at the same time.
    pub jobs: usize,
    pub lang: Lang,
    pub clock: Clock,
}

/// Scans `paths` and gives the fleet report.
///
/// # Errors
///
/// When the threads of the analysis cannot be started.
pub fn scan(
    paths: &[PathBuf],
    settings: &Settings<'_>,
    progress: &(dyn Fn(Progress) + Sync),
) -> Result<Report, rayon::ThreadPoolBuildError> {
    let started = Instant::now();
    let candidates = walk(paths);
    let files = analyse::analyse(&candidates, settings, progress)?;
    let clock = settings.clock;
    let mut report = Report::from_files(settings.rules, clock.generated_at(), settings.lang, files);
    report.tool.build = Some(Build::Cli);
    report.scan.roots = Some(paths.iter().map(|root| root_path(root)).collect());
    report.scan.duration_ms = clock.duration_ms(started);
    Ok(report)
}

/// A root as the report gives it, like the paths of the files under it.
fn root_path(root: &Path) -> String {
    report_path(&root.components().collect::<PathBuf>())
}
