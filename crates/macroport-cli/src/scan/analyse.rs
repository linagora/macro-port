// SPDX-License-Identifier: Apache-2.0
//! The analysis of a scan: each candidate file is read and analysed on a pool
//! of threads, and what the core cannot say about a file is said here.

use std::fs::File;
use std::io::{self, ErrorKind, Read};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::sync::{Mutex, PoisonError};

use macroport_core::analyze::{self, Options};
use macroport_core::report::{FileError, FileReport, Status};
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
const MAX_RESERVED: u64 = 256 * 1024 * 1024;

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

/// Analyses `candidates` on a pool of at most `settings.jobs` threads, and at
/// least one.
///
/// Gives a report for each candidate, in the order of `candidates`: how many
/// threads ran has no effect on the result. `progress` is called with 0 files
/// done, then once as each file finishes, from the thread that analysed it. The
/// calls come one at a time and in the order of their counts, so that the last
/// one says that all the files are done.
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
    // No thread for which there is no file, and not none at all: rayon reads 0
    // as "as many as there are cores".
    let threads = settings.jobs.min(total).max(1);
    let pool = ThreadPoolBuilder::new()
        .num_threads(threads)
        .stack_size(STACK_SIZE)
        .build()?;
    // The count is locked while `progress` runs, which orders the calls.
    let done = Mutex::new(0_usize);
    Ok(pool.install(|| {
        candidates
            .par_iter()
            .map(|candidate| {
                let file = analyse_one(candidate, settings);
                let mut finished = done.lock().unwrap_or_else(PoisonError::into_inner);
                *finished = finished.saturating_add(1);
                progress(Progress {
                    done: *finished,
                    total,
                });
                file
            })
            .collect()
    }))
}

/// The report of one candidate.
fn analyse_one(candidate: &Candidate, settings: &Settings<'_>) -> FileReport {
    let options = &settings.options;
    let path = &candidate.path;
    if let Some(reason) = &candidate.unreadable {
        return failure(path, Content::unread(), UNREADABLE_CODE, reason, options);
    }
    match read(&candidate.location, options.max_file_size) {
        Input::Bytes(bytes) => analyse_bytes(&bytes, path, options, &analyze::analyze),
        Input::TooLarge(content) => analyze::too_large(path, content.size, content.sha256, options),
        Input::Unreadable(error) => failure(
            path,
            Content::unread(),
            UNREADABLE_CODE,
            &error.to_string(),
            options,
        ),
    }
}

/// How the bytes of a file become its report.
type Analyser = dyn Fn(&[u8], &str, &Options<'_>) -> FileReport;

/// The report that `analyser` gives for `bytes`, the content of the file
/// `path`.
///
/// The analyser is always [`analyze::analyze`], for every file whatever its
/// extension: the core is given the name of the file with its bytes, and it is
/// the core that decides how to read it. The parameter is there so that a test
/// can put a panicking analyser in its place.
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
        let content = Content {
            size: byte_count(bytes.len()),
            sha256: hex(&Sha256::digest(bytes)),
        };
        failure(
            path,
            content,
            PANIC_CODE,
            "the analysis of the file failed unexpectedly",
            options,
        )
    })
}

/// What is known of the content of a file: its size and its SHA-256.
struct Content {
    size: u64,
    sha256: String,
}

impl Content {
    /// The content of a file that was not read: no size, and a hash of zeros.
    fn unread() -> Self {
        Self {
            size: 0,
            sha256: "0".repeat(64),
        }
    }
}

/// What reading a file gave.
enum Input {
    Bytes(Vec<u8>),
    /// The file is above the limit: it is hashed, not kept.
    TooLarge(Content),
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
            Ok(content) => Input::TooLarge(content),
            Err(error) => Input::Unreadable(error),
        };
    }
    // One byte more than the limit, so that a file that grew since its size
    // was read is still caught by the core's limit: it comes out too large with
    // the size and the hash of what was read.
    let room = usize::try_from(size.min(MAX_RESERVED)).unwrap_or_default();
    let mut bytes = Vec::with_capacity(room);
    match file.take(max.saturating_add(1)).read_to_end(&mut bytes) {
        Ok(_) => Input::Bytes(bytes),
        Err(error) => Input::Unreadable(error),
    }
}

/// The size and the SHA-256 of the first `limit` bytes of `file`, read in
/// blocks. A file that grows is not followed forever, and the size is that of
/// the bytes that were hashed, whatever the file system said before.
fn hash(file: &mut File, limit: u64) -> io::Result<Content> {
    let mut hasher = Sha256::new();
    let mut reader = file.take(limit);
    let mut block = vec![0_u8; BLOCK_SIZE];
    let mut size = 0_u64;
    loop {
        match reader.read(&mut block) {
            Ok(0) => break,
            Ok(filled) => {
                hasher.update(block.get(..filled).unwrap_or_default());
                size = size.saturating_add(byte_count(filled));
            }
            Err(error) if error.kind() == ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(Content {
        size,
        sha256: hex(&hasher.finalize()),
    })
}

/// The report of a file that could not be analysed: its status is `error`, and
/// `message` says why.
///
/// The core has no report for this case. The one that it gives for a file that
/// is too large has the right shape, the container taken from the extension and
/// nothing analysed, so its status and its errors are replaced.
fn failure(
    path: &str,
    content: Content,
    code: &str,
    message: &str,
    options: &Options<'_>,
) -> FileReport {
    FileReport {
        status: Status::Error,
        errors: Some(vec![FileError {
            code: code.to_owned(),
            message: message.to_owned(),
        }]),
        ..analyze::too_large(path, content.size, content.sha256, options)
    }
}

/// A length as a count of bytes. A length that does not fit is the largest
/// count, which no file reaches.
fn byte_count(len: usize) -> u64 {
    u64::try_from(len).unwrap_or(u64::MAX)
}

/// A digest as lowercase hexadecimal digits.
fn hex(digest: &[u8]) -> String {
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use macroport_core::report::{Container, Lang};
    use macroport_core::rules::{RuleSet, RulesSources};

    use super::*;
    use crate::clock::Clock;

    /// The SHA-256 of "abc", from FIPS 180-2.
    const SHA256_OF_ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

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

    /// `count` candidates whose files do not exist, named from `count - 1`
    /// down to 0: a file that cannot be opened is a report at once.
    fn missing(count: usize) -> Vec<Candidate> {
        (0..count)
            .rev()
            .map(|n| Candidate {
                path: format!("dir/f{n:03}.bas"),
                location: PathBuf::from(format!("/macro-port-test/missing/f{n:03}.bas")),
                unreadable: None,
            })
            .collect()
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
    fn a_failure_keeps_the_container_that_the_extension_names() {
        let rules = rules();
        let options = Options::new(&rules);

        let panicked = analyse_bytes(b"abc", "book.xlsm", &options, &|_, _, _| panic!("a bug"));
        let unread = failure("book.xls", Content::unread(), "x", "y", &options);
        let unknown = failure("notes.txt", Content::unread(), "x", "y", &options);

        assert_eq!(panicked.container, Container::Xlsm);
        assert_eq!(unread.container, Container::Xls);
        assert_eq!(unknown.container, Container::Unknown);
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
        let rules = rules();
        let options = Options::new(&rules);

        let file = failure(
            "a.bas",
            Content::unread(),
            UNREADABLE_CODE,
            "denied",
            &options,
        );

        assert_eq!(file.size, 0);
        assert_eq!(file.sha256, "0".repeat(64));
        assert_eq!(file.status, Status::Error);
    }

    #[test]
    fn a_candidate_that_the_walk_could_not_read_is_an_error_with_its_reason() {
        let rules = rules();
        let candidate = Candidate {
            path: "dir/locked".to_owned(),
            location: PathBuf::from("dir/locked"),
            unreadable: Some("denied".to_owned()),
        };

        let files =
            analyse(&[candidate], &settings(&rules, 1), &|_| {}).expect("the threads start");

        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "dir/locked");
        assert_eq!(files[0].status, Status::Error);
        let errors = files[0]
            .errors
            .as_deref()
            .expect("the failure has its error");
        assert_eq!(errors[0].code, UNREADABLE_CODE);
        assert_eq!(errors[0].message, "denied");
    }

    #[test]
    fn the_reports_come_in_the_order_of_the_candidates() {
        let rules = rules();
        let candidates = missing(300);

        let files = analyse(&candidates, &settings(&rules, 8), &|_| {}).expect("threads start");

        let paths: Vec<&str> = files.iter().map(|file| file.path.as_str()).collect();
        let expected: Vec<&str> = candidates.iter().map(|c| c.path.as_str()).collect();
        assert_eq!(paths, expected);
    }

    #[test]
    fn the_progress_calls_come_in_the_order_of_their_counts() {
        let rules = rules();
        let candidates = missing(600);
        let seen = Mutex::new(Vec::new());

        analyse(&candidates, &settings(&rules, 8), &|progress| {
            seen.lock()
                .expect("the lock is not poisoned")
                .push(progress);
        })
        .expect("the threads start");

        let seen = seen.into_inner().expect("the lock is not poisoned");
        let expected: Vec<Progress> = (0..=600)
            .map(|done| Progress { done, total: 600 })
            .collect();
        assert_eq!(seen, expected);
    }

    #[test]
    fn the_threads_are_jobs_but_not_more_than_files_and_not_none() {
        let rules = rules();
        for (jobs, files, expected) in [(0, 10, 1), (1, 10, 1), (3, 10, 3), (64, 5, 5)] {
            let sizes = Mutex::new(Vec::new());

            analyse(&missing(files), &settings(&rules, jobs), &|progress| {
                // The first call is made by the caller, outside the pool.
                if progress.done > 0 {
                    let mut sizes = sizes.lock().expect("the lock is not poisoned");
                    sizes.push(rayon::current_num_threads());
                }
            })
            .expect("the threads start");

            let sizes = sizes.into_inner().expect("the lock is not poisoned");
            assert_eq!(sizes.len(), files);
            assert!(
                sizes.iter().all(|&threads| threads == expected),
                "{jobs} jobs, {files} files, {expected} threads expected: {sizes:?}"
            );
        }
    }
}
