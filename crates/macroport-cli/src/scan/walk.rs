// SPDX-License-Identifier: Apache-2.0
//! The walk of a scan: the files under the paths it is given that the
//! inventory covers (SPEC M4).

use std::collections::BTreeMap;
use std::collections::btree_map::Entry;
use std::fs;
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
    /// Why the walk could not read it: for a directory it could not list, and
    /// for a root that does not exist, that is a link to nothing or that is
    /// neither a file nor a directory.
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
/// follow the symbolic links it meets in a directory: a link is neither read
/// nor entered. A root that is a link is followed, since it was asked for by
/// name.
///
/// A file that is reached twice, by two roots or through a link in the path of
/// a root, is taken once, under the shorter of its paths, the smaller of two
/// that are as long: the result does not depend on the order of the roots.
///
/// What the walk cannot look at comes out as a candidate that carries the
/// reason, so that the inventory shows what it could not cover: a directory
/// that cannot be listed, a root that does not exist or that is neither a file
/// nor a directory.
pub fn walk(roots: &[PathBuf]) -> Vec<Candidate> {
    let mut found: BTreeMap<PathBuf, Candidate> = BTreeMap::new();
    for root in roots {
        // `corpus/` and `corpus` are one root: the report names no double separator.
        let root: PathBuf = root.components().collect();
        for candidate in candidates_of(&root) {
            keep(&mut found, candidate);
        }
    }
    // The map is in the order of the real paths, the report in that of the
    // paths that it gives.
    let mut candidates: Vec<Candidate> = found.into_values().collect();
    candidates.sort_by(|a, b| a.path.cmp(&b.path));
    candidates
}

/// The candidates of one root.
fn candidates_of(root: &Path) -> Vec<Candidate> {
    // `metadata` follows a link: a root that is one is what it points to.
    match fs::metadata(root) {
        Ok(metadata) if metadata.is_file() => vec![Candidate::file(root)],
        Ok(metadata) if metadata.is_dir() => candidates_under(root),
        Ok(_) => vec![Candidate::unreadable(
            root,
            "neither a file nor a directory".to_owned(),
        )],
        Err(error) => vec![Candidate::unreadable(root, error.to_string())],
    }
}

/// The candidates under the directory `root`.
fn candidates_under(root: &Path) -> Vec<Candidate> {
    WalkDir::new(root)
        .into_iter()
        .filter_map(|entry| match entry {
            Ok(entry) if entry.file_type().is_file() && is_covered(entry.path()) => {
                Some(Candidate::file(entry.path()))
            }
            Ok(_) => None,
            Err(error) => {
                let location = error.path().unwrap_or(root);
                let reason = error
                    .io_error()
                    .map_or_else(|| error.to_string(), ToString::to_string);
                Some(Candidate::unreadable(location, reason))
            }
        })
        .collect()
}

/// Adds `candidate`, unless its file is there already, reached by another path:
/// of two paths to a file, the shorter stays, the smaller if they are as long.
fn keep(found: &mut BTreeMap<PathBuf, Candidate>, candidate: Candidate) {
    // What cannot be resolved, a root that is missing, stands for itself.
    let identity =
        fs::canonicalize(&candidate.location).unwrap_or_else(|_| candidate.location.clone());
    match found.entry(identity) {
        Entry::Vacant(slot) => {
            slot.insert(candidate);
        }
        Entry::Occupied(mut slot) => {
            let kept = &slot.get().path;
            if (candidate.path.len(), &candidate.path) < (kept.len(), kept) {
                slot.insert(candidate);
            }
        }
    }
}

/// Whether the inventory covers a file found under a directory. Its extension
/// is what follows the last dot of its name, as the core takes it: a file named
/// `.xls` has one.
fn is_covered(path: &Path) -> bool {
    let Some(name) = path.file_name() else {
        return false;
    };
    name.to_string_lossy()
        .rsplit_once('.')
        .is_some_and(|(_, extension)| {
            EXTENSIONS
                .iter()
                .any(|covered| covered.eq_ignore_ascii_case(extension))
        })
}
