// SPDX-License-Identifier: Apache-2.0
//! A5 (docs/ACCEPTANCE.md): the security signals of the text corpus.
//!
//! The ground truth is `corpus/ground-truth/security-expected.yaml`: the
//! signals that each sample of family 14 must raise. The samples go through
//! `analyze_modules`, as modules of a text file, so that the signals are those
//! of the whole pipeline: front end, rule engine and report. Every expected
//! signal must be raised. The other bullet of A5, the obfuscated ProgID of
//! 14-02 resolved to `WScript.Shell`, is checked here through
//! `sec.wscript-run` on that sample.

// The helpers are test code: a panic in one is a failed test. `clippy.toml`
// only exempts the bodies of `#[test]` functions. The file reads the corpus
// through `macroport_testkit::corpus`, so that it needs no exception for the
// file system either.
#![allow(clippy::expect_used)]

use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

use macroport_core::analyze::{Options, analyze_modules};
use macroport_core::report::FileReport;
use macroport_core::rules::{ModuleType, RuleSet, RulesSources, Severity};
use macroport_core::vba::ModuleSource;
use macroport_testkit::corpus;

static RULES: LazyLock<RuleSet> = LazyLock::new(|| {
    RuleSet::load(&RulesSources::EMBEDDED).expect("the rules that ship with the tool load")
});

/// The module of the sample `file`, whose text is `text`. The name of a
/// module is that of its file, and a class module that handles the events of
/// the workbook or of a sheet is a document module.
fn module_of(file: &str, text: String) -> ModuleSource {
    let name = file.rsplit('/').next().unwrap_or(file);
    let (stem, extension) = name.rsplit_once('.').unwrap_or((name, ""));
    let module_type = match extension {
        "bas" => ModuleType::Standard,
        "frm" => ModuleType::Form,
        _ if text.contains("Workbook_") || text.contains("Worksheet_") => ModuleType::Document,
        _ => ModuleType::Class,
    };
    ModuleSource {
        name: stem.to_owned(),
        module_type,
        text,
    }
}

/// The report of the sample `file` of the corpus, relative to `corpus/`.
fn report_of(file: &str) -> FileReport {
    let text = String::from_utf8_lossy(&corpus::read(file)).into_owned();
    analyze_modules(vec![module_of(file, text)], file, &Options::new(&RULES))
}

/// Every signal of a file, wherever the report puts it.
fn signals(file: &FileReport) -> Vec<(&str, Severity)> {
    let project = file.vba_project.iter().flat_map(|project| &project.modules);
    let in_modules = project.flat_map(|module| {
        let procedures = module
            .procedures
            .iter()
            .flat_map(|procedure| &procedure.signals);
        module.signals.iter().flatten().chain(procedures)
    });
    let found = file.signals.iter().flatten().chain(in_modules);
    found
        .map(|signal| (signal.signal_id.as_str(), signal.severity))
        .collect()
}

/// The expected signals of each sample, by the id of the sample (`14-01`).
fn expected() -> BTreeMap<String, Vec<String>> {
    let text = corpus::read("ground-truth/security-expected.yaml");
    let text = String::from_utf8(text).expect("the ground truth is UTF-8");
    serde_saphyr::from_str(&text).expect("the ground truth reads")
}

/// The sample of family 14 with the id `id`, relative to `corpus/`.
fn sample(id: &str) -> String {
    let prefix = format!("families/14-patterns-securite/{id}-");
    let found: Vec<_> = (corpus::files("families/14-patterns-securite", &["bas", "cls", "frm"]))
        .into_iter()
        .filter(|file| file.starts_with(&prefix))
        .collect();
    assert_eq!(found.len(), 1, "one sample for {id}: {found:?}");
    found.into_iter().next().expect("one sample")
}

#[test]
fn every_expected_signal_of_the_security_samples_is_raised() {
    let expected = expected();
    // A ground truth that lost its samples would pass for nothing.
    assert!(expected.len() >= 6, "{} samples", expected.len());
    let (mut raised_count, mut total) = (0, 0);
    let mut missing = Vec::new();

    for (id, wanted) in &expected {
        let report = report_of(&sample(id));
        let raised: BTreeSet<&str> = signals(&report).into_iter().map(|(id, _)| id).collect();
        println!("{id}: raised {raised:?}");
        for signal in wanted {
            total += 1;
            if raised.contains(signal.as_str()) {
                raised_count += 1;
            } else {
                missing.push(format!("{id}: {signal} is not raised"));
            }
        }
    }

    println!("A5: {raised_count} of {total} expected signals raised");
    assert_eq!(missing, Vec::<String>::new());
}

#[test]
fn no_sample_outside_family_14_raises_a_high_signal() {
    let samples = corpus::files("families", &["bas", "cls", "frm"]);
    let outside: Vec<_> = (samples.iter())
        .filter(|file| !file.starts_with("families/14-"))
        .collect();
    assert!(outside.len() >= 74, "{} samples", outside.len());

    let mut high = Vec::new();
    for file in outside {
        let report = report_of(file);
        if report.max_severity == Some(Severity::High) {
            high.push(format!("{file}: the file has a high severity"));
        }
        for (signal, severity) in signals(&report) {
            if severity == Severity::High {
                high.push(format!("{file}: {signal}"));
            }
        }
    }
    assert_eq!(high, Vec::<String>::new());
}

#[test]
fn the_security_samples_are_read_and_judged_by_the_engine() {
    for id in expected().keys() {
        let report = report_of(&sample(id));

        assert!(report.primary_family.is_some(), "{id}: no primary family");
        assert!(report.signals.is_some(), "{id}: no list of signals");
        assert!(report.traits.is_some(), "{id}: no list of traits");
    }
}
