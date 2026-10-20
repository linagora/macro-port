// SPDX-License-Identifier: Apache-2.0
//! Reading the test corpus.

use macroport_testkit::corpus;

#[test]
fn files_lists_the_text_samples_sorted_with_slashes() {
    let samples = corpus::files("families", &["bas", "cls", "frm"]);
    assert_eq!(samples.len(), 80);
    assert!(samples.is_sorted());
    assert!(
        samples
            .iter()
            .all(|path| path.starts_with("families/") && !path.contains('\\'))
    );
    assert!(samples.contains(
        &"families/14-patterns-securite/14-02-workbook-open-obfuscation-chr.cls".to_owned()
    ));
}

#[test]
fn files_without_extensions_keeps_every_file() {
    let malformed = corpus::files("fixtures/malformed", &[]);
    assert!(malformed.contains(&"fixtures/malformed/expected.yaml".to_owned()));
    assert!(malformed.contains(&"fixtures/malformed/zip-bomb.xlsm".to_owned()));
}

#[test]
fn read_returns_the_bytes_of_a_file() {
    assert!(corpus::read("manifest.json").starts_with(b"["));
}
