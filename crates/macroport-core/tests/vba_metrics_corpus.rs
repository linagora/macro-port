// SPDX-License-Identifier: Apache-2.0
//! The procedures of the corpus whose blocks do not match (SPEC M2 item 6): the
//! text samples and Excel's own exports of the binary fixtures, every procedure
//! that the parser finds measured by `measure_procedure`.

// clippy.toml excuses `expect` in the `#[test]` functions only, not in helpers.
#![allow(clippy::expect_used)]

use std::collections::BTreeMap;

use macroport_core::vba::{measure_procedure, parse, split_lines};
use macroport_testkit::corpus;

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

/// The number of each sample and the name of each procedure whose blocks do not
/// match, with how many modules of the corpus hold it. A sample is also in the
/// corpus as the exports that Excel made of its three workbooks, which have the
/// number of the sample in their names.
fn unbalanced_procedures() -> BTreeMap<(String, String), usize> {
    let mut found = BTreeMap::new();
    for (path, text) in modules() {
        let lines = split_lines(&text).expect("lines");
        for procedure in parse(&lines).procedures {
            let code = lines.logical.get(procedure.lines.clone());
            let measure = measure_procedure(
                procedure.start_line,
                procedure.end_line,
                procedure.parameter_count,
                code.expect("the lines of a procedure"),
            );
            if measure.unbalanced_blocks {
                let file = path.rsplit('/').next().unwrap_or_default();
                let sample = file.chars().take("00-00".len()).collect();
                let holding = found.entry((sample, procedure.name)).or_insert(0usize);
                *holding = holding.saturating_add(1);
            }
        }
    }
    found
}

#[test]
fn two_procedures_of_the_corpus_have_blocks_that_do_not_match() {
    // 09-03 is a sample with deliberate errors: `Worksheet_Change` has an `Else`
    // and an `End If` that no `If` holds. 13-05 has a line of prose, `Do
    // manipulations of your file here.`, that opens a `Do` which nothing closes.
    // Each of the two samples is one module of the text corpus and three exports.
    let expected: BTreeMap<(String, String), usize> = [
        (("09-03".to_owned(), "Worksheet_Change".to_owned()), 4),
        (("13-05".to_owned(), "GetExcel".to_owned()), 4),
    ]
    .into();

    assert_eq!(unbalanced_procedures(), expected);
}
