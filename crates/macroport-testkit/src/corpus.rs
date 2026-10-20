// SPDX-License-Identifier: Apache-2.0
//! The files of `corpus/`, for the tests of every crate. The core's tests may
//! not touch the file system themselves (its `clippy.toml` bans it), so they
//! read the corpus through these helpers.

use std::fs;
use std::path::{Path, PathBuf};

/// The `corpus/` directory at the root of the repository.
pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus")
}

/// The bytes of a corpus file, by its path relative to `corpus/` with `/`
/// separators.
///
/// # Panics
///
/// When the file cannot be read: a test should fail loudly then.
pub fn read(relative: &str) -> Vec<u8> {
    let path = root().join(relative);
    fs::read(&path).unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
}

/// The files under a corpus directory, recursively: their paths relative to
/// `corpus/` with `/` separators, sorted. Only the files whose lowercase
/// extension is in `extensions` are kept, or every file when `extensions` is
/// empty.
///
/// # Panics
///
/// When a directory cannot be read.
pub fn files(relative_dir: &str, extensions: &[&str]) -> Vec<String> {
    let mut found = Vec::new();
    collect(&root(), &root().join(relative_dir), extensions, &mut found);
    found.sort();
    found
}

fn collect(root: &Path, dir: &Path, extensions: &[&str], found: &mut Vec<String>) {
    let entries =
        fs::read_dir(dir).unwrap_or_else(|error| panic!("cannot read {}: {error}", dir.display()));
    for entry in entries {
        let path = entry.expect("a directory entry").path();
        if path.is_dir() {
            collect(root, &path, extensions, found);
            continue;
        }
        let extension = path
            .extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase);
        let wanted = extensions.is_empty()
            || extension.is_some_and(|extension| extensions.contains(&extension.as_str()));
        if wanted {
            let relative = path.strip_prefix(root).expect("a path under the corpus");
            let parts: Vec<_> = relative.iter().map(|part| part.to_string_lossy()).collect();
            found.push(parts.join("/"));
        }
    }
}
