// SPDX-License-Identifier: Apache-2.0
//! Tests of `scan --no-snippets` and `scan --redact` (SPEC section 8,
//! ACCEPTANCE A6): what the reports hide of the files, the key of the hashes,
//! and the options that go together. Most of them run the binary. Those that
//! name the library (`Privacy`, `reports::documents`) check it against the
//! core and the renderers that the binary is made of.

mod common;
mod support;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::ops::Range;
use std::path::Path;

use macroport_cli::args::Format;
use macroport_cli::i18n::Messages;
use macroport_cli::reports;
use macroport_cli::scan::{EXTENSIONS, Privacy, PrivacyArgs};
use macroport_core::redact::{KEPT_EXTENSIONS, Redaction, remove_snippets};
use macroport_core::report::{Lang, Report, Signal};
use macroport_core::rules::{RuleSet, RulesSources};
use macroport_testkit::{corpus, report_problems};
use serde_json::Value;
use support::{
    EVERY_REPORT, MODULE, as_arg, put, read_report, repo_root, report_json, run, run_in, scratch,
    text,
};

/// `--redact --redact-key k`, with `--redact-identifiers` if asked.
fn redact_with_key(identifiers: bool) -> PrivacyArgs {
    PrivacyArgs {
        redact: true,
        redact_key: Some("k".to_owned()),
        redact_identifiers: identifiers,
        ..PrivacyArgs::default()
    }
}

/// `report` once the options in `args` have been applied to it.
fn after(args: &PrivacyArgs, report: &Report) -> Report {
    let privacy = Privacy::new(args).expect("the privacy is made");
    let mut report = report.clone();
    privacy.apply(&mut report);
    report
}

/// The words of the reports, as the command has them without `--lang`.
fn messages() -> Messages {
    let rules = RuleSet::load(&RulesSources::EMBEDDED).expect("the embedded rules load");
    Messages::new(Lang::En, &rules)
}

/// The reports that `scan` wrote to `out`, by name.
fn written_reports(out: &Path) -> BTreeMap<&'static str, Vec<u8>> {
    EVERY_REPORT
        .into_iter()
        .map(|name| {
            let content = fs::read(out.join(name))
                .unwrap_or_else(|error| panic!("cannot read {name} in {}: {error}", out.display()));
            (name, content)
        })
        .collect()
}

/// Scans `tree` of `dir` into `dir/out` with every report, deterministically,
/// and the options `extra`. The scan has to succeed.
fn scan_tree(dir: &Path, out: &str, extra: &[&str]) {
    let mut args = vec!["scan", "tree", "-o", out, "--deterministic"];
    args.extend(extra);
    let output = run_in(dir, &args);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
}

/// Scans `root`, a path of the repository, into `out` with the options `extra`
/// and gives the report. The exit code is 0, or 1 when the root holds files that
/// cannot be analysed, as the corpus does (ACCEPTANCE A6).
fn scan_root(root: &str, out: &Path, extra: &[&str]) -> Report {
    let mut args = vec!["scan", root, "-o", as_arg(out), "--deterministic"];
    args.extend(extra);
    let output = run(&args);
    assert!(
        matches!(output.status.code(), Some(0 | 1)),
        "{args:?}: {output:?}"
    );
    read_report(out)
}

/// Whether `value` has an object with the key `key` anywhere in it.
fn has_key(value: &Value, key: &str) -> bool {
    match value {
        Value::Object(map) => map.contains_key(key) || map.values().any(|v| has_key(v, key)),
        Value::Array(items) => items.iter().any(|item| has_key(item, key)),
        _ => false,
    }
}

/// Whether `text` is what a redacted report puts in place of a path or a name:
/// 16 lowercase hexadecimal digits, then perhaps a dot and one of the
/// extensions that a scan reads (in any case).
fn is_hash(text: &str) -> bool {
    let (hash, extension) = match text.split_once('.') {
        Some((hash, extension)) => (hash, Some(extension)),
        None => (text, None),
    };
    hash.len() == 16
        && hash.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f'))
        && extension.is_none_or(|extension| {
            EXTENSIONS
                .iter()
                .any(|known| known.eq_ignore_ascii_case(extension))
        })
}

/// The `module` and the `procedure` that `signals` name.
fn signal_names<'a>(signals: impl IntoIterator<Item = &'a Signal>) -> Vec<String> {
    signals
        .into_iter()
        .flat_map(|signal| [signal.module.clone(), signal.procedure.clone()])
        .flatten()
        .collect()
}

/// Every name that the project of a report defines, where the report holds
/// one: the modules, their streams, the procedures, the parts of the calls
/// (which all name procedures of the project) and the modules and procedures
/// that the signals of a file, a module and a procedure name.
fn project_names(report: &Report) -> Vec<String> {
    let mut names = Vec::new();
    for file in &report.files {
        names.extend(signal_names(file.signals.iter().flatten()));
        let modules = file.vba_project.iter().flat_map(|project| &project.modules);
        for module in modules {
            names.push(module.name.clone());
            names.extend(module.stream_name.clone());
            names.extend(signal_names(module.signals.iter().flatten()));
            for procedure in &module.procedures {
                names.push(procedure.name.clone());
                let calls = procedure.calls.iter().flatten();
                names.extend(calls.flat_map(|call| call.split('.')).map(str::to_owned));
                names.extend(signal_names(&procedure.signals));
            }
        }
    }
    names
}

/// The names of the APIs that the procedures of a report use.
fn api_names(report: &Report) -> Vec<String> {
    let procedures = report
        .files
        .iter()
        .filter_map(|file| file.vba_project.as_ref())
        .flat_map(|project| &project.modules)
        .flat_map(|module| &module.procedures);
    procedures
        .flat_map(|procedure| procedure.apis.iter().flatten())
        .map(|api| api.name.clone())
        .collect()
}

/// What a redacted report keeps in clear on purpose, besides the words of the
/// report itself: the references of the projects and the APIs that the
/// procedures use (ADR-0007).
fn kept_names(report: &Report) -> Vec<String> {
    let projects = report.files.iter().filter_map(|f| f.vba_project.as_ref());
    let mut names: Vec<String> = projects
        .flat_map(|project| project.references.iter().flatten().cloned())
        .collect();
    names.extend(api_names(report));
    names
}

/// The words of the tool: what a literal of the corpus, or a name of a
/// project, can spell without having leaked anything. The reports of a scan of
/// a trivial file, made in `dir` (the page of the HTML report, the keys and the
/// values of the JSON), the schema, the rules, the texts of the interface, and
/// the codes of the errors of `report`.
fn vocabulary(dir: &Path, report: &Report) -> Vec<String> {
    put(dir, "tree/m.bas", MODULE);
    scan_tree(dir, "trivial", &["--redact"]);
    let mut words: Vec<String> = written_reports(&dir.join("trivial"))
        .values()
        .map(|content| text(content))
        .collect();
    let root = repo_root();
    let mut files = vec![root.join("schema/report.schema.json")];
    for directory in ["rules", "i18n/en", "i18n/fr"] {
        let entries = fs::read_dir(root.join(directory)).expect("a directory of the repository");
        files.extend(entries.map(|entry| entry.expect("a directory entry").path()));
    }
    for file in files {
        words.push(fs::read_to_string(&file).expect("a text file of the repository"));
    }
    let errors = report
        .files
        .iter()
        .flat_map(|file| file.errors.iter().flatten());
    words.extend(errors.map(|error| error.code.clone()));
    words
}

/// Whether one of `texts` has `needle` in it.
fn is_in(texts: &[String], needle: &str) -> bool {
    texts.iter().any(|text| text.contains(needle))
}

/// Whether one of `texts` shows `literal` other than across one of `kept`, the
/// names that a report keeps on purpose. A kept name next to the text of the
/// report spells what neither holds: `Err.LastDllError ×6` in the HTML spells
/// `Error `. A kept name inside the literal excuses nothing.
fn leaks(texts: &[String], literal: &str, kept: &[String]) -> bool {
    texts.iter().any(|text| {
        text.match_indices(literal).any(|(start, _)| {
            let end = start + literal.len();
            !kept.iter().any(|name| crosses(text, name, start..end))
        })
    })
}

/// Whether `name` is in `text` over a part of `range` and out of it.
fn crosses(text: &str, name: &str, range: Range<usize>) -> bool {
    // An occurrence that overlaps the range starts less than `name.len()` bytes
    // before it and ends less than as far after it.
    let from = text.floor_char_boundary(range.start.saturating_sub(name.len()));
    let to = text.ceil_char_boundary(range.end.saturating_add(name.len()));
    let Some(around) = text.get(from..to) else {
        return false;
    };
    around.match_indices(name).any(|(at, _)| {
        let (first, last) = (from + at, from + at + name.len());
        first < range.end && range.start < last && (first < range.start || last > range.end)
    })
}

/// The string literals of the VBA `source`: the text between double quotes,
/// outside the comments. A doubled quote inside a literal cuts it in two, which
/// leaves each half a piece of the text.
fn string_literals(source: &str) -> Vec<String> {
    let mut literals = Vec::new();
    for line in source.lines() {
        for (n, part) in line.split('"').enumerate() {
            if n % 2 == 1 {
                literals.push(part.to_owned());
            } else if part.contains('\'') {
                // The rest of the line is a comment.
                break;
            }
        }
    }
    literals
}

/// The string literals of the text modules of the corpus.
fn corpus_literals() -> BTreeSet<String> {
    corpus::files("families", &["bas", "cls", "frm"])
        .iter()
        .flat_map(|file| string_literals(&text(&corpus::read(file))))
        .collect()
}

/// Every string of `value`, the values of the objects and the items of the
/// arrays, as the JSON text spells it out: with the escapes undone.
fn strings_of(value: &Value, into: &mut Vec<String>) {
    match value {
        Value::String(text) => into.push(text.clone()),
        Value::Object(map) => map.values().for_each(|v| strings_of(v, into)),
        Value::Array(items) => items.iter().for_each(|item| strings_of(item, into)),
        _ => {}
    }
}

/// What the reports in `out` show: the text of each, and the strings of the
/// JSON with their escapes undone, which is how a quote or a backslash of a
/// literal is written there.
fn shown_by(out: &Path) -> Vec<String> {
    let mut shown: Vec<String> = written_reports(out)
        .values()
        .map(|content| text(content))
        .collect();
    let json: Value = serde_json::from_str(&report_json(out)).expect("report.json is JSON");
    strings_of(&json, &mut shown);
    shown
}

#[test]
fn the_options_give_the_privacy_that_the_core_applies() {
    let clear = common::fleet();

    // Nothing asked for, nothing changes.
    assert_eq!(after(&PrivacyArgs::default(), &clear), clear);

    // --no-snippets leaves the paths, the names and the flag.
    let mut expected = clear.clone();
    remove_snippets(&mut expected);
    let only_snippets = PrivacyArgs {
        no_snippets: true,
        ..PrivacyArgs::default()
    };
    let no_snippets = after(&only_snippets, &clear);
    assert_eq!(no_snippets, expected);
    assert!(!no_snippets.redacted);
    assert_ne!(no_snippets, clear, "the fleet has snippets to remove");

    // --redact is the redaction of the core under the bytes of the key, with
    // or without --no-snippets, which it includes.
    for identifiers in [false, true] {
        let mut expected = clear.clone();
        Redaction::new(b"k", identifiers)
            .expect("a key")
            .apply(&mut expected);
        assert!(expected.redacted);
        for no_snippets in [false, true] {
            let args = PrivacyArgs {
                no_snippets,
                ..redact_with_key(identifiers)
            };
            assert_eq!(
                after(&args, &clear),
                expected,
                "identifiers {identifiers}, no snippets {no_snippets}"
            );
        }
    }
}

#[test]
fn the_debug_text_of_the_arguments_and_of_the_privacy_hides_the_key() {
    let args = PrivacyArgs {
        redact: true,
        redact_key: Some("hunter2".to_owned()),
        ..PrivacyArgs::default()
    };
    let privacy = Privacy::new(&args).expect("the privacy is made");

    let shown = format!("{args:?}");
    assert!(shown.contains("redact: true"), "{shown}");
    assert!(!shown.contains("hunter2"), "{shown}");
    let shown = format!("{privacy:?}");
    assert!(!shown.contains("hunter2"), "{shown}");
}

#[test]
fn without_a_key_each_run_draws_its_own() {
    let clear = common::fleet();
    let args = PrivacyArgs {
        redact: true,
        redact_identifiers: true,
        ..PrivacyArgs::default()
    };
    let paths = |report: &Report| -> Vec<String> {
        report.files.iter().map(|file| file.path.clone()).collect()
    };

    let first = after(&args, &clear);
    let second = after(&args, &clear);

    assert_eq!(first.files.len(), second.files.len());
    assert_ne!(paths(&first), paths(&second));
    assert_ne!(first.scan.roots, second.scan.roots);
}

#[test]
fn every_report_is_written_from_the_report_after_the_privacy() {
    let dir = scratch("redact_oracle");
    for relative in [
        "tree/clients/acme/Q3 budget.bas",
        "tree/clients/acme/old.cls",
        "tree/clients/acme.bak/notes.frm",
    ] {
        put(&dir, relative, MODULE);
    }
    scan_tree(&dir, "clear", &[]);
    let clear = read_report(&dir.join("clear"));
    // The names that a redaction hides are in the reports to start with. The
    // CSV files of procedures and signals have no row until the analysis finds
    // some, so only these reports name the files.
    let naming_the_files = ["files.csv", "report.html", "report.json"];
    // The name of the file with its extension, not without: the module of a
    // text file is named after it, and a redaction that does not hash the
    // identifiers keeps the name of a module (ADR-0007).
    let hidden = ["Q3 budget.bas", "acme", "clients"];
    for name in naming_the_files {
        let content = text(&written_reports(&dir.join("clear"))[name]);
        for word in hidden {
            assert!(content.contains(word), "{name}: {word}");
        }
    }

    type Rewrite = fn(&mut Report);
    let redact: Rewrite = |report| {
        Redaction::new(b"k", false).expect("a key").apply(report);
    };
    let cases: [(&[&str], Rewrite, bool); 4] = [
        (&["--no-snippets"], remove_snippets, false),
        (&["--redact", "--redact-key", "k"], redact, true),
        (
            &["--redact", "--redact-key", "k", "--redact-identifiers"],
            |report| {
                Redaction::new(b"k", true).expect("a key").apply(report);
            },
            true,
        ),
        (
            &["--no-snippets", "--redact", "--redact-key", "k"],
            redact,
            true,
        ),
    ];
    let messages = messages();
    for (n, (flags, rewrite, hides_the_paths)) in cases.into_iter().enumerate() {
        let out = format!("case{n}");
        scan_tree(&dir, &out, flags);
        let mut expected = clear.clone();
        rewrite(&mut expected);

        // The JSON is the report after the privacy, and passes the schema.
        let written = written_reports(&dir.join(&out));
        assert_eq!(
            report_json(&dir.join(&out)),
            expected.to_json().expect("the report serialises"),
            "{flags:?}"
        );
        // The HTML and the CSV are rendered from that report, not from the
        // report of the scan.
        for format in [Format::Html, Format::Csv] {
            for document in reports::documents(format, &expected, &messages).expect("documents") {
                assert_eq!(
                    written[document.name], document.contents,
                    "{flags:?}: {}",
                    document.name
                );
            }
        }
        // The names of the files are in none of the reports, or still in all
        // those that name files when the paths are not to be hashed.
        for (name, content) in &written {
            let content = text(content);
            for word in hidden {
                if hides_the_paths {
                    assert!(!content.contains(word), "{flags:?}: {name}: {word}");
                } else if naming_the_files.contains(name) {
                    assert!(content.contains(word), "{flags:?}: {name}: {word}");
                }
            }
        }
    }
}

#[test]
fn the_same_key_gives_the_same_reports_and_no_key_gives_other_hashes() {
    let dir = scratch("redact_keys");
    put(&dir, "tree/a.bas", MODULE);
    put(&dir, "tree/b.bas", MODULE);
    let runs: [(&str, &[&str]); 5] = [
        ("one", &["--redact", "--redact-key", "k"]),
        ("two", &["--redact", "--redact-key", "k"]),
        ("other", &["--redact", "--redact-key", "another key"]),
        ("random-1", &["--redact", "--redact-identifiers"]),
        ("random-2", &["--redact", "--redact-identifiers"]),
    ];
    for (out, extra) in runs {
        scan_tree(&dir, out, extra);
    }
    let reports_of = |out: &str| written_reports(&dir.join(out));
    let hashes_of = |out: &str| {
        let report = read_report(&dir.join(out));
        let mut hashes = report.scan.roots.clone().unwrap_or_default();
        hashes.extend(report.files.iter().map(|file| file.path.clone()));
        hashes
    };

    // Across runs, under one key: the same bytes in every report.
    assert_eq!(reports_of("one"), reports_of("two"));
    // Under another key, or with a key of each run: other hashes.
    assert_ne!(hashes_of("one"), hashes_of("other"));
    assert_ne!(hashes_of("random-1"), hashes_of("random-2"));
    assert!(hashes_of("random-1").iter().all(|hash| is_hash(hash)));
}

#[test]
fn the_redaction_options_that_make_no_sense_alone_are_usage_errors() {
    let dir = scratch("redact_usage");
    put(&dir, "m.bas", MODULE);
    let cases: [(&[&str], &str); 4] = [
        (&["--redact-key", "k"], "--redact"),
        (&["--redact-identifiers"], "--redact"),
        (&["--redact", "--redact-key", ""], "--redact-key"),
        (&["--redact", "--redact-key"], "--redact-key"),
    ];

    for (extra, mentioned) in cases {
        let mut args = vec!["scan", "m.bas", "-o", "out"];
        args.extend(extra);

        let output = run_in(&dir, &args);

        // SPEC M4: exit code 2 is a usage error, and nothing was made.
        assert_eq!(output.status.code(), Some(2), "{args:?}: {output:?}");
        assert!(output.stdout.is_empty(), "{args:?}: {output:?}");
        assert!(
            text(&output.stderr).contains(mentioned),
            "{args:?}: {output:?}"
        );
        assert!(!dir.join("out").exists(), "{args:?}");
    }
}

#[test]
fn a_redacted_scan_of_the_corpus_follows_the_table_of_the_spec() {
    let dir = scratch("redact_corpus");
    let clear_out = dir.join("clear");
    let out = dir.join("redacted");
    let clear = run(&[
        "scan",
        "corpus",
        "-o",
        as_arg(&clear_out),
        "--format",
        "json",
        "--deterministic",
    ]);
    let redacted = run(&[
        "scan",
        "corpus",
        "-o",
        as_arg(&out),
        "--deterministic",
        "--redact",
        "--redact-key",
        "k",
        "--redact-identifiers",
    ]);

    // ACCEPTANCE A6: 0, or 1 when malformed fixtures are included.
    assert!(matches!(clear.status.code(), Some(0 | 1)), "{clear:?}");
    assert_eq!(redacted.status.code(), clear.status.code(), "{redacted:?}");
    let clear = read_report(&clear_out);
    let report = read_report(&out);
    assert!(report.redacted);
    assert_eq!(report.files.len(), clear.files.len());
    assert!(!report.files.is_empty());

    // No clear root, and the paths are hashes.
    assert!(report.scan.roots.iter().flatten().all(|root| is_hash(root)));
    assert!(report.files.iter().all(|file| is_hash(&file.path)));
    // No snippet, and every error message is the fixed text.
    let json: Value = serde_json::from_str(&report_json(&out)).expect("report.json is JSON");
    assert!(!has_key(&json, "snippet"));
    let messages: BTreeSet<&str> = report
        .files
        .iter()
        .flat_map(|file| file.errors.iter().flatten())
        .map(|error| error.message.as_str())
        .collect();
    assert_eq!(
        messages,
        BTreeSet::from(["[redacted]"]),
        "the corpus has errors"
    );
    // None of the corpus paths, or of the names of its files, in any report.
    for (name, content) in written_reports(&out) {
        let content = text(&content);
        for file in &clear.files {
            let file_name = file.path.rsplit('/').next().unwrap_or(&file.path);
            assert!(!content.contains(&file.path), "{name}: {}", file.path);
            assert!(!content.contains(file_name), "{name}: {file_name}");
        }
    }
}

#[test]
fn a_redacted_path_keeps_the_extensions_that_the_walk_collects() {
    // The core keeps the extension of the files that a scan reads, and the walk
    // of the CLI has its own list of the same files: they have to be one.
    assert_eq!(KEPT_EXTENSIONS, EXTENSIONS);
}

#[test]
fn a_kept_name_excuses_a_literal_that_it_crosses_and_not_one_that_holds_it() {
    let kept = ["Err.LastDllError".to_owned(), "Err".to_owned()];

    // The name and the text after it spell the literal: no leak.
    assert!(!leaks(&["Err.LastDllError ×6".to_owned()], "Error ", &kept));
    // The literal itself, with a kept name inside it: a leak.
    let snippet = ["MsgBox \"Error \" & Err.Number".to_owned()];
    assert!(leaks(&snippet, "Error ", &kept));
}

#[test]
fn a_redacted_scan_of_the_corpus_shows_none_of_its_string_literals() {
    // The shortest literal that is looked for: shorter ones are the digits, the
    // words and the extensions that any report holds.
    const SHORTEST: usize = 6;
    let dir = scratch("redact_literals");
    let clear = scan_root("corpus", &dir.join("clear"), &[]);
    let plain = scan_root(
        "corpus",
        &dir.join("plain"),
        &["--redact", "--redact-key", "k"],
    );
    let identifiers = scan_root(
        "corpus",
        &dir.join("identifiers"),
        &["--redact", "--redact-key", "k", "--redact-identifiers"],
    );

    // A literal that spells a word of the tool leaks nothing, so only the others
    // are looked for.
    let words = vocabulary(&dir, &clear);
    let literals: BTreeSet<String> = corpus_literals()
        .into_iter()
        .filter(|literal| literal.chars().count() >= SHORTEST)
        .filter(|literal| !is_in(&words, literal))
        .collect();
    assert!(literals.len() > 100, "the corpus has literals to look for");

    // The search can find what it looks for: the reports of a scan that hides
    // nothing show some of the literals. Without this, a report that the
    // analysis has not filled yet passes for a report that hides everything.
    let shown_clear = shown_by(&dir.join("clear"));
    let kept_clear = kept_names(&clear);
    assert!(
        literals
            .iter()
            .any(|literal| leaks(&shown_clear, literal, &kept_clear)),
        "no literal of the corpus is in the reports of a scan that hides nothing"
    );

    // A report keeps the references and the APIs on purpose, and the names of
    // the project unless --redact-identifiers hashes them (ADR-0007, points for
    // the owner): a literal that is one of these is not a leak.
    for (name, report, names_stay) in [
        ("plain", &plain, true),
        ("identifiers", &identifiers, false),
    ] {
        let mut allowed = kept_names(report);
        if names_stay {
            allowed.extend(project_names(report));
        }
        let shown = shown_by(&dir.join(name));
        for literal in literals.iter().filter(|literal| !is_in(&allowed, literal)) {
            assert!(!leaks(&shown, literal, &allowed), "{name}: {literal:?}");
        }
    }
}

#[test]
fn a_redacted_scan_of_the_corpus_hashes_the_names_of_the_project() {
    let dir = scratch("redact_identifiers");
    let clear = scan_root("corpus/families", &dir.join("clear"), &[]);
    let redacted = scan_root(
        "corpus/families",
        &dir.join("redacted"),
        &["--redact", "--redact-key", "k", "--redact-identifiers"],
    );
    let clear_names = project_names(&clear);
    let redacted_names = project_names(&redacted);

    assert!(
        !clear_names.is_empty(),
        "the scan found no module or procedure"
    );
    // Every name of the project is a hash, and no hash is a name that the
    // project defines (which is lowercased before it is hashed).
    assert_eq!(redacted_names.len(), clear_names.len());
    let defined: BTreeSet<String> = clear_names.iter().map(|name| name.to_lowercase()).collect();
    for name in &redacted_names {
        assert!(is_hash(name), "{name}");
        assert!(!defined.contains(name), "{name}");
    }
    // The APIs keep the names of the object model and of the libraries: none
    // starts with a name that the project defines.
    for api in api_names(&redacted) {
        let root = api.split('.').next().unwrap_or_default();
        assert!(!defined.contains(&root.to_lowercase()), "{api}");
    }

    // No report spells a name of the project, in any case. The names that the
    // vocabulary of the reports spells (a word of the tool, a reference, an
    // API) are not looked for, and nor are those of fewer than six characters,
    // which cannot be told from the short words that any report holds.
    let mut known = vocabulary(&dir, &redacted);
    known.extend(kept_names(&redacted));
    let known: Vec<String> = known.iter().map(|text| text.to_lowercase()).collect();
    let shown: Vec<String> = shown_by(&dir.join("redacted"))
        .iter()
        .map(|text| text.to_lowercase())
        .collect();
    let looked_for = defined
        .iter()
        .filter(|name| name.chars().count() >= 6 && !is_in(&known, name));
    for name in looked_for {
        assert!(!is_in(&shown, name), "{name}");
    }
}

#[test]
fn a_text_module_is_named_after_its_file_and_the_name_stays_without_identifiers() {
    // The core names the module of a .bas, .cls or .frm file after the file
    // (without its extension), and the table of SPEC section 8 keeps the names
    // of the modules unless --redact-identifiers hashes them. A point for the
    // owner in ADR-0007, which this test pins until it is decided.
    let dir = scratch("redact_text_module");
    put(&dir, "tree/clients/acme/Q3 budget.bas", MODULE);
    for (out, extra, name_stays) in [
        ("names", &["--redact"][..], true),
        (
            "identifiers",
            &["--redact", "--redact-identifiers"][..],
            false,
        ),
    ] {
        scan_tree(&dir, out, extra);

        let names = project_names(&read_report(&dir.join(out)));

        assert!(!names.is_empty(), "{extra:?}: the scan found no module");
        assert_eq!(
            names.iter().any(|name| name == "Q3 budget"),
            name_stays,
            "{extra:?}: {names:?}"
        );
    }
}

#[test]
fn a_redacted_report_with_hostile_names_shows_none_of_them_in_any_format() {
    // In the fixture, these words are in a path, a name of module, a snippet
    // and a message of error: what a redaction hashes or removes. The
    // references, the calls to the outside and the names of the APIs stay, and
    // the renderers escape them (tests/render_html.rs), so no word is theirs.
    let [formula, markup, link] = ["HYPERLINK", "svg onload", "javascript:alert"];
    let messages = messages();
    let documents_of = |report: &Report| -> Vec<(&'static str, String)> {
        let documents: Vec<_> = [Format::Json, Format::Html, Format::Csv]
            .into_iter()
            .flat_map(|format| reports::documents(format, report, &messages).expect("documents"))
            .map(|document| (document.name, text(&document.contents)))
            .collect();
        // A redacted report is still a report.
        for (name, content) in &documents {
            if name.ends_with(".json") {
                assert_eq!(report_problems(content), Vec::<String>::new(), "{name}");
            }
        }
        documents
    };
    let shown = |documents: &[(&str, String)], word: &str| {
        documents.iter().any(|(_, content)| content.contains(word))
    };
    let injection = common::injection();

    // The words are in the report to start with.
    let clear = documents_of(&injection);
    for word in [formula, markup, link] {
        assert!(shown(&clear, word), "{word}");
    }

    // Without --redact-identifiers the names of the project stay, the
    // snippets are gone.
    let names_stay = documents_of(&after(&redact_with_key(false), &injection));
    assert!(shown(&names_stay, formula));
    assert!(!shown(&names_stay, markup));
    assert!(!shown(&names_stay, link));

    // With it, nothing is left of the three, in any report.
    let hidden = documents_of(&after(&redact_with_key(true), &injection));
    assert_eq!(hidden.len(), EVERY_REPORT.len());
    for word in [formula, markup, link] {
        assert!(!shown(&hidden, word), "{word}");
    }
}
