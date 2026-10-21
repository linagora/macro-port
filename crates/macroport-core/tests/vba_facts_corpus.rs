// SPDX-License-Identifier: Apache-2.0
//! The facts of the whole corpus (SPEC M2 items 3 and 4): the text samples and
//! Excel's own exports of the binary fixtures, read statement by statement as
//! the parser will give them. The ProgIDs that the security samples of family
//! 14 need (ACCEPTANCE A5) are pinned, and a second, much cruder reader checks
//! the first on every other statement.

// clippy.toml excuses `expect` in the `#[test]` functions only, not in helpers.
#![allow(clippy::expect_used)]

use std::collections::BTreeSet;

use macroport_core::vba::{ConstTable, Facts, LogicalKind, StatementReader, parse, split_lines};
use macroport_testkit::corpus;
use regex::Regex;

/// The obfuscated sample of family 14, whose `CreateObject` is given a class
/// built from pieces.
const OBFUSCATED: &str = "families/14-patterns-securite/14-02-workbook-open-obfuscation-chr.cls";

/// A statement of a module with its facts.
struct Statement {
    /// The module it is in, by its path in the corpus.
    path: String,
    /// The physical line where it starts.
    line: u32,
    /// Its code.
    text: String,
    /// What the reader made of it.
    facts: Facts,
}

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

/// The statements of a module, read as the parser will read them: the
/// constants declared outside the procedures first, then every statement in
/// order by one reader.
fn read_module(path: &str, text: &str) -> Vec<Statement> {
    let source = split_lines(text).expect("lines");
    let parsed = parse(&source);
    let mut consts = ConstTable::default();
    for (index, line) in source.logical.iter().enumerate() {
        let inside = parsed
            .procedures
            .iter()
            .any(|procedure| procedure.lines.contains(&index));
        if !inside {
            consts.declare(line);
        }
    }
    let mut reader = StatementReader::new(&consts);
    source
        .logical
        .iter()
        .filter(|line| line.kind == LogicalKind::Code)
        .map(|line| Statement {
            path: path.to_string(),
            line: line.line,
            text: line.text.clone(),
            facts: reader.read(line),
        })
        .collect()
}

/// Every statement of the corpus.
fn statements() -> Vec<Statement> {
    modules()
        .iter()
        .flat_map(|(path, text)| read_module(path, text))
        .collect()
}

/// The statements of the modules whose path contains `sample`.
fn of_sample<'a>(all: &'a [Statement], sample: &str) -> Vec<&'a Statement> {
    all.iter()
        .filter(|statement| statement.path.contains(sample))
        .collect()
}

#[test]
fn no_statement_of_the_corpus_nests_deeper_than_the_reader_goes() {
    // The reader lists no ProgID in the part of a statement that it reads
    // flat: none of the corpus may hide one there.
    let all = statements();
    assert!(all.len() > 20_000, "{} statements", all.len());
    let deep: Vec<_> = all
        .iter()
        .filter(|statement| statement.facts.too_deep)
        .map(|statement| format!("{}:{}", statement.path, statement.line))
        .collect();
    assert!(deep.is_empty(), "{deep:#?}");
}

#[test]
fn the_obfuscated_progid_of_sample_14_02_folds_to_wscript_shell() {
    let all = statements();
    let sample = of_sample(&all, OBFUSCATED);
    let found: Vec<_> = sample
        .iter()
        .filter(|statement| !statement.facts.progids.is_empty())
        .collect();
    let [create] = found.as_slice() else {
        panic!("{} statements with a ProgID", found.len());
    };
    assert_eq!(
        create.text,
        r#"Set o = CreateObject("WScr" & "ipt.Sh" & "ell")"#
    );
    assert_eq!(create.facts.progids, ["WScript.Shell"]);
    assert_eq!(create.facts.strings, ["WScript.Shell"]);
}

#[test]
fn the_exports_of_sample_14_02_give_the_same_progids_as_the_text() {
    let all = statements();
    let progids_of = |sample: &str| -> Vec<(String, Vec<String>)> {
        of_sample(&all, sample)
            .iter()
            .filter(|s| !s.facts.progids.is_empty())
            .map(|s| (s.text.clone(), s.facts.progids.clone()))
            .collect()
    };
    let text = progids_of(OBFUSCATED);
    assert_eq!(text.len(), 1, "{text:?}");
    for extension in ["xls", "xlsb", "xlsm"] {
        let export = progids_of(&format!(
            "fixtures/generated/14-02-workbook-open-obfuscation-chr.{extension}.expected.cls"
        ));
        assert_eq!(export, text, "{extension}");
    }
}

#[test]
fn the_progids_of_the_security_samples_are_the_classes_they_create() {
    let all = statements();
    let progids_of = |sample: &str| -> BTreeSet<String> {
        of_sample(&all, sample)
            .iter()
            .flat_map(|statement| statement.facts.progids.iter().cloned())
            .collect()
    };
    for (sample, expected) in [
        ("14-01-", vec![]),
        ("14-02-", vec!["WScript.Shell"]),
        ("14-03-", vec!["ADODB.Stream", "MSXML2.XMLHTTP"]),
        ("14-04-", vec!["ADODB.Connection"]),
        ("14-05-", vec![]),
        ("14-06-", vec![]),
    ] {
        let expected: BTreeSet<String> = expected.into_iter().map(String::from).collect();
        let sample = format!("families/14-patterns-securite/{sample}");
        assert_eq!(progids_of(&sample), expected, "{sample}");
    }
}

#[test]
fn the_progids_written_out_in_the_corpus_are_exactly_the_ones_listed() {
    // A class written as a literal: `CreateObject("X.Y"[, ...])`, and the
    // second argument of `GetObject(path, "X.Y")`, never the path itself.
    let create =
        Regex::new(r#"(?i)\bcreateobject\s*\(\s*"([^"]*)"\s*[,)]"#).expect("a regular expression");
    let get = Regex::new(r#"(?i)\bgetobject\s*\(\s*[^,()"]*,\s*"([^"]*)"\s*\)"#)
        .expect("a regular expression");
    let all = statements();
    let mut written = 0usize;
    for statement in &all {
        let crude: Vec<String> = [&create, &get]
            .iter()
            .flat_map(|pattern| pattern.captures_iter(&statement.text))
            .map(|captures| captures[1].to_string())
            .collect();
        let place = format!("{}:{}: {}", statement.path, statement.line, statement.text);
        // The one class that is not written out is folded from pieces.
        let folded = statement.path.contains("14-02-") && statement.text.contains("\"ipt.Sh\"");
        if folded {
            assert!(crude.is_empty(), "{place}");
            continue;
        }
        let mut read = statement.facts.progids.clone();
        let mut expected = crude;
        read.sort();
        expected.sort();
        assert_eq!(read, expected, "{place}");
        written += expected.len();
    }
    assert!(written > 100, "{written} classes");
}
