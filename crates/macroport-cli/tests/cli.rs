// SPDX-License-Identifier: Apache-2.0
//! End-to-end tests: run the `macro-port` binary as a user would.

use std::fs;
use std::io;
use std::path::Path;
use std::process::{Command, Output};

fn macro_port(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_macro-port"))
        .args(args)
        .output()
        .expect("the macro-port binary runs")
}

#[test]
fn version_flag_prints_the_tool_version() {
    let output = macro_port(&["--version"]);

    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        format!("macro-port {}\n", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn unknown_option_is_a_usage_error() {
    let output = macro_port(&["--no-such-option"]);

    // SPEC M4: exit code 2 is a usage error.
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stderr).contains("--no-such-option"));
}

#[test]
fn schema_prints_the_schema_of_the_report() {
    let output = macro_port(&["schema"]);

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schema/report.schema.json");
    let schema = fs::read(&path).expect("the schema of the report is read");
    // The schema as the repository holds it, byte for byte: stdout is the file.
    assert_eq!(output.stdout, schema);
    assert!(output.stderr.is_empty(), "{output:?}");
    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout).expect("JSON");
    assert_eq!(parsed["title"], "Macro-Port report");
}

#[test]
fn schema_takes_no_argument() {
    let output = macro_port(&["schema", "report"]);

    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(output.stdout.is_empty(), "{output:?}");
}

#[test]
fn a_reader_of_the_schema_that_went_away_is_not_an_error() {
    // The reader of the pipe is closed before the command starts, as with
    // `macro-port schema | head -n 0`: every write on stdout fails.
    let (reader, writer) = io::pipe().expect("a pipe");
    drop(reader);
    let output = Command::new(env!("CARGO_BIN_EXE_macro-port"))
        .arg("schema")
        .stdout(writer)
        .output()
        .expect("the macro-port binary runs");

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
}
