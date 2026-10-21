// SPDX-License-Identifier: Apache-2.0
//! End-to-end tests of `scan --rules`: the rules of a directory are checked as
//! `rules check` checks them, then used (SPEC M4).

mod support;

use std::fs;
use std::path::Path;

use macroport_core::report::Report;
use macroport_testkit::report_problems;
use support::{
    RULES_FILES, as_arg, fixture_rules, put, run_in, scratch, shipped_rules_with_version, text,
};

/// A module with a procedure, small enough to be analysed at once.
const MODULE: &[u8] = b"Sub Hello()\n    Range(\"A1\").Value = 1\nEnd Sub\n";

/// The version of rules that no release ships.
const OTHER_VERSION: &str = "9.9.9";

/// The `report.json` in `out`, once it has passed the schema.
fn read_report(out: &Path) -> Report {
    let json = fs::read_to_string(out.join("report.json")).expect("report.json is written");
    assert_eq!(report_problems(&json), Vec::<String>::new());
    serde_json::from_str(&json).expect("report.json is a report")
}

#[test]
fn scan_uses_the_rules_of_the_directory() {
    let dir = scratch("scan_rules_used");
    put(&dir, "m.bas", MODULE);
    let rules = shipped_rules_with_version(&dir, OTHER_VERSION);

    let own = run_in(
        &dir,
        &["scan", "m.bas", "-o", "own", "--rules", as_arg(&rules)],
    );
    let embedded = run_in(&dir, &["scan", "m.bas", "-o", "embedded"]);

    assert_eq!(own.status.code(), Some(0), "{own:?}");
    assert_eq!(embedded.status.code(), Some(0), "{embedded:?}");
    let own = read_report(&dir.join("own"));
    let embedded = read_report(&dir.join("embedded"));
    assert_eq!(own.tool.rules_version, OTHER_VERSION);
    assert_ne!(embedded.tool.rules_version, OTHER_VERSION);
}

#[test]
fn scan_stops_with_3_on_rules_that_do_not_pass_the_schemas() {
    let dir = scratch("scan_rules_schema");
    put(&dir, "m.bas", MODULE);
    let rules = shipped_rules_with_version(&dir, OTHER_VERSION);
    let families = rules.join("families.yaml");
    let mut text_of_families = fs::read_to_string(&families).expect("families.yaml is read");
    text_of_families.push_str("bogus: 1\n");
    fs::write(&families, text_of_families).expect("families.yaml is written");

    let output = run_in(
        &dir,
        &["scan", "m.bas", "-o", "out", "--rules", as_arg(&rules)],
    );

    // SPEC M4: any problem in the rules stops the scan with exit code 3, and
    // the problems are those of `rules check`.
    assert_eq!(output.status.code(), Some(3), "{output:?}");
    assert_eq!(
        text(&output.stderr),
        "families.yaml: unknown key 'bogus' (at /bogus)\n\
         1 problem found (regexes, ids and references are not checked yet)\n"
    );
    assert!(text(&output.stdout).is_empty());
    assert!(!dir.join("out").exists(), "nothing is scanned or written");
}

#[test]
fn scan_stops_with_3_on_rules_that_do_not_pass_the_core() {
    let dir = scratch("scan_rules_core");
    put(&dir, "m.bas", MODULE);
    let rules = fixture_rules(&dir, "invalid-regex");

    let output = run_in(
        &dir,
        &["scan", "m.bas", "-o", "out", "--rules", as_arg(&rules)],
    );

    assert_eq!(output.status.code(), Some(3), "{output:?}");
    assert_eq!(
        text(&output.stderr),
        "families.yaml: fmt.loop-cells: invalid regex '\\bFor\\s+(Each': unclosed group \
         (at /families/0/signals/2/pattern)\n\
         families.yaml: fmt.style-name: invalid regex '[Heading': unclosed character class \
         (at /families/0/signals/3/pattern/1)\n\
         2 problems found\n"
    );
    assert!(!dir.join("out").exists(), "nothing is scanned or written");
}

#[test]
fn scan_stops_with_3_when_a_rules_file_cannot_be_read() {
    for name in RULES_FILES {
        let dir = scratch(&format!("scan_rules_missing_{}", name.replace('.', "_")));
        put(&dir, "m.bas", MODULE);
        let rules = shipped_rules_with_version(&dir, OTHER_VERSION);
        fs::remove_file(rules.join(name)).expect("a rules file is removed");

        let output = run_in(
            &dir,
            &["scan", "m.bas", "-o", "out", "--rules", as_arg(&rules)],
        );

        assert_eq!(output.status.code(), Some(3), "{name}: {output:?}");
        let stderr = text(&output.stderr);
        let start = format!("error: cannot read {}: ", rules.join(name).display());
        assert!(stderr.starts_with(&start), "{name}: {stderr}");
        assert!(!dir.join("out").exists(), "{name}");
    }
}

#[test]
fn scan_exits_with_2_when_the_rules_directory_does_not_exist() {
    let dir = scratch("scan_rules_absent");
    put(&dir, "m.bas", MODULE);

    let output = run_in(&dir, &["scan", "m.bas", "--rules", "no-such-rules"]);

    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(text(&output.stderr).contains("not an existing directory"));
    assert!(!dir.join("macro-port-report").exists());
}
