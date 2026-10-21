// SPDX-License-Identifier: Apache-2.0
//! The redaction of a report (SPEC section 8, ADR-0007): what it hashes, what it
//! removes and what it leaves. The hashes were computed apart from the code
//! under test, with Python's `hmac` module and the key `key`.

#![allow(clippy::expect_used)]

use macroport_core::redact::{Redaction, remove_snippets};
use macroport_core::report::Report;
use serde_json::{Value, json};

const EXAMPLE: &str = include_str!("../../../schema/examples/report.example.json");

const KEY: &[u8] = b"key";

// First 16 hexadecimal digits of HMAC-SHA256 under `KEY`, of:
/// `The quick brown fox jumps over the lazy dog`, the vector of Wikipedia.
const FOX: &str = "f7bc83f430538424";
/// `/srv/clients/acme`
const ROOT: &str = "094834cb035cdf23";
/// `examples`
const SHORT_ROOT: &str = "b7ac6ee2bd4e8fca";
/// `/srv/clients/acme/Q3 budget.xlsm`
const PATH: &str = "57e87dabd54d2365";
/// `AcmeBudget`, in the case it is written in.
const PROJECT: &str = "7e4d81bd1d6ee16c";
/// `m_14_01`: the module `M_14_01`, which VBA writes in any case.
const MODULE: &str = "07295cd3f68512f1";
/// `auto_open`: the procedure `Auto_Open`.
const PROCEDURE: &str = "e3bf1cebadcd237f";
/// `helper`
const HELPER: &str = "3c86544d78e5ce2e";

/// The example report with the strings that a redaction has to hide: the paths
/// of a client, an error that quotes one, the name of the project, a call to
/// another procedure and a signal of the module.
fn sample() -> Report {
    let mut value: Value = serde_json::from_str(EXAMPLE).expect("the example is JSON");
    let module = "/files/0/vba_project/modules/0";
    for (pointer, new) in [
        ("/scan/roots", json!(["/srv/clients/acme", "examples"])),
        ("/files/0/path", json!("/srv/clients/acme/Q3 budget.xlsm")),
        (
            "/files/0/errors",
            json!([{
                "code": "ooxml.truncated",
                "message": "cannot read /srv/clients/acme/Q3 budget.xlsm"
            }]),
        ),
        ("/files/0/vba_project/name", json!("AcmeBudget")),
        (
            &format!("{module}/signals"),
            json!([{
                "signal_id": "sec.declare",
                "severity": "medium",
                "module": "M_14_01",
                "evidence": [{
                    "rule_id": "sec.declare",
                    "line": 2,
                    "snippet": "Declare Function GetTickCount Lib \"kernel32\" () As Long"
                }]
            }]),
        ),
        (
            &format!("{module}/procedures/0/calls"),
            json!(["Helper", "M_14_01.Helper"]),
        ),
    ] {
        *value.pointer_mut(pointer).expect(pointer) = new;
    }
    serde_json::from_value(value).expect("the sample is a report")
}

fn to_value(report: &Report) -> Value {
    serde_json::to_value(report).expect("a report is JSON")
}

/// The sample, redacted under `KEY`, as JSON.
fn redacted(identifiers: bool) -> Value {
    let mut report = sample();
    Redaction::new(KEY, identifiers)
        .expect("HMAC takes any key")
        .apply(&mut report);
    to_value(&report)
}

/// `value` without the `snippet` of any evidence.
fn without_snippets(mut value: Value) -> Value {
    fn strip(value: &mut Value) {
        match value {
            Value::Object(map) => {
                map.remove("snippet");
                map.values_mut().for_each(strip);
            }
            Value::Array(items) => items.iter_mut().for_each(strip),
            _ => {}
        }
    }
    strip(&mut value);
    value
}

/// Replaces the value at `pointer` of `value`, which has to exist.
fn replace(value: &mut Value, pointer: &str, new: Value) {
    *value.pointer_mut(pointer).expect(pointer) = new;
}

#[test]
fn a_hash_is_the_first_16_digits_of_the_hmac_sha256_of_the_text() {
    let mut report = sample();
    report.scan.roots = Some(vec![
        "The quick brown fox jumps over the lazy dog".to_owned(),
    ]);

    Redaction::new(KEY, false)
        .expect("a key")
        .apply(&mut report);

    assert_eq!(report.scan.roots, Some(vec![FOX.to_owned()]));
}

#[test]
fn redaction_changes_what_the_table_of_the_spec_lists_and_nothing_else() {
    let before = sample();
    let after = redacted(false);

    // The same report without the snippets, with the values of the table.
    let mut expected = without_snippets(to_value(&before));
    for (pointer, new) in [
        ("/redacted", json!(true)),
        ("/scan/roots", json!([ROOT, SHORT_ROOT])),
        ("/files/0/path", json!(format!("{PATH}.xlsm"))),
        ("/files/0/errors/0/message", json!("[redacted]")),
        ("/files/0/vba_project/name", json!(PROJECT)),
    ] {
        replace(&mut expected, pointer, new);
    }
    assert_eq!(after, expected);
}

#[test]
fn identifiers_are_hashed_in_every_place_that_names_the_project() {
    let after = redacted(true);

    // As without identifiers, and the names, in lowercase as VBA reads them:
    // the module and its stream, the procedure, the module and the procedure
    // of the signals, and the calls, a qualified name part by part.
    let mut expected = without_snippets(to_value(&sample()));
    let module = "/files/0/vba_project/modules/0";
    for (pointer, new) in [
        ("/redacted", json!(true)),
        ("/scan/roots", json!([ROOT, SHORT_ROOT])),
        ("/files/0/path", json!(format!("{PATH}.xlsm"))),
        ("/files/0/errors/0/message", json!("[redacted]")),
        ("/files/0/vba_project/name", json!(PROJECT)),
        (&format!("{module}/name"), json!(MODULE)),
        (&format!("{module}/stream_name"), json!(MODULE)),
        (&format!("{module}/signals/0/module"), json!(MODULE)),
        (&format!("{module}/procedures/0/name"), json!(PROCEDURE)),
        (
            &format!("{module}/procedures/0/calls"),
            json!([HELPER, format!("{MODULE}.{HELPER}")]),
        ),
        (
            &format!("{module}/procedures/0/signals/0/module"),
            json!(MODULE),
        ),
        (
            &format!("{module}/procedures/0/signals/0/procedure"),
            json!(PROCEDURE),
        ),
        (
            &format!("{module}/procedures/0/signals/1/module"),
            json!(MODULE),
        ),
        (
            &format!("{module}/procedures/0/signals/1/procedure"),
            json!(PROCEDURE),
        ),
    ] {
        replace(&mut expected, pointer, new);
    }
    assert_eq!(after, expected);
}

#[test]
fn what_comes_from_outside_the_project_stays_even_with_identifiers() {
    let before = to_value(&sample());
    let after = redacted(true);
    let procedure = "/files/0/vba_project/modules/0/procedures/0";

    // The references, the APIs that a procedure uses, the identifiers of the
    // rules and the hashes of the contents.
    for pointer in [
        "/files/0/vba_project/references",
        &format!("{procedure}/apis"),
        &format!("{procedure}/signals/0/signal_id"),
        &format!("{procedure}/signals/0/evidence/0/rule_id"),
        "/files/0/sha256",
        "/files/0/vba_project/modules/0/source_sha256",
        "/files/0/errors/0/code",
    ] {
        assert_eq!(after.pointer(pointer), before.pointer(pointer), "{pointer}");
        assert!(after.pointer(pointer).is_some(), "{pointer}");
    }
}

#[test]
fn identifiers_are_hashed_without_regard_to_case() {
    let mut report = sample();
    let module = report
        .files
        .first_mut()
        .and_then(|file| file.vba_project.as_mut())
        .and_then(|project| project.modules.first_mut())
        .expect("the sample has a module");
    module.name = "m_14_01".to_owned();
    module.stream_name = Some("M_14_01".to_owned());
    let procedure = module
        .procedures
        .first_mut()
        .expect("the sample has a procedure");
    procedure.name = "AUTO_OPEN".to_owned();

    Redaction::new(KEY, true).expect("a key").apply(&mut report);

    let after = to_value(&report);
    let module = "/files/0/vba_project/modules/0";
    assert_eq!(
        after.pointer(&format!("{module}/name")),
        Some(&json!(MODULE))
    );
    assert_eq!(
        after.pointer(&format!("{module}/stream_name")),
        Some(&json!(MODULE))
    );
    assert_eq!(
        after.pointer(&format!("{module}/procedures/0/name")),
        Some(&json!(PROCEDURE))
    );
}

#[test]
fn nothing_that_names_a_file_is_left() {
    let names = ["M_14_01", "Auto_Open", "Helper"];
    let others = [
        "acme",
        "Q3 budget",
        "srv/clients",
        "AcmeBudget",
        "notepad.exe",
        "kernel32",
        "\"snippet\"",
        "cannot read",
    ];
    // The strings are in the report to start with, or the test proves nothing.
    let clear = to_value(&sample()).to_string();
    for text_of_the_file in others.iter().chain(&names) {
        assert!(clear.contains(text_of_the_file), "{text_of_the_file}");
    }

    for identifiers in [false, true] {
        let text = redacted(identifiers).to_string();
        for text_of_the_file in others {
            assert!(
                !text.contains(text_of_the_file),
                "identifiers {identifiers}: {text_of_the_file}"
            );
        }
        // The names stay when they are not asked for: reviewers need them.
        for name in names {
            assert_eq!(
                text.contains(name),
                !identifiers,
                "identifiers {identifiers}: {name}"
            );
        }
    }
}

#[test]
fn the_same_key_gives_the_same_report_and_another_key_another() {
    let again = redacted(true);
    let mut other = sample();
    Redaction::new(b"another key", true)
        .expect("a key")
        .apply(&mut other);
    let other = to_value(&other);

    assert_eq!(redacted(true), again);
    assert_ne!(
        other.pointer("/files/0/path"),
        again.pointer("/files/0/path")
    );
    assert_ne!(other.pointer("/scan/roots"), again.pointer("/scan/roots"));
    assert_ne!(
        other.pointer("/files/0/vba_project/modules/0/name"),
        again.pointer("/files/0/vba_project/modules/0/name")
    );
}

#[test]
fn a_path_keeps_the_extension_of_the_files_that_a_scan_reads_and_no_other() {
    let cases = [
        ("/srv/clients/acme/Q3 budget.xlsm", Some("xlsm")),
        ("BOOK.XLSM", Some("XLSM")),
        ("C:\\Users\\alice\\old.xls", Some("xls")),
        ("archive.tar.bas", Some("bas")),
        ("form.frm", Some("frm")),
        // Text after the last dot can be part of a name that is hidden.
        ("notes.docx", None),
        ("acme.client.invoice", None),
        ("dir.xlsm/readme", None),
        ("dir.xlsm\\readme", None),
        ("no_extension", None),
        ("trailing.", None),
        (".xlsm", Some("xlsm")),
    ];
    for (path, extension) in cases {
        let mut report = sample();
        report.files.truncate(1);
        report.files[0].path = path.to_owned();

        Redaction::new(KEY, false)
            .expect("a key")
            .apply(&mut report);

        // 16 lowercase hexadecimal digits, then the extension if one is kept.
        let redacted = &report.files[0].path;
        let hash = match extension {
            Some(extension) => redacted.strip_suffix(&format!(".{extension}")),
            None => Some(redacted.as_str()),
        };
        let hash = hash.unwrap_or_else(|| panic!("{path}: {redacted}"));
        assert_eq!(hash.len(), 16, "{path}: {redacted}");
        assert!(
            hash.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')),
            "{path}: {redacted}"
        );
    }
}

#[test]
fn files_are_sorted_again_by_their_redacted_paths() {
    let clear = ["a.bas", "b.bas", "c.bas", "d.bas", "e.bas", "f.bas"];
    let mut report = sample();
    let file = report.files[0].clone();
    report.files = clear
        .iter()
        .map(|path| {
            let mut file = file.clone();
            file.path = (*path).to_owned();
            file
        })
        .collect();

    Redaction::new(KEY, false)
        .expect("a key")
        .apply(&mut report);

    // Under this key the hashes of a.bas to f.bas are 0ac7, 956c, 65db, ee27,
    // 5a10 and 2f10: they do not keep the order of the clear paths, and the
    // report is in the order of the new ones, as a report always is.
    let paths: Vec<&str> = report.files.iter().map(|file| file.path.as_str()).collect();
    assert_eq!(
        paths,
        [
            "0ac7d70e000aa894.bas",
            "2f10d15a3b6ff852.bas",
            "5a10a05106469d61.bas",
            "65db73ef9df8ac0e.bas",
            "956c5cb0b85b895f.bas",
            "ee271aeedddc911a.bas",
        ]
    );
}

#[test]
fn removing_the_snippets_alone_keeps_the_paths_the_names_and_the_flag() {
    let mut report = sample();
    let before = to_value(&report);
    assert!(before.to_string().contains("\"snippet\""));

    remove_snippets(&mut report);

    assert!(!report.redacted);
    assert_eq!(to_value(&report), without_snippets(before));
}

#[test]
fn a_key_of_any_length_works_and_is_not_shown() {
    assert!(Redaction::new(&[], false).is_ok());
    assert!(Redaction::new(&[7; 4096], false).is_ok());

    let redaction = Redaction::new(b"hunter2-the-key", true).expect("a key");
    let shown = format!("{redaction:?}");

    assert!(shown.contains("identifiers: true"), "{shown}");
    assert!(!shown.contains("hunter2"), "{shown}");
}
