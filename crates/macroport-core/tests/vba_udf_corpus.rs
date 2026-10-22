// SPDX-License-Identifier: Apache-2.0
//! The functions of the corpus that the predicate `udf_candidate` (SPEC section
//! 7) turns down: every public or default function of the standard modules, in
//! the text samples and in Excel's own exports of the binary fixtures.

// clippy.toml excuses `expect` in the `#[test]` functions only, not in helpers.
#![allow(clippy::expect_used)]

use std::collections::BTreeMap;

use macroport_core::report::ProcedureKind;
use macroport_core::rules::ModuleType;
use macroport_core::vba::{is_udf_candidate, parse, split_lines};
use macroport_testkit::corpus;

/// The path and the text of every standard module of the text samples and of
/// Excel's exports.
fn modules() -> Vec<(String, String)> {
    ["families", "fixtures/generated"]
        .into_iter()
        .flat_map(|dir| {
            let paths = corpus::files(dir, &["bas"]);
            assert!(!paths.is_empty(), "no module in {dir}");
            paths
        })
        .map(|path| {
            let text = String::from_utf8_lossy(&corpus::read(&path)).into_owned();
            (path, text)
        })
        .collect()
}

/// For the number of each sample and the name of each of its public or default
/// functions, whether it is a candidate in each module of the corpus that holds
/// it. A sample is also in the corpus as the exports that Excel made of its
/// three workbooks, which have the number of the sample in their names.
fn candidates() -> BTreeMap<(String, String), Vec<bool>> {
    let mut found: BTreeMap<_, Vec<bool>> = BTreeMap::new();
    for (path, text) in modules() {
        let lines = split_lines(&text).expect("lines");
        let file = path.rsplit('/').next().unwrap_or_default();
        let sample: String = file.chars().take("00-00".len()).collect();
        for procedure in parse(&lines).procedures {
            if procedure.kind == ProcedureKind::Function && procedure.is_public() {
                let candidate = is_udf_candidate(ModuleType::Standard, &procedure, &lines);
                (found.entry((sample.clone(), procedure.name)).or_default()).push(candidate);
            }
        }
    }
    found
}

#[test]
fn two_functions_of_the_corpus_are_not_candidates() {
    // Both call `MsgBox`: `RegexFindObject` to say that a match is missing,
    // `SuperMid` to report an error. Every other public or default function is a
    // candidate. Each of the two samples is one module of the text corpus and
    // three exports.
    let found = candidates();
    let turned_down: BTreeMap<_, _> = (found.clone().into_iter())
        .filter(|(_, candidates)| candidates.contains(&false))
        .collect();
    let expected: BTreeMap<_, _> = [
        (
            ("08-01".to_owned(), "RegexFindObject".to_owned()),
            vec![false; 4],
        ),
        (("08-02".to_owned(), "SuperMid".to_owned()), vec![false; 4]),
    ]
    .into();

    assert_eq!(turned_down, expected);

    // `GetListProjectChecked` of sample 06-01 compares `.Value` with `True` in
    // `If .Cells(i, colCheck).value = True Then`: the calibration prototype
    // reads an assignment there and turns the function down.
    let key = ("06-01".to_owned(), "GetListProjectChecked".to_owned());
    assert_eq!(found.get(&key), Some(&vec![true; 4]));
}
