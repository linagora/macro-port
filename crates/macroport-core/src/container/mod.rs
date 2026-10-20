// SPDX-License-Identifier: Apache-2.0
//! Container layer: finds the VBA project and the XLM macro sheets in the
//! bytes of a file, within the limits of SPEC M1. A file it cannot read gets a
//! [`ContainerError`], which carries the status of that file.

use std::fmt;

use crate::report::Status;

mod ooxml;

pub use ooxml::{OoxmlPackage, read_ooxml};

/// Why a container could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerError {
    /// The status the file gets in the report.
    pub status: Status,
    /// Stable code, lowercase and dotted as `<layer>.<problem>`, such as
    /// `zip.ratio` or `cfb.too_many_entries`.
    pub code: &'static str,
    /// What went wrong.
    pub message: String,
}

impl ContainerError {
    /// An error giving the file `status`.
    pub fn new(status: Status, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
        }
    }
}

impl fmt::Display for ContainerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
