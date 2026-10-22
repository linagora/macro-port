// SPDX-License-Identifier: Apache-2.0
//! The structural parser on the whole corpus (SPEC M2: no panic on any corpus
//! file, the deliberate compile errors included): the text samples and Excel's
//! own exports of the binary fixtures. The spans and the declarations are
//! checked against the physical lines of the text, read by a second, much
//! cruder reader.

// clippy.toml excuses `expect` in the `#[test]` functions only, not in helpers.
#![allow(clippy::expect_used)]

mod vba_parse_common;

use std::collections::BTreeSet;
use std::sync::LazyLock;

use macroport_core::report::ProcedureKind::{self, Function, Sub};
use macroport_core::vba::{DeclarationKind, Procedure, RecoveryKind, SkipKind, parse};
use macroport_testkit::corpus;
use regex::{Captures, Regex};
use vba_parse_common::{lines_of, module_of, spans};

/// The text of the corpus file `path`.
fn text_of(path: &str) -> String {
    String::from_utf8_lossy(&corpus::read(path)).into_owned()
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
            let text = text_of(&path);
            (path, text)
        })
        .collect()
}

/// Whether the parser closed the procedure where its code stops, for lack of
/// an end.
fn misses_end(procedure: &Procedure) -> bool {
    procedure
        .recoveries
        .iter()
        .any(|r| r.kind == RecoveryKind::MissingEnd)
}

/// The physical line `number`, counted from 1.
fn physical_line<'a>(physical: &[&'a str], number: u32) -> &'a str {
    let index = usize::try_from(number).ok().and_then(|n| n.checked_sub(1));
    index.and_then(|n| physical.get(n)).copied().unwrap_or("")
}

/// The physical lines of a text that the crude readers look at, as their
/// number and their code, trimmed: no comment, no continuation line. A line
/// that ends with ` _` goes on over the next one, a comment too.
fn crude_lines(text: &str) -> impl Iterator<Item = (u32, &str)> {
    let mut continued = false;
    (1u32..)
        .zip(text.lines())
        .filter_map(move |(number, line)| {
            let code = line.trim();
            let skipped = continued || code.starts_with('\'');
            continued = code.ends_with(" _");
            (!skipped).then_some((number, code))
        })
}

static PROCEDURE_HEADER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^\s*((public|private|friend|static)\s+)*(sub|function|property\s+(get|let|set))\s+\[?\w",
    )
    .expect("a regular expression")
});

static PROCEDURE_END: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^\s*end\s+(sub|function|property)\b").expect("a regular expression")
});

/// The headers and the ends of procedures that a regular expression finds in
/// the physical lines of a text, [`crude_lines`] only. A header that stands
/// after a `:` is not found, nor is an end.
struct CrudeProcedures {
    headers: BTreeSet<u32>,
    ends: BTreeSet<u32>,
}

fn crude_procedures(text: &str) -> CrudeProcedures {
    let mut found = CrudeProcedures {
        headers: BTreeSet::new(),
        ends: BTreeSet::new(),
    };
    for (number, code) in crude_lines(text) {
        if PROCEDURE_HEADER.is_match(code) {
            found.headers.insert(number);
        }
        if PROCEDURE_END.is_match(code) {
            found.ends.insert(number);
        }
    }
    found
}

#[test]
fn the_physical_lines_of_every_module_are_the_lines_of_its_text() {
    // The crude readers of this file count the lines as `str::lines` does.
    for (path, text) in modules() {
        let counted = usize::try_from(lines_of(&text).physical_lines).ok();
        assert_eq!(counted, Some(text.lines().count()), "{path}");
    }
}

#[test]
fn the_procedures_of_every_module_are_in_order_and_inside_it() {
    let mut procedures = 0usize;
    for (path, text) in modules() {
        let lines = lines_of(&text);
        let module = parse(&lines);
        procedures = procedures.saturating_add(module.procedures.len());
        let mut next = 0usize;
        for p in &module.procedures {
            let name = format!("{path}: {}", p.name);
            assert!(
                next <= p.lines.start && p.lines.start < p.lines.end,
                "{name}: {:?} after line {next}",
                p.lines
            );
            assert!(p.lines.end <= lines.logical.len(), "{name}: {:?}", p.lines);
            next = p.lines.end;
            assert!(
                1 <= p.start_line && p.start_line <= p.end_line,
                "{name}: {}..{}",
                p.start_line,
                p.end_line
            );
            assert!(p.end_line <= lines.physical_lines, "{name}");
            let first = &lines.logical[p.lines.start];
            let last = &lines.logical[p.lines.end.saturating_sub(1)];
            assert!(
                first.line <= p.start_line && p.start_line <= first.end_line,
                "{name}"
            );
            assert!(
                last.line <= p.end_line && p.end_line <= last.end_line,
                "{name}"
            );
        }
        assert!(
            module.recoveries.is_empty(),
            "{path}: {:?}",
            module.recoveries
        );
    }
    assert!(procedures > 1350, "{procedures} procedures");
}

#[test]
fn every_span_of_the_corpus_starts_at_a_header_and_ends_at_an_end() {
    for (path, text) in modules() {
        let module = module_of(&text);
        let found = crude_procedures(&text);
        for p in &module.procedures {
            let name = format!("{path}: {}", p.name);
            assert!(
                found.headers.contains(&p.start_line),
                "{name}: header at {}",
                p.start_line
            );
            assert_eq!(
                found.ends.contains(&p.end_line),
                !misses_end(p),
                "{name}: end at {}",
                p.end_line
            );
        }
        // Every end the crude reader finds closes a procedure.
        let ends: BTreeSet<u32> = module.procedures.iter().map(|p| p.end_line).collect();
        assert!(
            found.ends.is_subset(&ends),
            "{path}: {:?}",
            found.ends.difference(&ends)
        );
    }
}

#[test]
fn the_headers_that_do_not_start_a_procedure_are_the_second_forms_of_a_procedure() {
    let mut second_forms = Vec::new();
    for (path, text) in modules() {
        let module = module_of(&text);
        let found = crude_procedures(&text);
        let physical: Vec<&str> = text.lines().collect();
        let starts: BTreeSet<u32> = module.procedures.iter().map(|p| p.start_line).collect();
        for line in found.headers.difference(&starts) {
            // It stands inside a procedure, and the code before it is an `#Else`
            // or an `#ElseIf`.
            let inside = module
                .procedures
                .iter()
                .any(|p| p.start_line < *line && *line <= p.end_line);
            assert!(inside, "{path}:{line}");
            let before = (1..*line)
                .rev()
                .map(|number| physical_line(&physical, number).trim())
                .find(|code| !code.is_empty() && !code.starts_with('\''));
            assert!(
                before.is_some_and(|code| code.to_ascii_lowercase().starts_with("#else")),
                "{path}:{line}: {before:?}"
            );
            if path.starts_with("families/") {
                second_forms.push(format!("{path}:{line}"));
            }
        }
    }
    // A header under `#If`, a second one under `#Else`, `#End If`, and the body
    // that the two forms share. The second form of 10-06 drops a parameter
    // that the first has (`#If Mac Then`), the ones of 13-06 differ by
    // `LongPtr` and `Long` (`#If VBA7 Then`).
    let expected = [
        "10-fichiers-systeme/10-06-libfiletools-bibliotheque.bas:698",
        "13-api-windows-activex/13-06-stdclipboard-api-presse-papiers.cls:624",
        "13-api-windows-activex/13-06-stdclipboard-api-presse-papiers.cls:635",
        "13-api-windows-activex/13-06-stdclipboard-api-presse-papiers.cls:716",
        "13-api-windows-activex/13-06-stdclipboard-api-presse-papiers.cls:735",
        "13-api-windows-activex/13-06-stdclipboard-api-presse-papiers.cls:751",
        "13-api-windows-activex/13-06-stdclipboard-api-presse-papiers.cls:866",
        "13-api-windows-activex/13-06-stdclipboard-api-presse-papiers.cls:982",
    ];
    assert_eq!(
        second_forms,
        expected.map(|form| format!("families/{form}"))
    );
}

#[test]
fn the_only_procedures_of_the_corpus_that_miss_their_end_are_in_a_cut_sample() {
    let mut missing = Vec::new();
    for (path, text) in modules() {
        for p in module_of(&text).procedures.iter().filter(|p| misses_end(p)) {
            missing.push(format!(
                "{path}: {} {}..{}",
                p.name, p.start_line, p.end_line
            ));
        }
    }
    // The procedure stops at its last line of code: 28 in the sample, which is
    // cut after `Application.ScreenUpdating = True`. Excel's exports of it, one
    // for each format, are the same text with an `Attribute` line before it.
    let mut expected = vec![
        "families/10-fichiers-systeme/10-01-boucle-dir-classeurs-dossier.bas: LoopThroughFiles 7..28"
            .to_owned(),
    ];
    expected.extend(["xls", "xlsb", "xlsm"].map(|format| {
        format!(
            "fixtures/generated/10-01-boucle-dir-classeurs-dossier.{format}.expected.bas: LoopThroughFiles 2..23"
        )
    }));
    assert_eq!(missing, expected);
}

/// How many parameters the header `line` declares, counted by hand: the items
/// between its first `(` and the `)` that matches, cut at the commas outside
/// any parenthesis and string. `None` when the header goes on over the next
/// physical line.
fn crude_parameter_count(line: &str) -> Option<usize> {
    let Some(open) = line.find('(') else {
        return (!line.trim_end().ends_with(" _")).then_some(0);
    };
    let (mut depth, mut in_string, mut item, mut count) = (0usize, false, false, 0usize);
    for c in line.get(open..)?.chars() {
        match c {
            '"' => {
                in_string = !in_string;
                item = true;
            }
            _ if in_string => {}
            '(' => {
                depth = depth.saturating_add(1);
                item |= depth > 1;
            }
            ')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(count.saturating_add(usize::from(item)));
                }
                item = true;
            }
            ',' if depth == 1 => {
                count = count.saturating_add(usize::from(item));
                item = false;
            }
            c if !c.is_whitespace() => item = true,
            _ => {}
        }
    }
    None
}

#[test]
fn the_parameters_of_every_header_that_fits_a_line_are_counted_as_by_hand() {
    let mut checked = 0usize;
    for (path, text) in modules() {
        let physical: Vec<&str> = text.lines().collect();
        for p in &module_of(&text).procedures {
            let header = physical_line(&physical, p.start_line);
            if let Some(count) = crude_parameter_count(header) {
                assert_eq!(
                    p.parameter_count, count,
                    "{path}:{}: {header}",
                    p.start_line
                );
                checked = checked.saturating_add(1);
            }
        }
    }
    assert!(checked > 1150, "{checked}");
}

/// A sample with an error on purpose (ACCEPTANCE A2), as found by hand in its
/// text.
struct Deliberate {
    /// Its path in `corpus/families`.
    path: &'static str,
    /// The name, the kind, the first and the last line of each procedure.
    procedures: &'static [(&'static str, ProcedureKind, u32, u32)],
    /// The procedures in which the parser steps over something.
    recovered: &'static [&'static str],
    /// The declarations outside the procedures, in the words of [`describe`],
    /// and their lines. The `Dim` of a procedure declares nothing at module
    /// level.
    declarations: &'static [(&'static str, u32)],
}

/// The seven samples. The parser steps over a statement it cannot read: the
/// line of prose in 01-06 and 06-02, which the manifests call a syntax error,
/// and the continuation line of 05-04 that lost its `_`. The three samples
/// whose error is one of logic (02-04, 08-05) or has none to be seen in a
/// statement (08-04), and 09-03, whose extra `Else` and `End If` are blocks
/// that `measure_procedure` counts, read like any other.
const DELIBERATE_ERRORS: [Deliberate; 7] = [
    Deliberate {
        path: "01-mise-en-forme/01-06-edit-table-report.bas",
        procedures: &[
            ("editTable", Sub, 11, 86),
            ("SelectFirstEmptyRowInColumn", Function, 88, 93),
            ("AutoSumColumnAF", Sub, 94, 107),
            ("changeProperties", Sub, 108, 167),
            ("removeHeaderSpace", Sub, 169, 188),
            ("removeProductCodeSpace", Sub, 190, 209),
            ("SaveAsToFolderPath", Sub, 211, 230),
        ],
        recovered: &["editTable"],
        declarations: &[("option explicit", 8), ("variable", 9)],
    },
    Deliberate {
        path: "02-consolidation/02-04-consolidating-workbooks-filedialog.bas",
        procedures: &[("ConsolidatingMultipleWb", Sub, 9, 71)],
        recovered: &[],
        declarations: &[("option explicit", 8)],
    },
    Deliberate {
        path: "05-formulaires-saisie/05-04-formulaire-commercial-region.bas",
        procedures: &[
            ("cmdCancel_Click", Sub, 13, 16),
            ("cmdOK_Click", Sub, 18, 24),
            ("UserForm_Initialize", Sub, 26, 28),
            ("LaunchSalesPersonForm", Sub, 31, 40),
        ],
        recovered: &["LaunchSalesPersonForm"],
        declarations: &[("variable", 8), ("variable", 9), ("variable", 10)],
    },
    Deliberate {
        path: "06-generation-rapports/06-02-mise-en-page-impression-rapport.bas",
        procedures: &[
            ("editReportNew", Sub, 9, 75),
            ("editingProperties", Sub, 77, 117),
        ],
        recovered: &["editReportNew"],
        declarations: &[("option explicit", 8)],
    },
    Deliberate {
        path: "08-fonctions-personnalisees/08-04-udf-somme-cellules-visibles.bas",
        procedures: &[("SumOnlyVisible", Function, 7, 19)],
        recovered: &[],
        declarations: &[],
    },
    Deliberate {
        path: "08-fonctions-personnalisees/08-05-udf-montant-en-lettres.bas",
        procedures: &[("spellNumberAsCurrency", Function, 17, 99)],
        recovered: &[],
        declarations: &[],
    },
    Deliberate {
        path: "09-evenements/09-03-horodatage-statut.cls",
        procedures: &[("Worksheet_Change", Sub, 9, 32)],
        recovered: &[],
        declarations: &[],
    },
];

#[test]
fn a_deliberate_compile_error_never_fails_the_module_and_flags_only_what_cannot_be_read() {
    for sample in DELIBERATE_ERRORS {
        let path = format!("families/{}", sample.path);
        let module = module_of(&text_of(&path));
        assert_eq!(spans(&module), sample.procedures, "{path}");
        let recovered: Vec<_> = module
            .procedures
            .iter()
            .filter(|p| p.is_recovered())
            .map(|p| p.name.as_str())
            .collect();
        assert_eq!(recovered, sample.recovered, "{path}");
        assert!(module.recoveries.is_empty(), "{path}");
        let declarations: Vec<_> = module
            .declarations
            .iter()
            .map(|d| (describe(&d.kind), d.line))
            .collect();
        let expected: Vec<_> = sample
            .declarations
            .iter()
            .map(|&(words, line)| (words.to_owned(), line))
            .collect();
        assert_eq!(declarations, expected, "{path}");
    }
}

/// A statement of the corpus that cannot be read, found by hand.
struct Unreadable {
    /// The sample, without its extension, in `corpus/families`.
    sample: &'static str,
    /// The procedure that holds the statement.
    procedure: &'static str,
    /// Its line in the sample, and in Excel's exports of it.
    line: u32,
    exported_line: u32,
    /// What the line says.
    says: &'static str,
}

const UNREADABLE: [Unreadable; 4] = [
    Unreadable {
        sample: "01-mise-en-forme/01-06-edit-table-report",
        procedure: "editTable",
        line: 13,
        exported_line: 8,
        says: "Please note: you need to import ConfigProgressCode module and ProgressDialogue form in order to work",
    },
    Unreadable {
        sample: "05-formulaires-saisie/05-04-formulaire-commercial-region",
        procedure: "LaunchSalesPersonForm",
        line: 36,
        exported_line: 31,
        says: "MsgBox \"The Salesperson's ID is: \" &",
    },
    Unreadable {
        sample: "06-generation-rapports/06-02-mise-en-page-impression-rapport",
        procedure: "editReportNew",
        line: 11,
        exported_line: 6,
        says: "Please note: you need to import ConfigProgressCode module and ProgressDialogue form in order to work",
    },
    Unreadable {
        sample: "13-api-windows-activex/13-05-getobject-findwindow-sendmessage",
        procedure: "GetExcel",
        line: 43,
        exported_line: 38,
        says: "Do manipulations of your  file here.",
    },
];

#[test]
fn the_only_statements_of_the_corpus_that_cannot_be_read_are_the_four_listed() {
    // In the samples and in the three exports of each, in no other file, and
    // all in a procedure.
    let mut found = Vec::new();
    for (path, text) in modules() {
        let module = module_of(&text);
        let in_module = module.recoveries.iter().map(|r| (r, "<module>"));
        let in_procedures = module
            .procedures
            .iter()
            .flat_map(|p| p.recoveries.iter().map(|r| (r, p.name.as_str())));
        for (recovery, holder) in in_module.chain(in_procedures) {
            if recovery.kind == RecoveryKind::Unparseable {
                found.push(format!("{path}:{}: {holder}", recovery.line));
            }
        }
    }
    let mut expected = Vec::new();
    for sample in UNREADABLE {
        let Unreadable {
            procedure,
            line,
            exported_line,
            says,
            ..
        } = sample;
        let stem = sample.sample.rsplit('/').next().unwrap_or_default();
        let sample_path = format!("families/{}.bas", sample.sample);
        let exports = ["xls", "xlsb", "xlsm"]
            .map(|format| format!("fixtures/generated/{stem}.{format}.expected.bas"));
        expected.push(format!("{sample_path}:{line}: {procedure}"));
        for export in &exports {
            expected.push(format!("{export}:{exported_line}: {procedure}"));
        }
        // The line is the one of the statement, in the sample and in the exports.
        for (path, at) in
            std::iter::once((&sample_path, line)).chain(exports.iter().map(|e| (e, exported_line)))
        {
            let text = text_of(path);
            let physical: Vec<&str> = text.lines().collect();
            let said = physical_line(&physical, at);
            assert!(said.contains(says), "{path}:{at}: {said}");
        }
    }
    found.sort();
    expected.sort();
    assert_eq!(found, expected);
}

#[test]
fn the_physical_line_numbers_count_the_headers_and_attributes_of_the_export() {
    // The spans of a module that starts with `Attribute` lines are the
    // physical lines of the text (SPEC section 5), not the ones of the code.
    let mut checked = 0usize;
    for (path, text) in modules() {
        let lines = lines_of(&text);
        if !lines.skipped.iter().any(|s| s.kind == SkipKind::Attribute) {
            continue;
        }
        let physical: Vec<&str> = text.lines().collect();
        for p in &parse(&lines).procedures {
            let header = physical_line(&physical, p.start_line);
            assert!(
                header
                    .to_ascii_lowercase()
                    .contains(&p.name.to_ascii_lowercase()),
                "{path}: {} at {}: {header}",
                p.name,
                p.start_line
            );
            checked = checked.saturating_add(1);
        }
    }
    assert!(checked > 1300, "{checked}");
}

static OPTION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^option\s+(explicit|base|compare|vbasupport|private\s+module)\b")
        .expect("a regular expression")
});

static DECLARE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(?:(?:public|private)\s+)?declare\s+(?:ptrsafe\s+)?(function|sub)\s+(\w+)")
        .expect("a regular expression")
});

static WITH_EVENTS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(?:(?:public|private|dim)\s+)+withevents\s+(\w+)")
        .expect("a regular expression")
});

static KEYWORD_DECLARATION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^(?:(?:(?:public|private|global)\s+)?(const|type|enum|event)|(implements))\s+\w",
    )
    .expect("a regular expression")
});

static VARIABLE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(?:public|private|dim|global)\s+(\[?\w+)").expect("a regular expression")
});

static DIRECTIVE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^#\s*(if|elseif|else|end\s+if|const)\b").expect("a regular expression")
});

static BLOCK_END: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^end\s+(type|enum)\b").expect("a regular expression"));

/// The text of the group `index` of a match.
fn group<'a>(found: &Captures<'a>, index: usize) -> &'a str {
    found.get(index).map_or("", |group| group.as_str())
}

/// The declaration that the code of a physical line makes, if it makes one, in
/// the words of [`describe`].
fn crude_tag(code: &str) -> Option<String> {
    let squeezed = |words: &str| words.split_whitespace().collect::<String>();
    if let Some(found) = OPTION.captures(code) {
        return Some(format!("option {}", squeezed(group(&found, 1))));
    }
    if let Some(found) = DECLARE.captures(code) {
        return Some(format!("declare {} {}", group(&found, 1), group(&found, 2)));
    }
    if let Some(found) = WITH_EVENTS.captures(code) {
        return Some(format!("withevents {}", group(&found, 1)));
    }
    if let Some(found) = KEYWORD_DECLARATION.captures(code) {
        let word = group(&found, 1);
        return Some(
            if word.is_empty() {
                group(&found, 2)
            } else {
                word
            }
            .to_owned(),
        );
    }
    if let Some(found) = VARIABLE.captures(code) {
        // The headers of procedures come with a scope too.
        let word = group(&found, 1).to_ascii_lowercase();
        let header = ["sub", "function", "property", "static", "friend"];
        return (!header.contains(&word.as_str())).then(|| "variable".to_owned());
    }
    let found = DIRECTIVE.captures(code)?;
    Some(format!("directive({})", squeezed(group(&found, 1))))
}

/// The declarations that regular expressions find in the physical lines of a
/// text, [`crude_lines`] only, outside the procedures `inside` (the first and
/// the last line of each), as their words and their lines. They read nothing
/// after a `:`, and skip the members of a `Type` or an `Enum`.
fn crude_declarations(text: &str, inside: &[(u32, u32)]) -> Vec<(String, u32)> {
    let mut found = Vec::new();
    let mut in_block = false;
    for (number, code) in crude_lines(text) {
        let in_procedure = inside
            .iter()
            .any(|&(first, last)| (first..=last).contains(&number));
        if in_procedure {
            continue;
        }
        if in_block {
            in_block = !BLOCK_END.is_match(code);
        } else if let Some(tag) = crude_tag(code) {
            let tag = tag.to_ascii_lowercase();
            in_block = tag == "type" || tag == "enum";
            found.push((tag, number));
        }
    }
    found
}

/// A declaration in the words of the crude reader: in lowercase, with a name
/// for a `Declare` and for a variable declared `WithEvents`.
fn describe(kind: &DeclarationKind) -> String {
    let words = match kind {
        DeclarationKind::Option(option) => format!("option {option:?}"),
        DeclarationKind::Declare { name, is_function } => {
            let routine = if *is_function { "function" } else { "sub" };
            format!("declare {routine} {name}")
        }
        DeclarationKind::WithEvents { name } => format!("withevents {name}"),
        other => format!("{other:?}"),
    };
    words.to_ascii_lowercase()
}

#[test]
fn the_declarations_of_every_module_are_the_ones_a_crude_reader_finds_outside_its_procedures() {
    let mut compared = 0usize;
    for (path, text) in modules() {
        let module = module_of(&text);
        let inside: Vec<_> = module
            .procedures
            .iter()
            .map(|p| (p.start_line, p.end_line))
            .collect();
        let parsed: Vec<_> = module
            .declarations
            .iter()
            .map(|d| (describe(&d.kind), d.line))
            .collect();
        let crude = crude_declarations(&text, &inside);
        assert_eq!(parsed, crude, "{path}");
        compared = compared.saturating_add(crude.len());
    }
    assert!(compared > 1000, "{compared} declarations");
}

#[test]
fn the_declarations_never_overlap_the_procedures_and_a_block_runs_to_its_end() {
    for (path, text) in modules() {
        let lines = lines_of(&text);
        let module = parse(&lines);
        let mut ranges: Vec<_> = module
            .procedures
            .iter()
            .map(|p| p.lines.clone())
            .chain(module.declarations.iter().map(|d| d.lines.clone()))
            .collect();
        ranges.sort_by_key(|range| range.start);
        for pair in ranges.windows(2) {
            assert!(pair[0].end <= pair[1].start, "{path}: {pair:?}");
        }
        for d in &module.declarations {
            let first = &lines.logical[d.lines.start];
            let last = &lines.logical[d.lines.end.saturating_sub(1)];
            assert!(
                first.line <= d.line && d.line <= first.end_line,
                "{path}:{}",
                d.line
            );
            if matches!(d.kind, DeclarationKind::Type | DeclarationKind::Enum) {
                let end = format!("end {}", describe(&d.kind));
                assert!(
                    last.text
                        .trim_start()
                        .to_ascii_lowercase()
                        .starts_with(&end),
                    "{path}:{}: ends at {}",
                    d.line,
                    last.line
                );
            } else {
                assert_eq!(d.lines.len(), 1, "{path}:{}", d.line);
            }
        }
    }
}

/// The text samples with at least one declaration of a kind, and how many.
fn samples_with(is_kind: fn(&DeclarationKind) -> bool) -> Vec<String> {
    let samples = modules()
        .into_iter()
        .filter(|(path, _)| path.starts_with("families/"));
    samples
        .filter_map(|(path, text)| {
            let count = module_of(&text)
                .declarations
                .iter()
                .filter(|d| is_kind(&d.kind))
                .count();
            let file = path.rsplit('/').next().unwrap_or_default();
            (count > 0).then(|| format!("{file}: {count}"))
        })
        .collect()
}

#[test]
fn the_declare_statements_and_the_variables_with_events_of_the_samples_are_all_found() {
    // Counted by hand: 116 `Declare` statements, 7 variables `WithEvents`.
    assert_eq!(
        samples_with(|kind| matches!(kind, DeclarationKind::Declare { .. })),
        [
            "10-06-libfiletools-bibliotheque.bas: 36",
            "11-05-vba-json-jsonconverter.bas: 14",
            "13-01-declare-ptrsafe-compilation-conditionnelle.bas: 6",
            "13-05-getobject-findwindow-sendmessage.bas: 2",
            "13-06-stdclipboard-api-presse-papiers.cls: 58",
        ]
    );
    assert_eq!(
        samples_with(|kind| matches!(kind, DeclarationKind::WithEvents { .. })),
        ["09-01-ecouteur-evenements-controles.cls: 7"]
    );
}
