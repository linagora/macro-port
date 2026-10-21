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

/// A file or directory that a test made unreadable. It gets its permissions
/// back when the guard goes out of scope, a failed check included, so that the
/// next run can remove it.
#[cfg(unix)]
#[derive(Debug)]
pub struct Denied {
    path: PathBuf,
    permissions: fs::Permissions,
}

#[cfg(unix)]
impl Drop for Denied {
    fn drop(&mut self) {
        // Nothing to report from a drop: the next run fails to remove the path.
        let _ = fs::set_permissions(&self.path, self.permissions.clone());
    }
}

/// Makes the file or directory `path` unreadable, for as long as the guard
/// lives. `None` when it can be read all the same: a process that runs as root
/// reads anything, and then there is nothing to test.
#[cfg(unix)]
pub fn deny_access(path: &Path) -> Option<Denied> {
    use std::os::unix::fs::PermissionsExt;

    let permissions = fs::metadata(path)
        .expect("the path to deny exists")
        .permissions();
    fs::set_permissions(path, fs::Permissions::from_mode(0o000)).expect("permissions are set");
    let guard = Denied {
        path: path.to_owned(),
        permissions,
    };
    let denied = fs::File::open(path).is_err() && fs::read_dir(path).is_err();
    // A path that is not denied is given back by dropping the guard.
    denied.then_some(guard)
}
