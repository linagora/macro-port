// SPDX-License-Identifier: Apache-2.0
//! Helpers for the tests of the commands that read and write files: a scratch
//! directory for each test, the files to put in it, and the files that a test
//! makes unreadable.

use std::fs;
use std::path::{Path, PathBuf};

/// An empty directory for the test `name`, under the target directory of
/// cargo, so that no test writes outside the build tree. Each test names its
/// own, since the tests of a file run side by side.
pub fn scratch(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    if dir.exists() {
        fs::remove_dir_all(&dir).expect("the scratch directory of an earlier run is removed");
    }
    fs::create_dir_all(&dir).expect("the scratch directory is created");
    dir
}

/// Writes the file `relative` under `dir`, with the directories above it.
pub fn put(dir: &Path, relative: &str, content: &[u8]) -> PathBuf {
    let path = dir.join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("the directories of the file are created");
    }
    fs::write(&path, content).expect("the file is written");
    path
}

/// Makes the file or directory `path` unreadable, and tells whether it is: a
/// process that runs as root reads anything, and then there is nothing to test.
#[cfg(unix)]
pub fn lock(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o000)).expect("permissions are set");
    let locked = fs::File::open(path).is_err() && fs::read_dir(path).is_err();
    if !locked {
        unlock(path);
    }
    locked
}

/// Gives `path` back its permissions. A test calls it before it checks
/// anything, so that a failed check does not leave a path that the next run
/// cannot remove.
#[cfg(unix)]
pub fn unlock(path: &Path) {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).expect("permissions are set");
}
