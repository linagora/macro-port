// SPDX-License-Identifier: Apache-2.0
//! MS-OVBA, the format of a VBA project inside a CFB container ([MS-OVBA],
//! Office VBA File Format Structure).
//!
//! Pure functions over byte slices: the caller reads the streams out of the
//! container and hands over their bytes. Every input is hostile, so each
//! function bounds what it reads and never panics. Most return an
//! [`OvbaError`] for a stream they cannot read; [`parse_project`] never fails,
//! as the analysis goes on without what the `PROJECT` stream adds.

mod decompress;
mod dir;
mod encryption;
mod project;
mod reader;
mod reference;
mod text;

use crate::report::Status;

pub use decompress::decompress;
pub use dir::{DirInfo, DirModule, parse_compressed_dir, parse_dir};
pub use project::{ProjectInfo, parse_project};

/// The largest decompressed `dir` stream (SPEC M1).
pub const MAX_DIR_STREAM: usize = 16 * 1024 * 1024;

/// The most modules a project may declare (SPEC M1).
pub const MAX_MODULES: usize = 2_000;

/// The most references a project may declare. SPEC M1 sets no limit, so this
/// one mirrors that of modules.
pub const MAX_REFERENCES: usize = 2_000;

/// The most bytes of the `PROJECT` stream that are read. SPEC M1 sets no limit
/// for it beyond the 64 MB of any stream of the compound file, so this one
/// mirrors that of the `dir` stream: far more than the properties of a project
/// take, and a bound on what decoding a stream made to be large can cost.
pub const MAX_PROJECT_STREAM: usize = 16 * 1024 * 1024;

/// Why a stream of a VBA project could not be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{code}: {message}")]
pub struct OvbaError {
    /// The status the file gets in the report.
    pub status: Status,
    /// Stable code, lowercase and dotted as `<layer>.<problem>`, such as
    /// `ovba.decompress.truncated`.
    pub code: &'static str,
    /// What went wrong.
    pub message: String,
}

impl OvbaError {
    /// An error giving the file `status`.
    pub fn new(status: Status, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
        }
    }

    /// A stream whose structure is broken.
    pub(crate) fn corrupted(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(Status::Corrupted, code, message)
    }
}
