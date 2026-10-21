// SPDX-License-Identifier: Apache-2.0
//! The lexer on the whole corpus (SPEC M2: no panic on any corpus file, the
//! deliberate compile errors included): the text samples and Excel's own
//! exports, read as logical lines, as physical lines and as one line.

mod vba_lexer_common;

use macroport_core::vba::{MAX_TOKENS, TokenKind, split_lines, tokenize};
use macroport_testkit::corpus;
use vba_lexer_common::tiling_problem;

/// The path and the text of every module of the text samples and of Excel's
/// exports.
fn modules() -> Vec<(String, String)> {
    ["families", "fixtures/generated"]
        .into_iter()
        .flat_map(|dir| {
            let paths = corpus::files(dir, &["bas", "cls", "frm"]);
            assert!(!paths.is_empty(), "no module in {dir}");
            paths
        })
        .map(|path| {
            let text = String::from_utf8_lossy(&corpus::read(&path)).into_owned();
            (path, text)
        })
        .collect()
}

#[test]
fn every_logical_line_of_the_corpus_is_cut_into_known_tokens() {
    let mut tokens = 0usize;
    let mut problems = Vec::new();
    for (path, text) in modules() {
        let lines = split_lines(&text).expect("lines");
        for line in &lines.logical {
            let found = tokenize(&line.text);
            tokens += found.len();
            if let Some(problem) = tiling_problem(&line.text, &found) {
                problems.push(format!("{path}:{}: {problem}", line.line));
            }
            // Real code holds no character that starts no token, and the
            // splitter cuts the comments off: only a `Rem` after `Then` or
            // `Else` is left.
            let odd = found.iter().find(|token| match token.kind {
                TokenKind::Unknown => true,
                TokenKind::Comment => token.text(&line.text).starts_with('\''),
                _ => false,
            });
            if let Some(token) = odd {
                let text = token.text(&line.text);
                problems.push(format!("{path}:{}: {:?} {text:?}", line.line, token.kind));
            }
        }
    }
    assert!(problems.is_empty(), "{problems:#?}");
    assert!(tokens > 100_000, "{tokens} tokens");
}

#[test]
fn every_physical_line_of_the_corpus_is_cut_without_loss() {
    let mut comments = 0usize;
    for (path, text) in modules() {
        for (number, line) in text.lines().enumerate() {
            let found = tokenize(line);
            let problem = tiling_problem(line, &found);
            assert_eq!(problem, None, "{path}:{}: {line}", number.saturating_add(1));
            comments += found
                .iter()
                .filter(|token| token.kind == TokenKind::Comment)
                .count();
        }
    }
    assert!(comments > 100, "{comments} comments");
}

#[test]
fn the_code_of_every_module_of_the_corpus_is_cut_without_loss_as_one_line() {
    for (path, text) in modules() {
        let lines = split_lines(&text).expect("lines");
        let code: Vec<&str> = lines
            .logical
            .iter()
            .map(|line| line.text.as_str())
            .collect();
        let line = code.join(" ");
        let found = tokenize(&line);
        assert!(found.len() < MAX_TOKENS, "{path}: {} tokens", found.len());
        assert_eq!(tiling_problem(&line, &found), None, "{path}");
    }
}
