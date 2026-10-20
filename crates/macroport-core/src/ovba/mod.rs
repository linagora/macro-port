// SPDX-License-Identifier: Apache-2.0
//! MS-OVBA, the format of a VBA project inside a CFB container ([MS-OVBA],
//! Office VBA File Format Structure).
//!
//! Pure functions over byte slices: the caller reads the streams out of the
//! container and hands over their bytes. Every input is hostile, so each
//! function bounds what it reads and returns an [`OvbaError`] rather than
//! panicking.

mod decompress;
mod dir;
mod reader;
mod reference;
mod text;

use crate::report::Status;

pub use decompress::decompress;
pub use dir::{DirInfo, DirModule, parse_compressed_dir, parse_dir};

/// The largest decompressed `dir` stream (SPEC M1).
pub const MAX_DIR_STREAM: usize = 16 * 1024 * 1024;

/// The most modules a project may declare (SPEC M1).
pub const MAX_MODULES: usize = 2_000;

/// The most references a project may declare. SPEC M1 sets no limit, so this
/// one mirrors that of modules.
pub const MAX_REFERENCES: usize = 2_000;

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
