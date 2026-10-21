// SPDX-License-Identifier: Apache-2.0
//! The line splitter on the text samples of the corpus and on the exports of
//! Excel itself: no panic, and every logical line is where it says.

use macroport_core::vba::{LogicalKind, LogicalLine, split_lines};
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

/// The text of `line` cut where `line_at` moves to another physical line: the
/// code of each physical line the statement occupies, with its number.
fn pieces(line: &LogicalLine) -> Vec<(u32, &str)> {
    let mut cuts: Vec<(usize, u32)> = Vec::new();
    for (at, _) in line.text.char_indices() {
        let number = line.line_at(at);
        if cuts.last().is_none_or(|&(_, last)| last != number) {
            cuts.push((at, number));
        }
    }
    let ends = cuts
        .iter()
        .skip(1)
        .map(|&(at, _)| at)
        .chain([line.text.len()]);
    cuts.iter()
        .zip(ends)
        .filter_map(|(&(at, number), end)| Some((number, line.text.get(at..end)?.trim())))
        .collect()
}

/// The samples end their lines with LF, and the exports of Excel with CR LF:
/// `str::lines` reads both. Each piece of code must open the physical line it
/// is attributed to, so a line number that is off by one is caught unless the
/// next line opens with the same code.
#[test]
fn every_logical_line_points_to_its_first_physical_line() {
    for dir in ["families", "fixtures/generated"] {
        for (path, text) in samples(dir) {
            let lines = split_lines(&text).expect("lines");
            let physical: Vec<&str> = text.lines().collect();
            assert_eq!(lines.physical_lines as usize, physical.len(), "{path}");
            let at = |number: u32| {
                let index = number.saturating_sub(1) as usize;
                physical
                    .get(index)
                    .copied()
                    .unwrap_or_default()
                    .trim_start()
            };
            let mut previous = 0;
            for line in &lines.logical {
                let ordered = previous <= line.line
                    && line.line <= line.end_line
                    && line.end_line <= lines.physical_lines;
                assert!(ordered, "{path}: {line:?}");
                previous = line.line;
                match line.kind {
                    LogicalKind::Code => {
                        for (number, piece) in pieces(line) {
                            assert!(at(number).starts_with(piece), "{path}: {number} {piece:?}");
                        }
                    }
                    LogicalKind::CommentOnly => {
                        let first = at(line.line);
                        let rem = first
                            .get(..3)
                            .is_some_and(|w| w.eq_ignore_ascii_case("rem"));
                        assert!(first.starts_with('\'') || rem, "{path}: {line:?}");
                    }
                }
                if let Some(comment) = &line.comment {
                    let word = comment.text.split(' ').next().unwrap_or_default();
                    assert!(at(comment.line).contains(word), "{path}: {comment:?}");
                }
            }
        }
    }
}
