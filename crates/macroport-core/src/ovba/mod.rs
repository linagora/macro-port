// SPDX-License-Identifier: Apache-2.0
//! MS-OVBA, the format of a VBA project inside a CFB container ([MS-OVBA],
//! Office VBA File Format Structure).
//!
//! Pure functions over byte slices: the caller reads the streams out of the
//! container and hands over their bytes. Every input is hostile, so each
//! function bounds what it reads and returns an [`OvbaError`] rather than
//! panicking.

mod decompress;

use crate::report::Status;

pub use decompress::decompress;

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
