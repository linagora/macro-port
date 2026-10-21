// SPDX-License-Identifier: Apache-2.0
//! End-to-end tests of `scan --rules`: the rules of a directory are checked as
//! `rules check` checks them, then used (SPEC M4).

mod support;

use std::fs;
use std::path::Path;
use std::process::Output;

use support::{
    MODULE, OTHER_VERSION, RULES_FILES, as_arg, put, read_report, run_in, scratch,
    shipped_rules_with_version, text, use_fixture, valid_rules,
};

/// Scans `m.bas` of `dir` into `dir/out` with the rules of `rules`, and gives
/// the output of `scan` followed by that of `rules check` on the same rules.
fn scan_and_check(dir: &Path, rules: &Path, lang: &str) -> (Output, Output) {
    let rules = as_arg(rules);
    let scan = run_in(
        dir,
        &[
            "scan", "m.bas", "-o", "out", "--rules", rules, "--lang", lang,
        ],
    );
    let check = run_in(dir, &["rules", "check", "--rules", rules]);
    (scan, check)
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

    let (scan, check) = scan_and_check(&dir, &rules, "en");

    // SPEC M4: any problem in the rules stops the scan with exit code 3, and
    // the problems are those of `rules check`, which exits with 1.
    assert_eq!(scan.status.code(), Some(3), "{scan:?}");
    assert_eq!(check.status.code(), Some(1), "{check:?}");
    assert_eq!(text(&scan.stderr), text(&check.stderr));
    assert!(text(&scan.stderr).starts_with("families.yaml: unknown key 'bogus'"));
    assert!(text(&scan.stdout).is_empty());
    assert!(!dir.join("out").exists(), "nothing is scanned or written");
}

#[test]
fn scan_stops_with_3_on_rules_that_do_not_pass_the_core() {
    let dir = scratch("scan_rules_core");
    put(&dir, "m.bas", MODULE);
    let rules = valid_rules(&dir);
    use_fixture(&rules, "families", "invalid-regex");

    let (scan, check) = scan_and_check(&dir, &rules, "en");

    assert_eq!(scan.status.code(), Some(3), "{scan:?}");
    assert_eq!(check.status.code(), Some(1), "{check:?}");
    assert_eq!(text(&scan.stderr), text(&check.stderr));
    assert!(text(&scan.stderr).ends_with("2 problems found\n"));
    assert!(!dir.join("out").exists(), "nothing is scanned or written");
}

#[test]
fn scan_stops_with_3_when_a_rules_file_cannot_be_read() {
    for name in RULES_FILES {
        let dir = scratch(&format!("scan_rules_missing_{}", name.replace('.', "_")));
        put(&dir, "m.bas", MODULE);
        let rules = shipped_rules_with_version(&dir, OTHER_VERSION);
        fs::remove_file(rules.join(name)).expect("a rules file is removed");

        let (scan, check) = scan_and_check(&dir, &rules, "en");

        // Nothing can be checked: both commands exit with 3.
        assert_eq!(scan.status.code(), Some(3), "{name}: {scan:?}");
        assert_eq!(check.status.code(), Some(3), "{name}: {check:?}");
        assert_eq!(text(&scan.stderr), text(&check.stderr), "{name}");
        let start = format!("error: cannot read {}: ", rules.join(name).display());
        assert!(text(&scan.stderr).starts_with(&start), "{name}");
        assert!(!dir.join("out").exists(), "{name}");
    }
}

#[test]
fn scan_says_what_is_wrong_with_the_rules_in_the_language_of_the_command() {
    let dir = scratch("scan_rules_french");
    put(&dir, "m.bas", MODULE);

    // Problems found before the core.
    let rules = valid_rules(&dir.join("schema"));
    fs::write(rules.join("security.yaml"), b"").expect("security.yaml is emptied");
    let (scan, _) = scan_and_check(&dir, &rules, "fr");
    assert_eq!(scan.status.code(), Some(3), "{scan:?}");
    assert_eq!(
        text(&scan.stderr),
        "security.yaml: empty document\n\
         1 problème trouvé (les expressions régulières, les identifiants et les références \
         ne sont pas encore vérifiés)\n"
    );

    // Problems found by the core.
    let rules = valid_rules(&dir.join("core"));
    use_fixture(&rules, "families", "invalid-regex");
    let (scan, _) = scan_and_check(&dir, &rules, "fr");
    assert_eq!(scan.status.code(), Some(3), "{scan:?}");
    assert!(text(&scan.stderr).ends_with("2 problèmes trouvés\n"));

    // A file that cannot be read.
    let rules = valid_rules(&dir.join("unreadable"));
    fs::remove_file(rules.join("complexity.yaml")).expect("complexity.yaml is removed");
    let (scan, _) = scan_and_check(&dir, &rules, "fr");
    assert_eq!(scan.status.code(), Some(3), "{scan:?}");
    let start = format!(
        "erreur : impossible de lire {} : ",
        rules.join("complexity.yaml").display()
    );
    assert!(text(&scan.stderr).starts_with(&start), "{scan:?}");

    assert!(!dir.join("out").exists(), "nothing is scanned or written");
}

#[test]
fn scan_exits_with_2_when_the_rules_are_not_a_directory() {
    let dir = scratch("scan_rules_absent");
    put(&dir, "m.bas", MODULE);
    put(&dir, "rules.yaml", b"version: 0.1.0\n");

    // A usage error, as for `rules check --rules`: a directory that does not
    // exist, and a file.
    for rules in ["no-such-rules", "rules.yaml"] {
        let output = run_in(&dir, &["scan", "m.bas", "--rules", rules]);

        assert_eq!(output.status.code(), Some(2), "{rules}: {output:?}");
        assert!(
            text(&output.stderr).contains("not an existing directory"),
            "{rules}: {output:?}"
        );
        assert!(!dir.join("macro-port-report").exists(), "{rules}");
    }
}
