// SPDX-License-Identifier: Apache-2.0
//! End-to-end tests of `macro-port rules check`.

mod support;

use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use support::{RULES_FILES, scratch, use_fixture, valid_rules};

/// Ends the count of the problems found before the core loads the rules.
const FILE_STAGE: &str = " (regexes, ids and references are not checked yet)";

/// What `rules check` prints when the rules are valid.
const ALL_OK: &str = "families.yaml: ok\nsecurity.yaml: ok\ncomplexity.yaml: ok\n";

/// The metrics of the report, which are those that `complexity.yaml` can name.
const METRICS: &str = "loc, logical_loc, procedures, cyclomatic, cyclomatic_total, max_nesting, \
                       parameters, goto_count, forms, declare_count, external_dependencies";

fn macro_port<S: AsRef<OsStr>>(args: impl IntoIterator<Item = S>) -> Output {
    Command::new(env!("CARGO_BIN_EXE_macro-port"))
        .args(args)
        .output()
        .expect("the macro-port binary runs")
}

/// A fresh rules directory holding the valid fixture of every rules file:
/// `tests/fixtures/rules/<kind>/valid-minimal.yaml` as `<kind>.yaml`.
fn rules_dir(name: &str) -> PathBuf {
    valid_rules(&scratch(&format!("rules_check_{name}")))
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

/// Runs `rules check` on valid rules but for `families.yaml`, a fixture.
fn check_families(fixture: &str) -> Output {
    let dir = rules_dir(&format!("families-{fixture}"));
    use_fixture(&dir, "families", fixture);
    check_dir(&dir)
}

/// Runs `rules check` on valid rules but for `security.yaml`, a fixture.
fn check_security(fixture: &str) -> Output {
    let dir = rules_dir(&format!("security-{fixture}"));
    use_fixture(&dir, "security", fixture);
    check_dir(&dir)
}

/// Runs `rules check` on valid rules but for `complexity.yaml`, a fixture.
fn check_complexity(fixture: &str) -> Output {
    let dir = rules_dir(&format!("complexity-{fixture}"));
    use_fixture(&dir, "complexity", fixture);
    check_dir(&dir)
}

/// A rules directory whose `file` is the valid fixture padded with a comment
/// to `size` bytes.
fn padded_rules_dir(name: &str, file: &str, size: usize) -> PathBuf {
    let dir = rules_dir(name);
    let path = dir.join(file);
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
    assert_eq!(stdout(&output), ALL_OK);
}

#[test]
fn shipped_rules_directory_passes() {
    let output = check_dir(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../rules"));

    assert!(output.status.success(), "{output:?}");
    assert_eq!(stdout(&output), ALL_OK);
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
    let output = check_dir(&rules_dir("valid-minimal"));

    assert!(output.status.success(), "{output:?}");
    assert_eq!(stdout(&output), ALL_OK);
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
        format!(
            "families.yaml: invalid YAML: mismatched bracket '[' closed by '}}' \
             at line 13, column 55\n\
             1 problem found{FILE_STAGE}\n"
        )
    );
}

#[test]
fn duplicate_yaml_keys_are_rejected() {
    let output = check_families("duplicate-key");

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(
        stderr(&output),
        format!(
            "families.yaml: invalid YAML: duplicate mapping key: weight at line 13, column 87\n\
             1 problem found{FILE_STAGE}\n"
        )
    );
}

#[test]
fn merge_keys_are_rejected() {
    let output = check_families("merge-key");

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(
        stderr(&output),
        format!(
            "families.yaml: invalid YAML: YAML merge keys are not allowed by configured policy \
             at line 14, column 11\n\
             1 problem found{FILE_STAGE}\n"
        )
    );
}

#[test]
fn a_second_yaml_document_is_rejected() {
    let output = check_families("two-documents");

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(
        stderr(&output),
        format!(
            "families.yaml: invalid YAML: multiple YAML documents detected at line 16, column 1\n\
             1 problem found{FILE_STAGE}\n"
        )
    );
}

#[test]
fn empty_rules_files_are_rejected() {
    let comments_only = check_families("empty");
    let dir = rules_dir("zero-bytes");
    fs::write(dir.join("families.yaml"), b"").expect("families.yaml emptied");
    let zero_bytes = check_dir(&dir);

    for output in [comments_only, zero_bytes] {
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert_eq!(
            stderr(&output),
            format!("families.yaml: empty document\n1 problem found{FILE_STAGE}\n")
        );
    }
}

#[test]
fn unreadable_rules_files_are_fatal() {
    for file in RULES_FILES {
        let dir = rules_dir(&format!("missing-{file}"));
        let path = dir.join(file);
        fs::remove_file(&path).expect("rules file removed");
        let output = check_dir(&dir);

        // SPEC M4: exit code 3 is a fatal error, after which nothing is checked.
        assert_eq!(output.status.code(), Some(3), "{output:?}");
        let stderr = stderr(&output);
        let start = format!("error: cannot read {}: ", path.display());
        assert!(stderr.starts_with(&start), "{stderr}");
        assert_eq!(stderr.lines().count(), 1, "{stderr}");
    }
}

#[test]
fn non_utf8_rules_files_are_rejected() {
    for file in RULES_FILES {
        let dir = rules_dir(&format!("not-utf8-{file}"));
        fs::write(dir.join(file), b"version: \xff\n").expect("rules file written");
        let output = check_dir(&dir);

        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert_eq!(
            stderr(&output),
            format!(
                "{file}: not UTF-8: invalid utf-8 sequence of 1 bytes from index 9\n\
                 1 problem found{FILE_STAGE}\n"
            )
        );
    }
}

#[test]
fn rules_files_of_one_mib_are_read() {
    for file in RULES_FILES {
        let output = check_dir(&padded_rules_dir(&format!("one-mib-{file}"), file, 1 << 20));

        assert!(output.status.success(), "{output:?}");
        assert_eq!(stdout(&output), ALL_OK);
    }
}

#[test]
fn rules_files_over_one_mib_are_rejected() {
    for file in RULES_FILES {
        let dir = padded_rules_dir(&format!("over-one-mib-{file}"), file, (1 << 20) + 1);
        let output = check_dir(&dir);

        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert_eq!(
            stderr(&output),
            format!("{file}: larger than 1 MiB\n1 problem found{FILE_STAGE}\n")
        );
    }
}

#[test]
fn unknown_keys_are_rejected_at_every_level() {
    let output = check_families("unknown-keys");

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(
        stderr(&output),
        format!(
            "families.yaml: unknown key 'author' (at /author)\n\
             families.yaml: 01-formatting: unknown key 'colour' (at /families/0/colour)\n\
             families.yaml: 01-formatting: unknown key 'de' (at /families/0/label/de)\n\
             families.yaml: fmt.cell-style: unknown key 'severity' (at /families/0/signals/0/severity)\n\
             families.yaml: unknown key 'max_score' (at /scoring/max_score)\n\
             5 problems found{FILE_STAGE}\n"
        )
    );
}

#[test]
fn unknown_matcher_is_rejected() {
    let output = check_families("unknown-matcher");

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(
        stderr(&output),
        format!(
            "families.yaml: fmt.cell-style: 'membre' is not one of: member, chain, call, progid, \
             procedure_name, module_type, reference, regex, string, comment, structural \
             (at /families/0/signals/0/match)\n\
             1 problem found{FILE_STAGE}\n"
        )
    );
}

#[test]
fn structural_predicate_outside_the_closed_list_is_rejected() {
    let output = check_families("unknown-structural");

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(
        stderr(&output),
        format!(
            "families.yaml: fmt.magic: 'magic_predicate' is not one of: udf_candidate, \
             event_handler, auto_exec, chr_obfuscation, long_hex_or_base64_literal, \
             nested_loop_compare, possibly_unused, parse_recovered \
             (at /families/0/signals/2/pattern)\n\
             1 problem found{FILE_STAGE}\n"
        )
    );
}

#[test]
fn unknown_module_type_is_rejected() {
    let output = check_families("unknown-module-type");

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(
        stderr(&output),
        format!(
            "families.yaml: fmt.sheet-module: 'sheet' is not one of: standard, class, document, \
             form (at /families/0/signals/2/pattern/1)\n\
             1 problem found{FILE_STAGE}\n"
        )
    );
}

#[test]
fn labels_need_english_and_french() {
    let output = check_families("missing-label");

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(
        stderr(&output),
        format!(
            "families.yaml: 01-formatting: missing key 'fr' (at /families/0/label)\n\
             families.yaml: fmt.cell-style: missing key 'en' (at /families/0/signals/0/label)\n\
             2 problems found{FILE_STAGE}\n"
        )
    );
}

#[test]
fn family_ids_follow_the_report_format() {
    let output = check_families("family-id-format");

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(
        stderr(&output),
        format!(
            "families.yaml: Formatting: \"Formatting\" does not match \"^[0-9]{{2}}-[a-z0-9-]+$\" \
             (at /families/0/id)\n\
             1 problem found{FILE_STAGE}\n"
        )
    );
}

#[test]
fn weights_fit_32_bits() {
    let output = check_families("weight-out-of-range");

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(
        stderr(&output),
        format!(
            "families.yaml: fmt.cell-style: 4294967296 is greater than the maximum of 4294967295 \
             (at /families/0/signals/0/weight)\n\
             1 problem found{FILE_STAGE}\n"
        )
    );
}

#[test]
fn invalid_security_regexes_are_rejected() {
    let output = check_security("invalid-regex");

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(
        stderr(&output),
        "security.yaml: sec.declare: invalid regex '^\\s*(Declare\\s+': unclosed group \
         (at /signals/2/pattern)\n\
         1 problem found\n"
    );
}

#[test]
fn security_ids_are_unique_across_signals_chains_and_container_indicators() {
    let output = check_security("duplicate-id");

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(
        stderr(&output),
        "security.yaml: sec.declare: duplicate id, first defined at /signals/2 (at /signals/4/id)\n\
         security.yaml: sec.shell: duplicate id, first defined at /signals/1 (at /chains/1/id)\n\
         security.yaml: sec.chain.autoexec-execution: duplicate id, first defined at /chains/0 \
         (at /container_indicators/1/id)\n\
         security.yaml: sec.very-hidden: duplicate id, first defined at /signals/3 \
         (at /container_indicators/2/id)\n\
         4 problems found\n"
    );
}

#[test]
fn chain_terms_must_name_signals() {
    let output = check_security("unknown-chain-term");

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(
        stderr(&output),
        "security.yaml: sec.chain.autoexec-execution: unknown signal 'sec.autoexec' \
         (at /chains/0/all/0)\n\
         security.yaml: sec.chain.autoexec-execution: unknown signal 'sec.powershell' \
         (at /chains/0/all/1/any/1)\n\
         2 problems found\n"
    );
}

#[test]
fn required_families_must_exist() {
    let output = check_security("unknown-family");

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(
        stderr(&output),
        "security.yaml: sec.very-hidden: unknown family '01-mise-en-forme' \
         (at /signals/3/requires_family)\n\
         1 problem found\n"
    );
}

#[test]
fn rules_files_must_share_their_version() {
    for (file, output) in [
        ("security.yaml", check_security("other-version")),
        ("complexity.yaml", check_complexity("other-version")),
    ] {
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert_eq!(
            stderr(&output),
            format!(
                "{file}: version '0.2.0' differs from version '0.1.0' of families.yaml \
                 (at /version)\n\
                 1 problem found\n"
            )
        );
    }
}

#[test]
fn core_problems_of_every_file_are_reported_together() {
    // The family that security.yaml names is checked though families.yaml has problems.
    let dir = rules_dir("core-problems-of-every-file");
    use_fixture(&dir, "families", "invalid-regex");
    use_fixture(&dir, "security", "unknown-family");
    use_fixture(&dir, "complexity", "duplicate-trait-id");
    let output = check_dir(&dir);

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(
        stderr(&output),
        "families.yaml: fmt.loop-cells: invalid regex '\\bFor\\s+(Each': unclosed group \
         (at /families/0/signals/2/pattern)\n\
         families.yaml: fmt.style-name: invalid regex '[Heading': unclosed character class \
         (at /families/0/signals/3/pattern/1)\n\
         security.yaml: sec.very-hidden: unknown family '01-mise-en-forme' \
         (at /signals/3/requires_family)\n\
         complexity.yaml: trivial: duplicate trait id, first defined at /traits/1 \
         (at /traits/3/id)\n\
         4 problems found\n"
    );
}

#[test]
fn file_problems_of_every_file_are_reported_together() {
    let dir = rules_dir("file-problems-of-every-file");
    use_fixture(&dir, "families", "invalid-yaml");
    fs::write(dir.join("security.yaml"), b"").expect("security.yaml emptied");
    use_fixture(&dir, "complexity", "unknown-structural");
    let output = check_dir(&dir);

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(
        stderr(&output),
        format!(
            "families.yaml: invalid YAML: mismatched bracket '[' closed by '}}' \
             at line 13, column 55\n\
             security.yaml: empty document\n\
             complexity.yaml: possibly_unused: 'unused' is not one of: udf_candidate, \
             event_handler, auto_exec, chr_obfuscation, long_hex_or_base64_literal, \
             nested_loop_compare, possibly_unused, parse_recovered (at /traits/2/structural)\n\
             3 problems found{FILE_STAGE}\n"
        )
    );
}

#[test]
fn the_core_checks_no_file_until_every_file_passes_the_file_stage() {
    // The invalid regex of security.yaml waits for families.yaml to match its schema.
    let dir = rules_dir("families-schema-first");
    use_fixture(&dir, "families", "unknown-matcher");
    use_fixture(&dir, "security", "invalid-regex");
    let output = check_dir(&dir);

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(
        stderr(&output),
        format!(
            "families.yaml: fmt.cell-style: 'membre' is not one of: member, chain, call, progid, \
             procedure_name, module_type, reference, regex, string, comment, structural \
             (at /families/0/signals/0/match)\n\
             1 problem found{FILE_STAGE}\n"
        )
    );

    // The invalid regexes of families.yaml wait for security.yaml to be a document.
    let dir = rules_dir("security-document-first");
    use_fixture(&dir, "families", "invalid-regex");
    fs::write(dir.join("security.yaml"), b"").expect("security.yaml emptied");
    let output = check_dir(&dir);

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(
        stderr(&output),
        format!("security.yaml: empty document\n1 problem found{FILE_STAGE}\n")
    );

    // And for security.yaml to match its schema.
    let dir = rules_dir("security-schema-first");
    use_fixture(&dir, "families", "invalid-regex");
    use_fixture(&dir, "security", "unknown-matcher");
    let output = check_dir(&dir);

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(
        stderr(&output),
        format!(
            "security.yaml: sec.shell: 'membre' is not one of: member, chain, call, progid, \
             procedure_name, module_type, reference, regex, string, comment, structural \
             (at /signals/1/match)\n\
             1 problem found{FILE_STAGE}\n"
        )
    );

    // And for complexity.yaml to match its schema.
    let dir = rules_dir("complexity-schema-first");
    use_fixture(&dir, "families", "invalid-regex");
    use_fixture(&dir, "complexity", "unknown-matcher");
    let output = check_dir(&dir);

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(
        stderr(&output),
        format!(
            "complexity.yaml: recorded_macro: 'commentaire' is not one of: comment, \
             procedure_name, ratio (at /traits/0/any/0/match)\n\
             1 problem found{FILE_STAGE}\n"
        )
    );
}

#[test]
fn unknown_security_keys_are_rejected_at_every_level() {
    let output = check_security("unknown-keys");

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(
        stderr(&output),
        format!(
            "security.yaml: unknown key 'author' (at /author)\n\
             security.yaml: sec.chain.autoexec-execution: unknown key 'note' (at /chains/0/note)\n\
             security.yaml: sec.source-missing: unknown key 'category' \
             (at /container_indicators/0/category)\n\
             security.yaml: sec.auto-exec: unknown key 'weight' (at /signals/0/weight)\n\
             security.yaml: sec.shell: unknown key 'de' (at /signals/1/label/de)\n\
             5 problems found{FILE_STAGE}\n"
        )
    );
}

#[test]
fn unknown_security_matcher_is_rejected() {
    let output = check_security("unknown-matcher");

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(
        stderr(&output),
        format!(
            "security.yaml: sec.shell: 'membre' is not one of: member, chain, call, progid, \
             procedure_name, module_type, reference, regex, string, comment, structural \
             (at /signals/1/match)\n\
             1 problem found{FILE_STAGE}\n"
        )
    );
}

#[test]
fn unknown_severity_is_rejected() {
    let output = check_security("unknown-severity");

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(
        stderr(&output),
        format!(
            "security.yaml: sec.chain.autoexec-execution: 'critical' is not one of: info, low, \
             medium, high (at /chains/0/severity)\n\
             security.yaml: sec.source-missing: 'critical' is not one of: info, low, medium, \
             high (at /container_indicators/0/severity)\n\
             security.yaml: sec.shell: 'critical' is not one of: info, low, medium, high \
             (at /signals/1/severity)\n\
             3 problems found{FILE_STAGE}\n"
        )
    );
}

#[test]
fn unknown_security_scope_is_rejected() {
    let output = check_security("unknown-scope");

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(
        stderr(&output),
        format!(
            "security.yaml: sec.declare: 'workbook' is not one of: procedure, module, project \
             (at /signals/2/scope)\n\
             1 problem found{FILE_STAGE}\n"
        )
    );
}

#[test]
fn security_structural_predicate_outside_the_closed_list_is_rejected() {
    let output = check_security("unknown-structural");

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(
        stderr(&output),
        format!(
            "security.yaml: sec.auto-exec: 'autoexec' is not one of: udf_candidate, \
             event_handler, auto_exec, chr_obfuscation, long_hex_or_base64_literal, \
             nested_loop_compare, possibly_unused, parse_recovered (at /signals/0/pattern/1)\n\
             1 problem found{FILE_STAGE}\n"
        )
    );
}

#[test]
fn security_labels_need_english_and_french() {
    let output = check_security("missing-label");

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(
        stderr(&output),
        format!(
            "security.yaml: sec.chain.autoexec-execution: missing key 'en' (at /chains/0/label)\n\
             security.yaml: sec.source-missing: missing key 'label' (at /container_indicators/0)\n\
             security.yaml: sec.shell: missing key 'fr' (at /signals/1/label)\n\
             3 problems found{FILE_STAGE}\n"
        )
    );
}

#[test]
fn chain_term_lists_are_never_empty() {
    let output = check_security("empty-chain-terms");

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(
        stderr(&output),
        format!(
            "security.yaml: sec.chain.autoexec-execution: [] has less than 1 item \
             (at /chains/0/all/1/any)\n\
             security.yaml: sec.chain.empty: [] has less than 1 item (at /chains/1/all)\n\
             2 problems found{FILE_STAGE}\n"
        )
    );
}

#[test]
fn chain_terms_are_signal_ids_or_any_lists() {
    let output = check_security("invalid-chain-term");

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(
        stderr(&output),
        format!(
            "security.yaml: sec.chain.autoexec-execution: missing key 'any' (at /chains/0/all/1)\n\
             security.yaml: sec.chain.autoexec-execution: unknown key 'anyy' \
             (at /chains/0/all/1/anyy)\n\
             security.yaml: sec.chain.autoexec-execution: [\"sec.declare\"] is not of types \
             \"string\", \"object\" (at /chains/0/all/2)\n\
             3 problems found{FILE_STAGE}\n"
        )
    );
}

#[test]
fn unknown_complexity_keys_are_rejected_at_every_level() {
    let output = check_complexity("unknown-keys");

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(
        stderr(&output),
        format!(
            "complexity.yaml: unknown key 'author' (at /author)\n\
             complexity.yaml: unknown key 'low' (at /file_levels/low)\n\
             complexity.yaml: unknown key 'note' (at /file_levels/medium/any/0/note)\n\
             complexity.yaml: recorded_macro: unknown key 'note' (at /traits/0/any/0/note)\n\
             complexity.yaml: recorded_macro: unknown key 'note' (at /traits/0/any/2/note)\n\
             complexity.yaml: recorded_macro: unknown key 'de' (at /traits/0/label/de)\n\
             complexity.yaml: recorded_macro: unknown key 'weight' (at /traits/0/weight)\n\
             complexity.yaml: trivial: unknown key 'note' (at /traits/1/all/0/note)\n\
             8 problems found{FILE_STAGE}\n"
        )
    );
}

#[test]
fn metrics_must_be_defined_in_metric_definitions() {
    let output = check_complexity("undefined-metric");

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(
        stderr(&output),
        "complexity.yaml: metric 'cyclomatic' is not defined in metric_definitions \
         (at /file_levels/medium/any/1/metric)\n\
         complexity.yaml: trivial: metric 'loc' is not defined in metric_definitions \
         (at /traits/1/all/0/metric)\n\
         2 problems found\n"
    );
}

#[test]
fn the_old_name_of_cyclomatic_is_rejected() {
    // Version 0.1.0 of complexity.yaml called the metric `max_cyclomatic`
    // where the report says `cyclomatic`. A file that still does, in a level,
    // in the definitions and in a trait, is no longer accepted (#29).
    let output = check_complexity("old-metric-name");

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(
        stderr(&output),
        format!(
            "complexity.yaml: 'max_cyclomatic' is not one of: {METRICS} \
             (at /file_levels/medium/any/1/metric)\n\
             complexity.yaml: unknown key 'max_cyclomatic' (at /metric_definitions/max_cyclomatic)\n\
             complexity.yaml: trivial: 'max_cyclomatic' is not one of: {METRICS} \
             (at /traits/1/all/1/metric)\n\
             3 problems found{FILE_STAGE}\n"
        )
    );
}

#[test]
fn metric_conditions_take_exactly_one_bound() {
    let output = check_complexity("metric-bounds");

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(
        stderr(&output),
        format!(
            "complexity.yaml: needs exactly one of: gte, lte (at /file_levels/medium/any/1)\n\
             complexity.yaml: trivial: needs exactly one of: gte, lte (at /traits/1/all/0)\n\
             2 problems found{FILE_STAGE}\n"
        )
    );
}

#[test]
fn traits_take_exactly_one_rule() {
    let output = check_complexity("trait-rules");

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(
        stderr(&output),
        format!(
            "complexity.yaml: trivial: needs exactly one of: any, all, structural (at /traits/1)\n\
             complexity.yaml: possibly_unused: needs exactly one of: any, all, structural \
             (at /traits/2)\n\
             2 problems found{FILE_STAGE}\n"
        )
    );
}

#[test]
fn trait_structural_predicate_outside_the_closed_list_is_rejected() {
    let output = check_complexity("unknown-structural");

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(
        stderr(&output),
        format!(
            "complexity.yaml: possibly_unused: 'unused' is not one of: udf_candidate, \
             event_handler, auto_exec, chr_obfuscation, long_hex_or_base64_literal, \
             nested_loop_compare, possibly_unused, parse_recovered (at /traits/2/structural)\n\
             1 problem found{FILE_STAGE}\n"
        )
    );
}

#[test]
fn invalid_trait_regexes_are_rejected() {
    let output = check_complexity("invalid-regex");

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(
        stderr(&output),
        "complexity.yaml: recorded_macro: invalid regex '(macro recorded': unclosed group \
         (at /traits/0/any/0/pattern)\n\
         complexity.yaml: recorded_macro: invalid regex '[Macro': unclosed character class \
         (at /traits/0/any/1/pattern/1)\n\
         2 problems found\n"
    );
}

#[test]
fn trait_ids_are_unique() {
    let output = check_complexity("duplicate-trait-id");

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(
        stderr(&output),
        "complexity.yaml: trivial: duplicate trait id, first defined at /traits/1 \
         (at /traits/3/id)\n\
         1 problem found\n"
    );
}

#[test]
fn trait_labels_need_english_and_french() {
    let output = check_complexity("missing-label");

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(
        stderr(&output),
        format!(
            "complexity.yaml: recorded_macro: missing key 'fr' (at /traits/0/label)\n\
             complexity.yaml: trivial: missing key 'label' (at /traits/1)\n\
             complexity.yaml: possibly_unused: missing key 'en' (at /traits/2/label)\n\
             3 problems found{FILE_STAGE}\n"
        )
    );
}

#[test]
fn condition_lists_are_never_empty() {
    let output = check_complexity("empty-conditions");

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(
        stderr(&output),
        format!(
            "complexity.yaml: [] has less than 1 item (at /file_levels/high/any)\n\
             complexity.yaml: trivial: [] has less than 1 item (at /traits/1/all)\n\
             2 problems found{FILE_STAGE}\n"
        )
    );
}

#[test]
fn unknown_condition_matcher_is_rejected() {
    let output = check_complexity("unknown-matcher");

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(
        stderr(&output),
        format!(
            "complexity.yaml: recorded_macro: 'commentaire' is not one of: comment, \
             procedure_name, ratio (at /traits/0/any/0/match)\n\
             1 problem found{FILE_STAGE}\n"
        )
    );
}
