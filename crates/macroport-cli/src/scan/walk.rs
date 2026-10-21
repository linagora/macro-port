// SPDX-License-Identifier: Apache-2.0
//! The walk of a scan: the files under the paths it is given that the
//! inventory covers (SPEC M4).

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::path::{MAIN_SEPARATOR, Path, PathBuf};

use walkdir::WalkDir;

/// The extensions of the files that a walk collects, in lowercase. The Excel
/// formats come first, then the VBA modules exported as text.
pub const EXTENSIONS: [&str; 12] = [
    "xlsm", "xltm", "xlam", "xlsb", "xls", "xlt", "xla", "xlsx", "xltx", "bas", "cls", "frm",
];

/// A file to analyse, or something that the walk could not look at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// The path that the report gives, with `/` between the names.
    pub path: String,
    /// Where the file is.
    pub location: PathBuf,
    /// Why the walk could not read it, for a directory it could not list.
    pub unreadable: Option<String>,
}

impl Candidate {
    fn file(location: &Path) -> Self {
        Self {
            path: report_path(location),
            location: location.to_owned(),
            unreadable: None,
        }
    }

    fn unreadable(location: &Path, reason: String) -> Self {
        Self {
            unreadable: Some(reason),
            ..Self::file(location)
        }
    }
}

/// The path as the report gives it: `/` between the names on every system.
pub fn report_path(path: &Path) -> String {
    let text = path.to_string_lossy();
    if MAIN_SEPARATOR == '/' {
        text.into_owned()
    } else {
        text.replace(MAIN_SEPARATOR, "/")
    }
}

/// The files to analyse under `roots`, in the order of their paths.
///
/// A root that is a file is taken whatever its extension: it was asked for by
/// name. Under a root that is a directory, the walk collects the regular files
/// whose extension is one of [`EXTENSIONS`], whatever its case. It does not
/// follow symbolic links: a link is neither read nor entered, but a root that
/// is a link is followed. A file reached twice is taken once.
///
/// A directory that cannot be listed comes out as a candidate that carries the
/// reason, so that the inventory shows what it could not cover.
pub fn walk(roots: &[PathBuf]) -> Vec<Candidate> {
    let mut found: BTreeMap<PathBuf, Candidate> = BTreeMap::new();
    for root in roots {
        // `corpus/` and `corpus` are one root: the report names no double separator.
        let root: PathBuf = root.components().collect();
        for entry in WalkDir::new(&root) {
            let candidate = match entry {
                Ok(entry) if !entry.file_type().is_file() => continue,
                Ok(entry) if entry.depth() > 0 && !is_covered(entry.path()) => continue,
                Ok(entry) => Candidate::file(entry.path()),
                Err(error) => {
                    let location = error.path().unwrap_or(&root);
                    let reason = error
                        .io_error()
                        .map_or_else(|| error.to_string(), ToString::to_string);
                    Candidate::unreadable(location, reason)
                }
            };
            found.entry(candidate.location.clone()).or_insert(candidate);
        }
    }
    let mut candidates: Vec<Candidate> = found.into_values().collect();
    candidates.sort_by(|a, b| a.path.cmp(&b.path));
    candidates
}

/// Whether the inventory covers a file found under a directory.
fn is_covered(path: &Path) -> bool {
    path.extension()
        .and_then(OsStr::to_str)
        .is_some_and(|extension| {
            EXTENSIONS
                .iter()
                .any(|covered| covered.eq_ignore_ascii_case(extension))
        })
}
