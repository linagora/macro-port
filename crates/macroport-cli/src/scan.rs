// SPDX-License-Identifier: Apache-2.0
//! `macro-port scan`: the files under the given paths, analysed in parallel and
//! reported together (SPEC M4).
//!
//! Everything impure is here, since the core is pure: the walk of the
//! directories, the reading of the files, the threads, the clock, the output
//! files and the console. The progress and the errors go to stderr; stdout only
//! gets the summary, once the reports are written.

mod analyse;
mod walk;

use std::collections::BTreeSet;
use std::fmt::Display;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use clap::Args as ClapArgs;
use macroport_core::analyze::Options;
use macroport_core::report::{Build, Lang, Report};
use macroport_core::rules::RuleSet;

use crate::args::{AnalysisArgs, Format, existing_path};
use crate::clock::Clock;
use crate::command;
use crate::console::{say_on_stderr, say_on_stdout};
use crate::i18n::Messages;
use crate::reports;

pub use analyse::{PANIC_CODE, Progress, UNREADABLE_CODE};
pub use walk::{Candidate, EXTENSIONS, report_path, walk};

/// The directory that the reports go to when `--output` is not given.
const DEFAULT_OUTPUT: &str = "macro-port-report";

/// The most threads that `--jobs` accepts.
const MOST_JOBS: i64 = 1024;

/// The arguments of `macro-port scan`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Files and directories to analyse. A directory is walked for the Excel
    /// files and VBA modules under it; the symbolic links it holds are not
    /// followed.
    #[arg(required = true, value_name = "PATH", value_parser = existing_path)]
    pub paths: Vec<PathBuf>,

    /// Directory that the reports are written to, created if need be.
    #[arg(short, long, value_name = "DIR", default_value = DEFAULT_OUTPUT)]
    pub output: PathBuf,

    /// Reports to write, separated by commas: `json` (report.json), `html`
    /// (report.html) and `csv` (files.csv, procedures.csv and signals.csv).
    #[arg(
        long,
        value_enum,
        value_delimiter = ',',
        value_name = "FORMAT",
        default_values_t = Format::ALL,
    )]
    pub format: Vec<Format>,

    /// How many files are analysed at the same time. The result is the same
    /// whatever the number. Defaults to the number of cores.
    #[arg(long, value_name = "N", value_parser = clap::value_parser!(u32).range(1..=MOST_JOBS))]
    pub jobs: Option<u32>,

    #[command(flatten)]
    pub analysis: AnalysisArgs,
}

/// What a scan needs to know besides the paths.
#[derive(Debug, Clone, Copy)]
pub struct Settings<'a> {
    /// The rules, and the largest file, in bytes, that is read.
    pub options: Options<'a>,
    /// The most threads that analyse files at the same time, and at least one.
    pub jobs: usize,
    pub lang: Lang,
    pub clock: Clock,
}

impl<'a> Settings<'a> {
    /// The settings of a command that analyses with `rules`: on `jobs` threads,
    /// as many as there are cores if it is none, and as `analysis` says.
    pub fn new(rules: &'a RuleSet, jobs: Option<u32>, analysis: &AnalysisArgs) -> Self {
        let cores = std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get);
        let mut options = Options::new(rules);
        options.max_file_size = analysis.max_file_size_bytes();
        Self {
            options,
            jobs: jobs.map_or(cores, |jobs| usize::try_from(jobs).unwrap_or(cores)),
            lang: analysis.lang(),
            clock: Clock::new(analysis.deterministic),
        }
    }
}

/// Runs `macro-port scan`.
pub fn run(args: &Args) -> ExitCode {
    let analysis = &args.analysis;
    command::run(
        analysis.lang(),
        analysis.rules.as_deref(),
        |rules, messages| execute(args, rules, messages),
    )
}

/// The scan, or the fatal error that stopped it.
fn execute(args: &Args, rules: &RuleSet, messages: &Messages) -> Result<ExitCode, String> {
    // The output directory is made before the scan, which can take minutes: a
    // path that cannot be made is known first. A file that cannot be written is
    // only known once the reports are.
    create_directory(&args.output, messages)?;
    // The messages are not shared between threads: the label is.
    let label = messages.text("cli-progress");
    let settings = Settings::new(rules, args.jobs, &args.analysis);
    let report = scan(&args.paths, &settings, &|progress| {
        print_progress(&label, progress);
    })
    .map_err(|error| messages.text_with("cli-error-threads", &[("reason", &error.to_string())]))?;

    write_reports(&args.output, &args.format, &report, messages)?;
    print_summary(&report, &args.output, messages);
    Ok(command::exit_code_of(&report))
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
    let rules = settings.options.rules;
    let mut report = Report::from_files(rules, clock.generated_at(), settings.lang, files);
    report.tool.build = Some(Build::Cli);
    report.scan.roots = Some(paths.iter().map(|root| root_path(root)).collect());
    report.scan.duration_ms = clock.duration_ms(started);
    Ok(report)
}

/// A root as the report gives it, like the paths of the files under it.
fn root_path(root: &Path) -> String {
    report_path(&root.components().collect::<PathBuf>())
}

/// Prints the progress of the scan on stderr: at the start, at every
/// twentieth of the files and at the end.
fn print_progress(label: &str, progress: Progress) {
    let Progress { done, total } = progress;
    let step = (total / 20).max(1);
    if total > 0 && (done == total || done % step == 0) {
        say_on_stderr(&format!("{label} {done} / {total}"));
    }
}

/// Creates `directory`, and the directories above it, if need be.
fn create_directory(directory: &Path, messages: &Messages) -> Result<(), String> {
    std::fs::create_dir_all(directory)
        .map_err(|error| path_error(messages, "cli-error-create", directory, &error))
}

/// Writes the files of `report` in each of `formats` to `directory`.
fn write_reports(
    directory: &Path,
    formats: &[Format],
    report: &Report,
    messages: &Messages,
) -> Result<(), String> {
    // A format that is asked for twice is written once.
    let formats: BTreeSet<Format> = formats.iter().copied().collect();
    for format in formats {
        for document in reports::documents(format, report, messages)? {
            let path = directory.join(document.name);
            std::fs::write(&path, document.contents)
                .map_err(|error| path_error(messages, "cli-error-write", &path, &error))?;
        }
    }
    Ok(())
}

/// The message `id` about `path`, with the reason `error`.
fn path_error(messages: &Messages, id: &str, path: &Path, error: &dyn Display) -> String {
    let path = path.display().to_string();
    messages.text_with(id, &[("path", &path), ("reason", &error.to_string())])
}

/// Prints the outcome of the scan on stdout.
fn print_summary(report: &Report, directory: &Path, messages: &Messages) {
    let totals = &report.scan;
    say_on_stdout(&messages.text_with(
        "cli-summary",
        &[
            ("files", &totals.files_total.to_string()),
            ("with_macros", &totals.files_with_macros.to_string()),
            ("failed", &totals.files_failed.to_string()),
        ],
    ));
    say_on_stdout(
        &messages.text_with("cli-written", &[("path", &directory.display().to_string())]),
    );
}
