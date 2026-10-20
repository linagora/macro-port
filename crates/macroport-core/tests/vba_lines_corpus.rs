// SPDX-License-Identifier: Apache-2.0
//! The line splitter on the text samples of the corpus and on the exports of
//! Excel itself: no panic, and every logical line is where it says.

use macroport_core::vba::{LogicalKind, split_lines};
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
                    LogicalKind::Code => &line.text,
                    LogicalKind::CommentOnly => &line.comment.as_ref().expect("a comment").text,
                };
                let word = own.split(' ').next().unwrap_or_default();
                assert!(first.contains(word), "{path}: line {} {word:?}", line.line);
            }
        }
    }
}
