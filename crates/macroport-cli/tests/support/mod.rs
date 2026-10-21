// SPDX-License-Identifier: Apache-2.0
//! Helpers for the tests of the commands that read and write files: a scratch
//! directory for each test, the files to put in it, the files that a test
//! makes unreadable, and the binary to run.

// Each test binary includes this module and uses some of its helpers.
#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// The root of the repository.
pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Runs `macro-port` in `dir` with `args`.
pub fn run_in(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_macro-port"))
        .current_dir(dir)
        .args(args)
        .output()
        .expect("the macro-port binary runs")
}

/// Runs `macro-port` at the root of the repository, so that the paths in its
/// reports are the relative paths `corpus/...`.
pub fn run(args: &[&str]) -> Output {
    run_in(&repo_root(), args)
}

/// The text of the bytes that a command printed.
pub fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// A path of the target directory as an argument.
pub fn as_arg(path: &Path) -> &str {
    path.to_str()
        .expect("a path of the target directory is UTF-8")
}

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

/// The rules files, by kind.
pub const RULES_FILES: [&str; 3] = ["families.yaml", "security.yaml", "complexity.yaml"];

/// A copy of the shipped rules in `dir/rules`, with `version` in place of the
/// version that they share. Gives the directory.
pub fn shipped_rules_with_version(dir: &Path, version: &str) -> PathBuf {
    let rules = dir.join("rules");
    fs::create_dir_all(&rules).expect("the rules directory is created");
    for name in RULES_FILES {
        let shipped = fs::read_to_string(repo_root().join("rules").join(name))
            .expect("a shipped rules file is read");
        assert!(
            shipped.lines().any(|line| line.starts_with("version: ")),
            "{name} declares its version"
        );
        let mut copy: String = shipped
            .lines()
            .map(|line| {
                if line.starts_with("version: ") {
                    format!("version: {version}")
                } else {
                    line.to_owned()
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        copy.push('\n');
        fs::write(rules.join(name), copy).expect("a rules file is written");
    }
    rules
}

/// The minimal valid rules of `tests/fixtures/rules/` in `dir/rules`, but for
/// `families.yaml`, which is the fixture `families` of
/// `tests/fixtures/rules/families/`. Gives the directory.
pub fn fixture_rules(dir: &Path, families: &str) -> PathBuf {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/rules");
    let rules = dir.join("rules");
    fs::create_dir_all(&rules).expect("the rules directory is created");
    for kind in ["families", "security", "complexity"] {
        let fixture = if kind == "families" {
            families
        } else {
            "valid-minimal"
        };
        let from = fixtures.join(kind).join(format!("{fixture}.yaml"));
        fs::copy(from, rules.join(format!("{kind}.yaml"))).expect("a fixture is copied");
    }
    rules
}
