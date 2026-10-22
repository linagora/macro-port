// SPDX-License-Identifier: Apache-2.0
//! A4 (docs/ACCEPTANCE.md): the usage families of the text corpus.
//!
//! The ground truth is `family` in `corpus/manifest.json`, which `corpus_slug`
//! in `rules/families.yaml` maps to the id of a family. The samples that count
//! are those that the manifest includes, extracts or makes synthetic, with a
//! file, in families 01 to 13: 74 of the 80 text samples (family 14 has no
//! family rules, see `security_corpus.rs`). Each goes through `analyze()`, which
//! reads a `.bas`, `.cls` or `.frm` file as a text module, and its report is
//! judged:
//!
//! - top-1: the `primary_family` of the file is the family of the sample;
//! - top-2: the family of the sample is one of the two best `families` of the
//!   file;
//! - recall: the top-2 rate within each family;
//! - evidence: each family score of a procedure cites lines of the procedure,
//!   and cites at least one.
//!
//! The test prints the confusion matrix and the samples that are not found,
//! and fails when a rate is below its threshold in ACCEPTANCE A4.

// clippy.toml excuses `expect` in the `#[test]` functions only, not in helpers.
#![allow(clippy::expect_used)]

use std::collections::BTreeMap;
use std::sync::LazyLock;

use macroport_core::analyze::{Options, analyze};
use macroport_core::report::{FamilyScore, FileReport, Lang, ModuleReport, Report};
use macroport_core::rules::{RuleSet, RulesSources};
use macroport_testkit::{corpus, report_problems};
use serde::Deserialize;

/// The thresholds of ACCEPTANCE A4, in percent of the samples: the file-level
/// top-1 and top-2 accuracy, and the top-2 recall of each family.
const TOP_1: usize = 75;
const TOP_2: usize = 90;
const TOP_2_PER_FAMILY: usize = 60;

/// The `primary_family` of a file that no family claims.
const UNCLASSIFIED: &str = "unclassified";

static RULES: LazyLock<RuleSet> = LazyLock::new(|| {
    RuleSet::load(&RulesSources::EMBEDDED).expect("the rules that ship with the tool load")
});

/// An entry of `corpus/manifest.json`: the fields that A4 reads.
#[derive(Deserialize)]
struct Entry {
    id: String,
    family: String,
    status: String,
    file: Option<String>,
    path: Option<String>,
}

/// What the ground truth says of a sample.
struct Truth {
    /// The id of the sample (`01-01`).
    id: String,
    /// The id of the family of the sample in the rules (`01-formatting`).
    family: String,
}

/// A text sample, and the report that `analyze()` makes of it.
struct Judged {
    /// Relative to `corpus/`.
    path: String,
    file: FileReport,
    /// `None` for a sample that A4 does not judge, those of family 14.
    truth: Option<Truth>,
}

impl Judged {
    /// The `primary_family` of the file.
    fn primary(&self) -> &str {
        self.file.primary_family.as_deref().unwrap_or(UNCLASSIFIED)
    }

    /// The `n` best family scores of the file, best first.
    fn best(&self, n: usize) -> impl Iterator<Item = &FamilyScore> {
        self.file.families.iter().flatten().take(n)
    }

    /// The two best families of the file, best first.
    fn best_two(&self) -> Vec<&str> {
        let scores = self.best(2);
        scores.map(|score| score.family.as_str()).collect()
    }

    /// The family of the sample in the ground truth.
    fn family(&self) -> Option<&str> {
        self.truth.as_ref().map(|truth| truth.family.as_str())
    }

    /// Whether the file is classified as the ground truth says.
    fn is_top_1(&self) -> bool {
        self.family() == Some(self.primary())
    }

    /// Whether the family of the ground truth is one of the two best.
    fn is_top_2(&self) -> bool {
        self.family()
            .is_some_and(|family| self.best_two().contains(&family))
    }
}

/// The ground truth that A4 judges, by the path of the sample relative to
/// `corpus/`.
fn ground_truth() -> BTreeMap<String, Truth> {
    let manifest = corpus::read("manifest.json");
    let manifest: Vec<Entry> = serde_json::from_slice(&manifest).expect("the manifest reads");
    let ids: BTreeMap<&str, &str> = (RULES.families.families.iter())
        .map(|family| (family.corpus_slug.as_str(), family.id.as_str()))
        .collect();

    let mut truth = BTreeMap::new();
    for entry in manifest {
        let counts = matches!(entry.status.as_str(), "included" | "extract" | "synthetic");
        let Some(path) = entry.path.filter(|_| counts && entry.file.is_some()) else {
            continue;
        };
        let Some(family) = ids.get(entry.family.as_str()) else {
            // Family 14 is judged on its signals, not on a family.
            assert!(
                entry.family.starts_with("14-"),
                "{}: the family {} has no rules",
                entry.id,
                entry.family
            );
            continue;
        };
        let family = (*family).to_owned();
        truth.insert(
            path,
            Truth {
                id: entry.id,
                family,
            },
        );
    }
    truth
}

/// The problems of `file` against the report schema, as the first file of a
/// fleet report.
fn report_problems_of(file: &FileReport) -> Vec<String> {
    let mut report = Report::empty(&RULES, "1970-01-01T00:00:00Z".to_owned(), Lang::En);
    report.files = vec![file.clone()];
    report_problems(&report.to_json().expect("the report serialises"))
}

/// Every text sample of the corpus, analysed once for the tests of this file.
/// Each report is valid against the report schema.
static JUDGED: LazyLock<Vec<Judged>> = LazyLock::new(|| {
    let mut truth = ground_truth();
    let judged: Vec<Judged> = corpus::files("families", &["bas", "cls", "frm"])
        .into_iter()
        .map(|path| {
            let file = analyze(&corpus::read(&path), &path, &Options::new(&RULES));
            assert_eq!(report_problems_of(&file), Vec::<String>::new(), "{path}");
            let truth = truth.remove(&path);
            Judged { path, file, truth }
        })
        .collect();
    let unknown: Vec<_> = truth.keys().collect();
    assert_eq!(unknown, Vec::<&String>::new(), "samples not in the corpus");
    judged
});

/// The modules of the VBA project of `file`.
fn modules(file: &FileReport) -> impl Iterator<Item = &ModuleReport> {
    file.vba_project.iter().flat_map(|project| &project.modules)
}

/// The number of a family, the first two characters of its id (`01`), or `--`
/// for an id too short to have one.
fn family_number(family: &str) -> &str {
    family.get(..2).unwrap_or("--")
}

/// What the samples of one family got: how many there are, and how many were
/// found at top-1 and at top-2.
#[derive(Default)]
struct Found {
    samples: usize,
    top_1: usize,
    top_2: usize,
}

#[test]
fn the_samples_are_found_in_their_family_at_the_rates_of_a4() {
    let samples: Vec<&Judged> = JUDGED.iter().filter(|j| j.truth.is_some()).collect();
    let total = samples.len();
    assert!(total >= 74, "{total} samples");
    let families: Vec<&str> = (RULES.families.families.iter())
        .map(|family| family.id.as_str())
        .collect();

    // The confusion matrix, and what the samples of each family got.
    let mut matrix: BTreeMap<(&str, &str), usize> = BTreeMap::new();
    let mut by_family: BTreeMap<&str, Found> = BTreeMap::new();
    for sample in &samples {
        let family = sample.family().expect("a judged sample");
        *matrix.entry((family, sample.primary())).or_default() += 1;
        let found = by_family.entry(family).or_default();
        found.samples += 1;
        found.top_1 += usize::from(sample.is_top_1());
        found.top_2 += usize::from(sample.is_top_2());
    }
    assert_eq!(by_family.len(), families.len(), "a family has no sample");

    println!("A4 confusion matrix: ground truth in rows, primary family of the file in columns");
    let columns = families.iter().copied().chain([UNCLASSIFIED]);
    let header: Vec<String> = columns
        .clone()
        .map(|id| format!("{:>3}", family_number(id)))
        .collect();
    println!("{:<18}{}   top-1 top-2", "", header.join(""));
    for family in &families {
        let cells: Vec<String> = columns
            .clone()
            .map(|primary| match matrix.get(&(*family, primary)) {
                Some(count) => format!("{count:>3}"),
                None => "  .".to_owned(),
            })
            .collect();
        let found = by_family.get(family).expect("a family with samples");
        let (n, top_1, top_2) = (found.samples, found.top_1, found.top_2);
        println!(
            "{family:<18}{}   {top_1:>2}/{n:<2} {top_2:>2}/{n:<2}",
            cells.join("")
        );
    }

    let top_1 = samples.iter().filter(|sample| sample.is_top_1()).count();
    let top_2 = samples.iter().filter(|sample| sample.is_top_2()).count();
    let percent = |part: usize, whole: usize| 100.0 * part as f64 / whole as f64;
    println!(
        "A4: {total} samples, top-1 {top_1} ({:.1} %), top-2 {top_2} ({:.1} %)",
        percent(top_1, total),
        percent(top_2, total)
    );
    for sample in &samples {
        if !sample.is_top_1() {
            let truth = sample.truth.as_ref().expect("a judged sample");
            let scores: Vec<String> = sample
                .best(3)
                .map(|score| format!("{} {}", score.family, score.score))
                .collect();
            println!(
                "not top-1: {} is {}, found {} ({})",
                truth.id,
                truth.family,
                sample.primary(),
                scores.join(", ")
            );
        }
    }

    let mut problems = Vec::new();
    if top_1 * 100 < TOP_1 * total {
        problems.push(format!("top-1 is {top_1} of {total}, under {TOP_1} %"));
    }
    if top_2 * 100 < TOP_2 * total {
        problems.push(format!("top-2 is {top_2} of {total}, under {TOP_2} %"));
    }
    for (family, found) in &by_family {
        if found.top_2 * 100 < TOP_2_PER_FAMILY * found.samples {
            problems.push(format!(
                "{family}: top-2 recall is {} of {}, under {TOP_2_PER_FAMILY} %",
                found.top_2, found.samples
            ));
        }
    }
    assert_eq!(problems, Vec::<String>::new());
}

#[test]
fn every_family_score_of_a_procedure_has_evidence_inside_the_procedure() {
    let mut scores = 0;
    let mut problems = Vec::new();
    for judged in JUDGED.iter() {
        for module in modules(&judged.file) {
            for procedure in &module.procedures {
                let inside = procedure.start_line..=procedure.end_line;
                for score in &procedure.families {
                    scores += 1;
                    let evidence = score.evidence.as_deref().unwrap_or_default();
                    let all_inside = evidence.iter().all(|item| inside.contains(&item.line));
                    if evidence.is_empty() || !all_inside {
                        problems.push(format!(
                            "{}: {} of {} has no evidence, or some outside lines {}-{}",
                            judged.path,
                            score.family,
                            procedure.name,
                            procedure.start_line,
                            procedure.end_line
                        ));
                    }
                }
            }
        }
    }
    assert!(scores > 0, "no family score to check");
    assert_eq!(problems, Vec::<String>::new());
}
