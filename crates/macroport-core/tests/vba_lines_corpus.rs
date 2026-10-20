// SPDX-License-Identifier: Apache-2.0
//! The line splitter on the text samples of the corpus and on the exports of
//! Excel itself: no panic, every logical line is where it says, and no
//! physical line is lost.

use macroport_core::vba::{LogicalKind, SkipKind, split_lines};
use macroport_testkit::corpus;

fn samples(dir: &str) -> Vec<(String, String)> {
    let paths = corpus::files(dir, &["bas", "cls", "frm"]);
    assert!(!paths.is_empty(), "no sample in {dir}");
    paths
        .into_iter()
        .map(|path| {
            let text = String::from_utf8_lossy(&corpus::read(&path)).into_owned();
            (path, text)
        })
        .collect()
}

/// The samples end their lines with LF, and the exports of Excel with CR LF:
/// `str::lines` reads both.
#[test]
fn every_logical_line_points_to_its_first_physical_line() {
    for dir in ["families", "fixtures/generated"] {
        for (path, text) in samples(dir) {
            let lines = split_lines(&text).expect("lines");
            let physical: Vec<&str> = text.lines().collect();
            assert_eq!(lines.physical_lines as usize, physical.len(), "{path}");
            let mut previous = 0;
            for line in &lines.logical {
                let ordered = previous <= line.line
                    && line.line <= line.end_line
                    && line.end_line <= lines.physical_lines;
                assert!(ordered, "{path}: {line:?}");
                previous = line.line;
                let index = line.line.saturating_sub(1) as usize;
                let first = physical.get(index).expect("a physical line");
                let own = match line.kind {
                    LogicalKind::Code | LogicalKind::Label => &line.text,
                    LogicalKind::CommentOnly => &line.comment.as_ref().expect("a comment").text,
                };
                let word = own.split(' ').next().unwrap_or_default();
                assert!(first.contains(word), "{path}: line {} {word:?}", line.line);
            }
        }
    }
}

/// Flags the physical lines `first` to `last` (1-based).
fn mark(flags: &mut [bool], first: u32, last: u32) {
    for number in first..=last {
        if let Some(flag) = flags.get_mut((number as usize).saturating_sub(1)) {
            *flag = true;
        }
    }
}

/// Every physical line is blank, skipped or part of a logical line, never
/// both skipped and part of one, and the skipped runs are ordered.
#[test]
fn every_physical_line_is_accounted_for() {
    for dir in ["families", "fixtures/generated"] {
        for (path, text) in samples(dir) {
            let lines = split_lines(&text).expect("lines");
            let count = lines.physical_lines as usize;
            let (mut skipped, mut code) = (vec![false; count], vec![false; count]);
            let mut previous = 0;
            for run in &lines.skipped {
                let ordered = previous < run.first && run.first <= run.last;
                assert!(ordered && run.last <= lines.physical_lines, "{path}");
                previous = run.last;
                mark(&mut skipped, run.first, run.last);
            }
            for line in &lines.logical {
                mark(&mut code, line.line, line.end_line);
            }
            let flags = skipped.iter().zip(&code).zip(text.lines());
            for (number, ((skip, held), physical)) in flags.enumerate() {
                let line = number + 1;
                assert!(!(*skip && *held), "{path}: line {line} is skipped and code");
                let blank = physical.trim().is_empty();
                assert!(*skip || *held || blank, "{path}: line {line} is lost");
            }
        }
    }
}

#[test]
fn every_sample_starts_with_its_corpus_header_and_every_form_with_its_designer() {
    for (path, text) in samples("families") {
        let lines = split_lines(&text).expect("lines");
        let first = lines.skipped.first().expect("a corpus header");
        assert_eq!(
            (first.first, first.kind),
            (1, SkipKind::CorpusHeader),
            "{path}"
        );
        let export = lines
            .skipped
            .iter()
            .any(|run| run.kind == SkipKind::ExportHeader);
        let has_version = text.lines().any(|line| line.starts_with("VERSION "));
        assert_eq!(export, has_version, "{path}");
        assert!(!path.ends_with(".frm") || export, "{path}");
    }
}

/// Excel's exports end their lines with CR LF and hold every `Attribute` line
/// at the start of a line: the splitter finds as many as a plain search does.
#[test]
fn excel_exports_give_the_attributes_a_reader_counts() {
    for (path, text) in samples("fixtures/generated") {
        let lines = split_lines(&text).expect("lines");
        let attributes = text
            .lines()
            .filter(|line| line.starts_with("Attribute "))
            .count();
        assert_eq!(lines.attributes.len(), attributes, "{path}");
    }
}
