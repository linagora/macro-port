// SPDX-License-Identifier: Apache-2.0
//! [`analyze`]: the bytes of one file in, its [`FileReport`] out.
//!
//! This is the extraction stage (SPEC M1): what kind of file it is, whether it
//! can be read, and the modules of its VBA project with their source. The
//! stages that read the source (M2, M3) fill the fields that extraction leaves
//! empty. [`extract`] runs the extraction alone and also hands back the source
//! of each module, which the report does not carry.
//!
//! Nothing here executes or evaluates anything from the file, and nothing
//! panics on its content: every failure is a status and an error code in the
//! report. The codes that this module gives, besides those of the container
//! and MS-OVBA layers:
//!
//! | code | status | meaning |
//! | --- | --- | --- |
//! | `file.too_large` | `too_large` | the file is above [`Options::max_file_size`] |
//! | `file.unsupported_format` | `unsupported` | the format of the file is not read |
//!
//! Only the first of them is raised so far: the formats are read by the next
//! changes, and every other file is `unsupported` until then.

mod detect;

use sha2::{Digest, Sha256};

use crate::report::{FileError, FileReport, Orientation, Status};
use crate::rules::RuleSet;

/// The largest file that [`analyze`] reads by default: 200 MiB. SPEC M1 says
/// 200 MB, and like the other limits of the container layer, it takes a
/// megabyte for 1,048,576 bytes.
pub const DEFAULT_MAX_FILE_SIZE: u64 = 200 * 1024 * 1024;

/// What the caller decides about an analysis.
#[derive(Debug, Clone, Copy)]
pub struct Options<'a> {
    /// The rules. Nothing reads them yet: the container indicators that
    /// extraction will raise take their severity from them.
    pub rules: &'a RuleSet,
    /// The largest file, in bytes, that is read. A larger one gets the status
    /// `too_large` and is not opened: its report still gives its size and
    /// hash.
    pub max_file_size: u64,
}

impl<'a> Options<'a> {
    /// The default options for `rules`: files up to [`DEFAULT_MAX_FILE_SIZE`].
    pub fn new(rules: &'a RuleSet) -> Self {
        Self {
            rules,
            max_file_size: DEFAULT_MAX_FILE_SIZE,
        }
    }
}

/// What [`extract`] found in a file.
#[derive(Debug, Clone, PartialEq)]
pub struct Extraction {
    /// The report of the file, as far as extraction fills it.
    pub file: FileReport,
    /// The source of each module of `file.vba_project`, in the same order: the
    /// text that the line numbers of the evidence count in. `None` for a
    /// module whose source could not be read. Empty when the file has no VBA
    /// project.
    pub sources: Vec<Option<String>>,
}

/// Analyses one file: `bytes` are its content, `path` its path as the report
/// gives it (its extension also picks the variant of the format).
///
/// The result never fails: a file that cannot be read gets a status other
/// than `ok` and `no_macros`, and the errors that explain it.
pub fn analyze(bytes: &[u8], path: &str, options: &Options<'_>) -> FileReport {
    extract(bytes, path, options).file
}

/// Runs the extraction stage alone.
///
/// A file above [`Options::max_file_size`] gets the status `too_large` and is
/// not opened. Reading the formats themselves comes with the next changes:
/// until then every other file gets the status `unsupported`.
pub fn extract(bytes: &[u8], path: &str, options: &Options<'_>) -> Extraction {
    let size = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    let sha256 = sha256_hex(bytes);
    let file = if size > options.max_file_size {
        too_large(path, size, sha256, options)
    } else {
        let failure = Failure::new(
            Status::Unsupported,
            "file.unsupported_format",
            "the format of the file is not read yet",
        );
        failed_report(path, size, sha256, failure)
    };
    Extraction {
        file,
        sources: Vec::new(),
    }
}

/// The report of a file above [`Options::max_file_size`], given its `size` and
/// the `sha256` of its content.
///
/// [`analyze`] gives it for the files it is handed. A caller that does not
/// want to hold such a file in memory can read its size from the file system,
/// hash it as it streams it, and ask for this report instead.
pub fn too_large(path: &str, size: u64, sha256: String, options: &Options<'_>) -> FileReport {
    let failure = Failure::new(
        Status::TooLarge,
        "file.too_large",
        format!(
            "the file is larger than the limit of {} bytes",
            options.max_file_size
        ),
    );
    failed_report(path, size, sha256, failure)
}

/// The report of a file that could not be read.
fn failed_report(path: &str, size: u64, sha256: String, failure: Failure) -> FileReport {
    FileReport {
        path: path.to_owned(),
        size,
        sha256,
        container: detect::container_by_extension(&detect::extension(path)),
        status: failure.status,
        errors: Some(vec![failure.error]),
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

/// The SHA-256 of `bytes` as 64 lowercase hexadecimal digits.
fn sha256_hex(bytes: &[u8]) -> String {
    let digit = |nibble: u8| char::from_digit(u32::from(nibble), 16).unwrap_or('0');
    Sha256::digest(bytes)
        .iter()
        .flat_map(|byte| [digit(byte >> 4), digit(byte & 0x0F)])
        .collect()
}

/// Why a file could not be read: the status it gets, and the error that its
/// report lists.
struct Failure {
    status: Status,
    error: FileError,
}

impl Failure {
    fn new(status: Status, code: &str, message: impl Into<String>) -> Self {
        Self {
            status,
            error: FileError {
                code: code.to_owned(),
                message: message.into(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_the_way_sha256_does() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }
}
