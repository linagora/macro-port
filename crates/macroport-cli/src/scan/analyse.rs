// SPDX-License-Identifier: Apache-2.0
//! The analysis of a scan: each candidate file is read and analysed on a pool
//! of threads, and what the core cannot say about a file is said here.

use std::fs::File;
use std::io::{self, ErrorKind, Read};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use macroport_core::analyze::{self, Options};
use macroport_core::report::{Container, FileError, FileReport, Orientation, Status};
use rayon::prelude::*;
use rayon::{ThreadPoolBuildError, ThreadPoolBuilder};
use sha2::{Digest, Sha256};

use super::Settings;
use super::walk::Candidate;

/// The stack of each thread of the pool: 16 MiB, where the default is 2 MiB.
/// A file is hostile, and the recursion that nested blocks make in the parser
/// has to end in an error, not in a stack overflow that no code can catch.
const STACK_SIZE: usize = 16 * 1024 * 1024;

/// The block in which a file that is too large is read to be hashed: 64 KiB.
const BLOCK_SIZE: usize = 64 * 1024;

/// The most memory, in bytes, that is reserved for a file before it is read: a
/// file grows its buffer from there, as the data comes.
const MOST_RESERVED: u64 = 256 * 1024 * 1024;

/// The error code of a file whose analysis panicked.
pub const PANIC_CODE: &str = "internal.panic";

/// The error code of a file that could not be read.
pub const UNREADABLE_CODE: &str = "file.unreadable";

/// How far a scan is, as files finish.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    pub done: usize,
    pub total: usize,
}

/// Analyses `candidates` on a pool of at most `settings.jobs` threads.
///
/// Gives a report for each candidate, in the order of `candidates`: how many
/// threads ran has no effect on the result. `progress` is called with 0 files
/// done, then once as each file finishes, from the thread that analysed it.
///
/// # Errors
///
/// When the pool of threads cannot be created.
pub fn analyse(
    candidates: &[Candidate],
    settings: &Settings<'_>,
    progress: &(dyn Fn(Progress) + Sync),
) -> Result<Vec<FileReport>, ThreadPoolBuildError> {
    let total = candidates.len();
    progress(Progress { done: 0, total });
    if candidates.is_empty() {
        return Ok(Vec::new());
    }
    // No thread for which there is no file.
    let pool = ThreadPoolBuilder::new()
        .num_threads(settings.jobs.min(total))
        .stack_size(STACK_SIZE)
        .build()?;
    let done = AtomicUsize::new(0);
    Ok(pool.install(|| {
        candidates
            .par_iter()
            .map(|candidate| {
                let file = analyse_one(candidate, settings);
                let finished = done.fetch_add(1, Ordering::Relaxed).saturating_add(1);
                progress(Progress {
                    done: finished,
                    total,
                });
                file
            })
            .collect()
    }))
}

/// The report of one candidate.
fn analyse_one(candidate: &Candidate, settings: &Settings<'_>) -> FileReport {
    if let Some(reason) = &candidate.unreadable {
        return failure(&candidate.path, 0, UNREADABLE_CODE, reason);
    }
    let mut options = Options::new(settings.rules);
    options.max_file_size = settings.max_file_size;
    match read(&candidate.location, settings.max_file_size) {
        Input::Bytes(bytes) => analyse_bytes(&bytes, &candidate.path, &options, &dispatch),
        Input::TooLarge { size, sha256 } => {
            analyze::too_large(&candidate.path, size, sha256, &options)
        }
        Input::Unreadable(error) => {
            failure(&candidate.path, 0, UNREADABLE_CODE, &error.to_string())
        }
    }
}

/// How the bytes of a file become its report.
type Analyser = dyn Fn(&[u8], &str, &Options<'_>) -> FileReport;

/// The report of the file `path`, which holds `bytes`: the entry point of the
/// core that reads this kind of file is chosen here, and nowhere else.
///
/// Every file goes through [`analyze::analyze`], the text modules (`.bas`,
/// `.cls`, `.frm`) included. They are meant for `analyze_modules`, which the
/// core does not have yet (M2): until then `analyze` reports them as it does,
/// today as `unsupported`, and the switch is a change of this function.
fn dispatch(bytes: &[u8], path: &str, options: &Options<'_>) -> FileReport {
    analyze::analyze(bytes, path, options)
}

/// The report that `analyser` gives for `bytes`, the content of the file
/// `path`.
///
/// The core is written not to panic. A panic is still a bug in one file, not a
/// reason to lose the report on all the others: it becomes the failure of that
/// file, which keeps its size and its hash.
fn analyse_bytes(
    bytes: &[u8],
    path: &str,
    options: &Options<'_>,
    analyser: &Analyser,
) -> FileReport {
    catch_unwind(AssertUnwindSafe(|| analyser(bytes, path, options))).unwrap_or_else(|_| {
        let mut file = failure(
            path,
            byte_count(bytes.len()),
            PANIC_CODE,
            "the analysis of the file failed unexpectedly",
        );
        file.sha256 = hex(&Sha256::digest(bytes));
        file
    })
}

/// What reading a file gave.
enum Input {
    Bytes(Vec<u8>),
    /// The file is above the limit: it is hashed, not kept.
    TooLarge {
        size: u64,
        sha256: String,
    },
    Unreadable(io::Error),
}

/// Reads the file at `path`, unless it is larger than `max` bytes. A file that
/// is too large is streamed through the hash, so that memory stays bounded.
fn read(path: &Path, max: u64) -> Input {
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(error) => return Input::Unreadable(error),
    };
    let size = match file.metadata() {
        Ok(metadata) => metadata.len(),
        Err(error) => return Input::Unreadable(error),
    };
    if size > max {
        return match hash(&mut file, size) {
            Ok(sha256) => Input::TooLarge { size, sha256 },
            Err(error) => Input::Unreadable(error),
        };
    }
    // One byte more than the limit, so that a file that grew since its size
    // was read is still caught by the core's limit.
    let room = usize::try_from(size.min(MOST_RESERVED)).unwrap_or_default();
    let mut bytes = Vec::with_capacity(room);
    match file.take(max.saturating_add(1)).read_to_end(&mut bytes) {
        Ok(_) => Input::Bytes(bytes),
        Err(error) => Input::Unreadable(error),
    }
}

/// The SHA-256 of the first `size` bytes of `file`, as lowercase hexadecimal
/// digits, read in blocks. A file that grows is not followed forever.
fn hash(file: &mut File, size: u64) -> io::Result<String> {
    let mut hasher = Sha256::new();
    let mut reader = file.take(size);
    let mut block = vec![0_u8; BLOCK_SIZE];
    loop {
        match reader.read(&mut block) {
            Ok(0) => break,
            Ok(read) => hasher.update(block.get(..read).unwrap_or_default()),
            Err(error) if error.kind() == ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(hex(&hasher.finalize()))
}

/// The report of a file that could not be analysed: its status is `error`,
/// and `message` says why. The hash is zeros when the content was not read.
fn failure(path: &str, size: u64, code: &str, message: &str) -> FileReport {
    FileReport {
        path: path.to_owned(),
        size,
        sha256: "0".repeat(64),
        container: Container::Unknown,
        status: Status::Error,
        errors: Some(vec![FileError {
            code: code.to_owned(),
            message: message.to_owned(),
        }]),
        vba_project: None,
        xlm: None,
        metrics: None,
        complexity_level: None,
        primary_family: None,
        families: None,
        max_severity: None,
        signals: None,
        traits: None,
        orientation: Orientation,
        workbook_encrypted: None,
        notes: None,
    }
}

fn byte_count(len: usize) -> u64 {
    u64::try_from(len).unwrap_or(u64::MAX)
}

/// `bytes` as lowercase hexadecimal digits.
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use macroport_core::rules::{RuleSet, RulesSources};

    use super::*;

    /// The SHA-256 of "abc", from FIPS 180-2.
    const SHA256_OF_ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    fn rules() -> RuleSet {
        RuleSet::load(&RulesSources::EMBEDDED).expect("the embedded rules load")
    }

    #[test]
    fn a_panic_in_the_analysis_is_the_failure_of_that_file() {
        let rules = rules();
        let options = Options::new(&rules);

        let file = analyse_bytes(b"abc", "dir/a.bas", &options, &|_, _, _| {
            panic!("a bug in the core")
        });

        assert_eq!(file.path, "dir/a.bas");
        assert_eq!(file.status, Status::Error);
        assert_eq!(file.size, 3);
        assert_eq!(file.sha256, SHA256_OF_ABC);
        let errors = file.errors.expect("the failure carries its error");
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, PANIC_CODE);
        // The message does not repeat the panic: it may quote the file.
        assert!(!errors[0].message.contains("a bug in the core"));
    }

    #[test]
    fn a_report_that_the_analysis_gives_is_kept() {
        let rules = rules();
        let options = Options::new(&rules);

        let file = analyse_bytes(b"abc", "a.bas", &options, &analyze::analyze);

        assert_eq!(file.path, "a.bas");
        assert_eq!(file.size, 3);
        assert_eq!(file.sha256, SHA256_OF_ABC);
        assert!(file.errors.iter().flatten().all(|e| e.code != PANIC_CODE));
    }

    #[test]
    fn the_hash_of_a_failure_is_zeros_when_the_content_was_not_read() {
        let file = failure("a.bas", 0, UNREADABLE_CODE, "denied");

        assert_eq!(file.sha256, "0".repeat(64));
        assert_eq!(file.status, Status::Error);
        assert_eq!(file.container, Container::Unknown);
    }
}
