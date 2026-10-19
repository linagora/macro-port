// SPDX-License-Identifier: Apache-2.0
//! End-to-end tests of `macro-port rules check`.

use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn macro_port<S: AsRef<OsStr>>(args: impl IntoIterator<Item = S>) -> Output {
    Command::new(env!("CARGO_BIN_EXE_macro-port"))
        .args(args)
        .output()
        .expect("the macro-port binary runs")
}

/// A fresh rules directory holding the shipped rules, whose `families.yaml` is
/// replaced by `tests/fixtures/rules/families/<fixture>.yaml` when given.
fn rules_dir(name: &str, families_fixture: Option<&str>) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("rules")
        .join(name);
    if dir.exists() {
        fs::remove_dir_all(&dir).expect("old rules directory removed");
    }
    fs::create_dir_all(&dir).expect("rules directory created");
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    for file in ["families.yaml", "security.yaml", "complexity.yaml"] {
        let shipped = manifest_dir.join("../../rules").join(file);
        fs::copy(shipped, dir.join(file)).expect("shipped rules copied");
    }
    if let Some(fixture) = families_fixture {
        let fixture = manifest_dir
            .join("tests/fixtures/rules/families")
            .join(format!("{fixture}.yaml"));
        fs::copy(fixture, dir.join("families.yaml")).expect("fixture copied");
    }
    dir
}

/// Runs `rules check` on a rules directory.
fn check_dir(dir: &Path) -> Output {
    macro_port([
        OsStr::new("rules"),
        OsStr::new("check"),
        OsStr::new("--rules"),
        dir.as_os_str(),
    ])
}

/// Runs `rules check` on a rules directory whose `families.yaml` is a fixture.
fn check_families(fixture: &str) -> Output {
    check_dir(&rules_dir(fixture, Some(fixture)))
}

/// A rules directory whose `families.yaml` is the valid fixture padded with a
/// comment to `size` bytes.
fn padded_rules_dir(name: &str, size: usize) -> PathBuf {
    let dir = rules_dir(name, Some("valid-minimal"));
    let path = dir.join("families.yaml");
    let mut text = fs::read_to_string(&path).expect("fixture read");
    let padding = size
        .checked_sub(text.len() + 1)
        .expect("the fixture is smaller than the size");
    text.push_str(&"#".repeat(padding));
    text.push('\n');
    fs::write(&path, text).expect("padded fixture written");
    dir
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn embedded_rules_pass() {
    let output = macro_port(["rules", "check"]);

    assert!(output.status.success(), "{output:?}");
    assert_eq!(stdout(&output), "families.yaml: ok\n");
}

#[test]
fn missing_rules_directory_is_a_usage_error() {
    let output = check_dir(&Path::new(env!("CARGO_TARGET_TMPDIR")).join("no-such-rules-directory"));

    // SPEC M4: exit code 2 is a usage error.
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(
        stderr(&output).contains("not an existing directory"),
        "{output:?}"
    );
}

#[test]
fn valid_rules_directory_passes() {
    let output = check_families("valid-minimal");

    assert!(output.status.success(), "{output:?}");
    assert_eq!(stdout(&output), "families.yaml: ok\n");
}

#[test]
fn invalid_regexes_are_rejected() {
    let output = check_families("invalid-regex");

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(
        stderr(&output),
        "families.yaml: fmt.loop-cells: invalid regex '\\bFor\\s+(Each': unclosed group \
         (at /families/0/signals/2/pattern)\n\
         families.yaml: fmt.style-name: invalid regex '[Heading': unclosed character class \
         (at /families/0/signals/3/pattern/1)\n\
         2 problems found\n"
    );
}

#[test]
fn duplicate_family_id_is_rejected() {
    let output = check_families("duplicate-family-id");

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(
        stderr(&output),
        "families.yaml: 01-formatting: duplicate family id, first defined at /families/0 \
         (at /families/1/id)\n\
         1 problem found\n"
    );
}

#[test]
fn duplicate_signal_id_is_rejected() {
    let output = check_families("duplicate-signal-id");

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(
        stderr(&output),
        "families.yaml: fmt.cell-style: duplicate signal id, first defined at \
         /families/0/signals/0 (at /families/1/signals/0/id)\n\
         1 problem found\n"
    );
}

#[test]
fn invalid_yaml_is_rejected() {
    let output = check_families("invalid-yaml");

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(
        stderr(&output),
        "families.yaml: invalid YAML: mismatched bracket '[' closed by '}' at line 13, column 55\n\
         1 problem found\n"
    );
}

#[test]
fn duplicate_yaml_keys_are_rejected() {
    let output = check_families("duplicate-key");

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(
        stderr(&output),
        "families.yaml: invalid YAML: duplicate mapping key: weight at line 13, column 87\n\
         1 problem found\n"
    );
}

#[test]
fn merge_keys_are_rejected() {
    let output = check_families("merge-key");

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(
        stderr(&output),
        "families.yaml: invalid YAML: YAML merge keys are not allowed by configured policy \
         at line 14, column 11\n\
         1 problem found\n"
    );
}

#[test]
fn a_second_yaml_document_is_rejected() {
    let output = check_families("two-documents");

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(
        stderr(&output),
        "families.yaml: invalid YAML: multiple YAML documents detected at line 16, column 1\n\
         1 problem found\n"
    );
}

#[test]
fn empty_rules_files_are_rejected() {
    let comments_only = check_families("empty");
    let dir = rules_dir("zero-bytes", None);
    fs::write(dir.join("families.yaml"), b"").expect("families.yaml emptied");
    let zero_bytes = check_dir(&dir);

    for output in [comments_only, zero_bytes] {
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert_eq!(
            stderr(&output),
            "families.yaml: empty document\n1 problem found\n"
        );
    }
}

#[test]
fn unreadable_families_file_is_fatal() {
    let dir = rules_dir("missing-families", None);
    let path = dir.join("families.yaml");
    fs::remove_file(&path).expect("families.yaml removed");
    let output = check_dir(&dir);

    // SPEC M4: exit code 3 is a fatal error, after which nothing is checked.
    assert_eq!(output.status.code(), Some(3), "{output:?}");
    let stderr = stderr(&output);
    let start = format!("error: cannot read {}: ", path.display());
    assert!(stderr.starts_with(&start), "{stderr}");
    assert_eq!(stderr.lines().count(), 1, "{stderr}");
}

#[test]
fn non_utf8_rules_file_is_rejected() {
    let dir = rules_dir("not-utf8", None);
    fs::write(dir.join("families.yaml"), b"version: \xff\n").expect("families.yaml written");
    let output = check_dir(&dir);

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(
        stderr(&output),
        "families.yaml: not UTF-8: invalid utf-8 sequence of 1 bytes from index 9\n\
         1 problem found\n"
    );
}

#[test]
fn rules_file_of_one_mib_is_read() {
    let output = check_dir(&padded_rules_dir("one-mib", 1 << 20));

    assert!(output.status.success(), "{output:?}");
    assert_eq!(stdout(&output), "families.yaml: ok\n");
}

#[test]
fn rules_file_over_one_mib_is_rejected() {
    let output = check_dir(&padded_rules_dir("over-one-mib", (1 << 20) + 1));

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(
        stderr(&output),
        "families.yaml: larger than 1 MiB\n1 problem found\n"
    );
}
