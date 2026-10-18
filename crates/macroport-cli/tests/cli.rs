// SPDX-License-Identifier: Apache-2.0
//! End-to-end tests: run the `macro-port` binary as a user would.

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
