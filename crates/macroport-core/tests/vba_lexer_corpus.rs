// SPDX-License-Identifier: Apache-2.0
//! Smoke test of the lexer draft: every logical line of every text sample of
//! the corpus and of Excel's exports tokenises without panic, and nothing but
//! whitespace lies between two tokens.

use macroport_core::vba::{split_lines, tokenize};
use macroport_testkit::corpus;

#[test]
fn every_logical_line_of_the_corpus_tokenises_without_loss() {
    let mut tokens = 0usize;
    for dir in ["families", "fixtures/generated"] {
        for path in corpus::files(dir, &["bas", "cls", "frm"]) {
            let text = String::from_utf8_lossy(&corpus::read(&path)).into_owned();
            let lines = split_lines(&text).expect("lines");
            for line in &lines.logical {
                let mut at = 0usize;
                for token in tokenize(&line.text) {
                    let gap = line.text.get(at..token.start).expect("a gap");
                    assert!(gap.chars().all(char::is_whitespace), "{path}: {line:?}");
                    assert!(token.start < token.end, "{path}: {line:?}");
                    at = token.end;
                    tokens += 1;
                }
                let rest = line.text.get(at..).expect("the end");
                assert!(rest.chars().all(char::is_whitespace), "{path}: {line:?}");
            }
        }
    }
    assert!(tokens > 100_000, "{tokens} tokens");
}
