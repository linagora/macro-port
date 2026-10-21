// SPDX-License-Identifier: Apache-2.0
//! What the front end reports for the corpus, through `analyze()`: the text
//! samples, and the workbooks and the exports of their modules that Excel made
//! of them. That every file of the corpus is read without a panic is
//! `robustness.rs`.

use std::collections::BTreeMap;

use macroport_core::analyze::analyze_modules;
use macroport_core::report::{Container, FileReport, Metrics, ProcedureReport, Status};
use macroport_core::vba::ModuleSource;
use macroport_testkit::corpus;

use crate::support::{
    checked, codes, corpus_report, marks_recovery, module, modules, options, procedures,
    sample_text, samples,
};

/// The name of the file at `path` without its directories and its extension.
fn stem(path: &str) -> &str {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.rsplit_once('.').map_or(name, |(stem, _)| stem)
}

/// The id of the sample at `path`, as `09-03`.
fn sample_id(path: &str) -> &str {
    let stem = stem(path);
    stem.get(..5).unwrap_or(stem)
}

#[test]
fn a_procedure_whose_blocks_do_not_match_gets_the_trait_in_its_sample_and_in_its_fixtures() {
    // See `vba_metrics_corpus.rs`: what makes their blocks not match.
    for (family, sample, extension, name) in [
        (
            "09-evenements",
            "09-03-horodatage-statut",
            "cls",
            "Worksheet_Change",
        ),
        (
            "13-api-windows-activex",
            "13-05-getobject-findwindow-sendmessage",
            "bas",
            "GetExcel",
        ),
    ] {
        let mut paths = vec![format!("families/{family}/{sample}.{extension}")];
        for format in ["xls", "xlsb", "xlsm"] {
            paths.push(format!("fixtures/generated/{sample}.{format}"));
            paths.push(format!(
                "fixtures/generated/{sample}.{format}.expected.{extension}"
            ));
        }

        for path in paths {
            let file = corpus_report(&path);

            let found: Vec<_> = procedures(&file)
                .filter(|procedure| marks_recovery(procedure.traits.as_deref()))
                .map(|procedure| procedure.name.as_str())
                .collect();
            assert_eq!(found, [name], "{path}");
            assert!(marks_recovery(file.traits.as_deref()), "{path}");
        }
    }
}

#[test]
fn a_sample_has_no_macros_only_when_it_has_no_procedure() {
    let mut without = Vec::new();
    for path in samples() {
        let file = corpus_report(&path);

        assert_eq!(file.container, Container::TextModule, "{path}");
        assert_eq!(codes(&file), Vec::<&str>::new(), "{path}");
        let has_procedure = procedures(&file).next().is_some();
        let status = if has_procedure {
            Status::Ok
        } else {
            Status::NoMacros
        };
        assert_eq!(file.status, status, "{path}");
        if !has_procedure {
            without.push(sample_id(&path).to_owned());
        }
    }
    // Loose statements or declarations: no `Sub` or `Function` in them.
    assert_eq!(without, ["02-01", "07-02", "11-01", "13-01", "13-02"]);
}

#[test]
fn the_trait_of_a_parse_that_recovered_goes_to_the_expected_samples_only() {
    let mut marked = Vec::new();
    for path in samples() {
        let file = corpus_report(&path);

        let file_marked = marks_recovery(file.traits.as_deref());
        let procedure_marked = procedures(&file).any(|p| marks_recovery(p.traits.as_deref()));
        // The trait of a procedure comes with the trait of its file.
        assert!(file_marked || !procedure_marked, "{path}");
        if file_marked {
            marked.push(sample_id(&path).to_owned());
        }
    }
    // 01-06, 05-04, 06-02 and 13-05: a statement that cannot be read (see
    // `vba_parse_corpus.rs`). 09-03 and 13-05: blocks that do not match (see
    // `vba_metrics_corpus.rs`). 10-01 stops before the `End Sub` of its only
    // procedure.
    assert_eq!(
        marked,
        ["01-06", "05-04", "06-02", "09-03", "10-01", "13-05"]
    );
}

/// The samples with a compile error on purpose (ACCEPTANCE A2), and whether the
/// parser marks them. It marks 01-06, 05-04 and 06-02 for a statement that it
/// cannot read, and 09-03 for blocks that do not match. It reads no such
/// statement in 02-04, 08-04 and 08-05, and leaves them unmarked on purpose
/// (see `vba_parse_corpus.rs`).
const COMPILE_ERRORS: [(&str, bool); 7] = [
    ("01-06", true),
    ("02-04", false),
    ("05-04", true),
    ("06-02", true),
    ("08-04", false),
    ("08-05", false),
    ("09-03", true),
];

#[test]
fn the_samples_with_a_compile_error_on_purpose_are_read_in_full() {
    let mut read = Vec::new();
    for path in samples() {
        let id = sample_id(&path);
        let Some((_, marked)) = COMPILE_ERRORS.iter().find(|(wanted, _)| *wanted == id) else {
            continue;
        };
        let file = corpus_report(&path);

        // The error does not fail the module: the file is read as any other.
        assert_eq!(file.status, Status::Ok, "{path}");
        assert_eq!(codes(&file), Vec::<&str>::new(), "{path}");
        assert!(procedures(&file).next().is_some(), "{path}");
        assert_eq!(marks_recovery(file.traits.as_deref()), *marked, "{path}");
        read.push(id.to_owned());
    }
    assert_eq!(read, COMPILE_ERRORS.map(|(id, _)| id));
}

#[test]
fn the_procedures_of_a_sample_lie_in_its_text() {
    for path in samples() {
        let bytes = corpus::read(&path);
        let text = String::from_utf8_lossy(&bytes);
        // The samples end their lines with LF or CR LF.
        let lines: Vec<_> = text.lines().collect();
        let file = corpus_report(&path);

        for module in modules(&file) {
            assert_eq!(module.metrics.loc as usize, lines.len(), "{path}");
            let mut before = 0;
            for procedure in &module.procedures {
                let at = format!("{path}: {}", procedure.name);
                let (start, end) = (procedure.start_line, procedure.end_line);
                // The span lies in the module, after the procedure before it,
                // and the line where it starts names the procedure.
                assert!(before < start && start <= end, "{at}");
                assert!(end <= module.metrics.loc, "{at}");
                before = end;
                let first = lines.get(start as usize - 1).expect("a line of the module");
                assert!(
                    first
                        .to_lowercase()
                        .contains(&procedure.name.to_lowercase()),
                    "{at}: {first}"
                );
                assert_eq!(procedure.metrics.loc, end - start + 1, "{at}");
            }
        }
    }
}

/// The metrics that a module or a file adds up from its procedures.
#[derive(Debug, PartialEq)]
struct Sums {
    procedures: Option<u32>,
    cyclomatic: Option<u32>,
    cyclomatic_total: Option<u32>,
    max_nesting: Option<u32>,
}

impl Sums {
    /// What `metrics` report.
    fn reported(metrics: &Metrics) -> Self {
        Self {
            procedures: metrics.procedures,
            cyclomatic: metrics.cyclomatic,
            cyclomatic_total: metrics.cyclomatic_total,
            max_nesting: metrics.max_nesting,
        }
    }

    /// What the metrics of `procedures` add up to: how many there are, the
    /// highest complexity, the sum of the complexities and the deepest nesting.
    fn of<'a>(procedures: impl Iterator<Item = &'a ProcedureReport>) -> Self {
        let (mut count, mut highest, mut total, mut deepest) = (0, 0, 0, 0);
        for procedure in procedures {
            let cyclomatic = procedure.metrics.cyclomatic.expect("a complexity");
            count += 1;
            highest = highest.max(cyclomatic);
            total += cyclomatic;
            deepest = deepest.max(procedure.metrics.max_nesting.expect("a nesting"));
        }
        Self {
            procedures: Some(count),
            cyclomatic: Some(highest),
            cyclomatic_total: Some(total),
            max_nesting: Some(deepest),
        }
    }
}

#[test]
fn the_metrics_of_a_sample_add_up_from_its_procedures() {
    for path in samples() {
        let file = corpus_report(&path);

        let mut file_loc = 0;
        for module in modules(&file) {
            file_loc += module.metrics.loc;
            let sums = Sums::of(module.procedures.iter());
            assert_eq!(Sums::reported(&module.metrics), sums, "{path}");
            for procedure in &module.procedures {
                let at = format!("{path}: {}", procedure.name);
                // Its header is a logical line. A `:` gives a physical line
                // several (`Case 1: x = 1`), so `loc` does not bound them.
                assert!(procedure.metrics.logical_loc >= 1, "{at}");
                assert!(procedure.metrics.cyclomatic >= Some(1), "{at}");
            }
        }
        let metrics = file.metrics.as_ref().expect("the metrics of the file");
        assert_eq!(metrics.loc, file_loc, "{path}");
        assert_eq!(
            Sums::reported(metrics),
            Sums::of(procedures(&file)),
            "{path}"
        );
    }
}

/// What a procedure reports whatever the container its module came from: all
/// but its lines, which the header of an export and the `Attribute` lines move.
fn fingerprint(procedure: &ProcedureReport) -> String {
    let metrics = &procedure.metrics;
    format!(
        "{} {:?} {:?} loc={} logical_loc={} cyclomatic={:?} max_nesting={:?} \
         parameters={:?} goto_count={:?} traits={:?}",
        procedure.name,
        procedure.kind,
        procedure.visibility,
        metrics.loc,
        metrics.logical_loc,
        metrics.cyclomatic,
        metrics.max_nesting,
        metrics.parameters,
        metrics.goto_count,
        procedure.traits,
    )
}

/// What a file reports whatever the container of its modules, but for what
/// depends on their lines.
#[derive(Debug, PartialEq)]
struct Reading<'a> {
    status: Status,
    traits: Option<&'a [String]>,
    /// The fingerprints of its procedures, sorted.
    procedures: Vec<String>,
    /// How many procedures, the highest complexity, their sum, the deepest
    /// nesting, the declares and the external dependencies.
    counts: [Option<u32>; 6],
}

fn reading(file: &FileReport) -> Reading<'_> {
    let mut found: Vec<_> = procedures(file).map(fingerprint).collect();
    found.sort();
    let metrics = file.metrics.as_ref().expect("the metrics of the file");
    Reading {
        status: file.status,
        traits: file.traits.as_deref(),
        procedures: found,
        counts: [
            metrics.procedures,
            metrics.cyclomatic,
            metrics.cyclomatic_total,
            metrics.max_nesting,
            metrics.declare_count,
            metrics.external_dependencies,
        ],
    }
}

#[test]
fn a_sample_and_the_workbooks_built_from_it_give_the_same_reading() {
    let texts: BTreeMap<_, _> = (samples().iter())
        .map(|path| (stem(path).to_owned(), corpus_report(path)))
        .collect();
    let mut compared = 0;
    for workbook in corpus::files("fixtures/generated", &["xls", "xlsb", "xlsm"]) {
        // The encrypted, XLM and macro-less workbooks come from no sample.
        let Some(text) = texts.get(stem(&workbook)) else {
            continue;
        };

        let built = corpus_report(&workbook);

        assert_eq!(reading(&built), reading(text), "{workbook}");
        compared += 1;
    }
    // Nine of the 240 workbooks (80 samples in three formats) are not built yet.
    assert!(compared >= 231, "{compared} workbooks");
}

#[test]
fn analyze_modules_gives_what_analyze_gives_for_the_text_of_a_sample() {
    for path in samples() {
        let by_file = corpus_report(&path);
        let only = module(&by_file);
        let source = ModuleSource {
            name: only.name.clone(),
            module_type: only.module_type,
            text: sample_text(&path),
        };

        let by_modules = checked(analyze_modules(vec![source], &path, &options()));

        assert_eq!(by_modules, by_file, "{path}");
    }
}
