// SPDX-License-Identifier: Apache-2.0
//! The CSV reports: their columns and rows, their quoting, and the protection
//! of their cells against formula injection (SPEC M4 and section 8).

mod common;

use ::csv::ReaderBuilder;
use macroport_cli::render::csv as render;
use macroport_core::report::{Lang, Report, Signal};
use macroport_core::rules::{RuleSet, RulesSources, Severity};

/// The characters that make a spreadsheet read a cell as a formula or a
/// command (SPEC section 8, A7).
const DANGEROUS_STARTS: [char; 6] = ['=', '+', '-', '@', '\t', '\r'];

const BYTE_ORDER_MARK: &[u8] = b"\xEF\xBB\xBF";

/// The three documents of `report`, by name.
fn documents(report: &Report) -> [(&'static str, Vec<u8>); 3] {
    [
        ("files", render::files(report).expect("files.csv")),
        (
            "procedures",
            render::procedures(report).expect("procedures.csv"),
        ),
        ("signals", render::signals(report).expect("signals.csv")),
    ]
}

/// The rows of `document`, the header included. The reader refuses a row of
/// another length than the header, and takes off the byte order mark.
fn rows(document: &[u8]) -> Vec<Vec<String>> {
    ReaderBuilder::new()
        .has_headers(false)
        .from_reader(document)
        .records()
        .map(|record| {
            let record = record.expect("a well-formed row");
            record.iter().map(str::to_owned).collect()
        })
        .collect()
}

/// The text of `document` without its byte order mark.
fn text(document: &[u8]) -> &str {
    let text = std::str::from_utf8(document).expect("UTF-8");
    text.strip_prefix('\u{feff}').expect("a byte order mark")
}

/// The lines of `document`, as they read in the file. Only for the documents
/// of the fleet, which has no cell with a line break.
fn lines(document: &[u8]) -> Vec<&str> {
    text(document).lines().collect()
}

#[test]
fn the_columns_are_those_of_the_spec() {
    let report = common::fleet();
    let [files, procedures, signals] = documents(&report).map(|(_, document)| rows(&document));

    assert_eq!(
        files[0].join(","),
        concat!(
            "path,container,status,complexity_level,primary_family,max_severity,",
            "modules,procedures,logical_loc,xlm_present,vba_project_protected"
        )
    );
    assert_eq!(
        procedures[0].join(","),
        concat!(
            "path,module,procedure,kind,primary_family,secondary_families,",
            "logical_loc,cyclomatic,signals,traits"
        )
    );
    assert_eq!(
        signals[0].join(","),
        "path,module,procedure,signal_id,severity,line"
    );
}

#[test]
fn documents_are_utf8_with_a_byte_order_mark_and_crlf_rows() {
    let report = common::fleet();

    for (name, document) in documents(&report) {
        assert!(document.starts_with(BYTE_ORDER_MARK), "{name}");
        let text = text(&document);
        assert!(text.ends_with("\r\n"), "{name}");
        // No row ends with a bare line feed: the fleet has no cell with one.
        assert!(!text.replace("\r\n", "").contains('\n'), "{name}");
        assert!(!text.contains("\r\r"), "{name}");
    }
}

#[test]
fn an_empty_report_gives_the_headers_only() {
    let rules = RuleSet::load(&RulesSources::EMBEDDED).expect("the shipped rules load");
    let report = Report::empty(&rules, "2026-10-20T08:00:00Z".to_owned(), Lang::En);

    for (name, document) in documents(&report) {
        assert!(document.starts_with(BYTE_ORDER_MARK), "{name}");
        assert_eq!(rows(&document).len(), 1, "{name}");
    }
}

#[test]
fn the_documents_of_the_fleet() {
    let [files, procedures, signals] = documents(&common::fleet()).map(|(_, document)| document);

    insta::assert_snapshot!("files", text(&files));
    insta::assert_snapshot!("procedures", text(&procedures));
    insta::assert_snapshot!("signals", text(&signals));
}

#[test]
fn every_file_has_a_row_whatever_its_status() {
    let report = common::fleet();
    let files = render::files(&report).expect("files.csv");

    let rows = rows(&files);
    let paths: Vec<_> = rows.iter().skip(1).map(|row| row[0].as_str()).collect();
    let expected: Vec<_> = report.files.iter().map(|file| file.path.as_str()).collect();
    assert_eq!(paths, expected);
    let lines = lines(&files);
    // Nothing is known of a file that could not be read: its row says so with
    // empty cells, not zeros.
    assert!(lines.contains(&"locked/payroll.xlsm,xlsm,encrypted,,,,,,,,"));
    assert!(lines.contains(&"crash/internal.xlsm,xlsm,error,,,,,,,,"));
    // A file that was read and has no VBA project has no module and no
    // procedure, which is a count and not an absence.
    assert!(lines.contains(&"blank/empty.xlsx,xlsx,no_macros,low,,,0,0,0,false,"));
    assert!(lines.contains(&"legacy/xlm-only.xlsb,xlsb,ok,low,,medium,0,0,0,true,"));
    assert!(lines.contains(&"legacy/budget.xls,xls,ok,medium,05-forms,medium,3,5,54,false,true"));
}

#[test]
fn the_procedures_of_a_file_are_those_of_its_metrics() {
    let mut report = common::fleet();
    report.files[1]
        .metrics
        .as_mut()
        .expect("metrics")
        .procedures = Some(99);
    report.files[2]
        .metrics
        .as_mut()
        .expect("metrics")
        .procedures = None;

    let files = rows(&render::files(&report).expect("files.csv"));

    // The count is the report's, not one made again from the modules.
    assert_eq!(files[2][7], "99");
    assert_eq!(files[3][7], "");
}

#[test]
fn procedures_come_only_from_the_files_with_a_vba_project() {
    let report = common::fleet();
    let procedures = render::procedures(&report).expect("procedures.csv");

    let listed: usize = report
        .files
        .iter()
        .filter_map(|file| file.vba_project.as_ref())
        .flat_map(|project| &project.modules)
        .map(|module| module.procedures.len())
        .sum();
    let lines = lines(&procedures);
    assert_eq!(lines.len() - 1, listed);
    assert_eq!(listed, 7);
    // Lists share a cell, and what a procedure does not have is empty.
    assert!(lines.contains(&concat!(
        "legacy/budget.xls,Module1,FormatReport,sub,01-formatting,06-reporting,",
        "14,4,,"
    )));
    assert!(lines.contains(&concat!(
        "legacy/budget.xls,Module1,Macro1,sub,unclassified,,",
        "6,1,,recorded_macro;possibly_unused"
    )));
    assert!(lines.contains(&concat!(
        "legacy/budget.xls,ThisWorkbook,Workbook_Open,sub,09-events,,",
        "6,2,sec.auto-exec;sec.alerts-off,"
    )));
}

#[test]
fn signals_have_a_row_for_each_line_of_evidence() {
    let report = common::fleet();
    let signals = render::signals(&report).expect("signals.csv");
    let lines = lines(&signals);

    // Two lines of evidence, two rows.
    assert!(lines.contains(&"legacy/budget.xls,ThisWorkbook,Workbook_Open,sec.alerts-off,info,9"));
    assert!(lines.contains(&"legacy/budget.xls,ThisWorkbook,Workbook_Open,sec.alerts-off,info,12"));
    // A signal of the file has no module and no procedure, one of a module no
    // procedure.
    assert!(
        lines
            .contains(&"examples/14-01-auto-open-shell.xlsm,,,sec.chain.autoexec-execution,high,6")
    );
    assert!(lines.contains(&"legacy/budget.xls,Module1,,sec.declare,low,2"));
    // A signal without evidence still has a row, with no line.
    assert!(lines.contains(&"legacy/xlm-only.xlsb,,,sec.xlm-macrosheet,medium,"));
}

#[test]
fn a_line_that_the_evidence_of_a_signal_repeats_has_one_row() {
    let mut report = common::fleet();
    let chain = &mut report.files[0].signals.as_mut().expect("file signals")[0];
    let evidence = chain.evidence.as_mut().expect("evidence");
    evidence.push(evidence[0].clone());

    let signals = render::signals(&report).expect("signals.csv");

    let rows_of_chain = lines(&signals)
        .into_iter()
        .filter(|line| line.contains(",sec.chain.autoexec-execution,"))
        .count();
    assert_eq!(rows_of_chain, 2);
}

#[test]
fn a_signal_that_names_no_module_takes_the_one_it_was_found_in() {
    let report = common::injection();
    let signals = rows(&render::signals(&report).expect("signals.csv"));

    let declare = signals
        .iter()
        .find(|row| row[3] == "sec.declare")
        .expect("a row for sec.declare");
    // The module is named by the report, not by the signal.
    assert_eq!(declare[1], "'=HYPERLINK(\"http://x\")");
    assert_eq!(declare[2], "");
}

#[test]
fn cells_are_quoted_as_rfc_4180_says() {
    let mut report = common::fleet();
    let hostile = "a,b \"c\"\nd";
    let project = report.files[1].vba_project.as_mut().expect("a VBA project");
    project.modules[1].name = hostile.to_owned();
    let procedures = render::procedures(&report).expect("procedures.csv");
    let written = text(&procedures);

    // A comma, a quote and a line feed put the cell in quotes, and a quote in
    // a cell is written twice.
    assert!(
        written.contains("legacy/budget.xls,\"a,b \"\"c\"\"\nd\",FormatReport,sub,01-formatting,"),
        "{written}"
    );
    // Read back, the cell is the one that went in.
    assert!(rows(&procedures).iter().any(|row| row[1] == hostile));
    // A cell that needs no quotes has none.
    assert!(written.contains("legacy/budget.xls,ThisWorkbook,Workbook_Open,sub,09-events,,6,2,"));
}

/// Puts `text` in every cell of `report` that the CSV reports hold and that
/// may come from an analysed file or from custom rules.
fn put_everywhere(report: &mut Report, text: &str) {
    let signal = |severity, module: Option<&str>, procedure: Option<&str>| Signal {
        signal_id: text.to_owned(),
        severity,
        module: module.map(str::to_owned),
        procedure: procedure.map(str::to_owned),
        evidence: None,
    };
    let file = &mut report.files[1];
    file.path = text.to_owned();
    file.primary_family = Some(text.to_owned());
    file.signals = Some(vec![signal(Severity::High, Some(text), Some(text))]);
    let project = file.vba_project.as_mut().expect("a VBA project");
    let module = &mut project.modules[1];
    module.name = text.to_owned();
    let procedure = &mut module.procedures[0];
    procedure.name = text.to_owned();
    procedure.primary_family = text.to_owned();
    procedure.secondary_families = Some(vec![text.to_owned()]);
    procedure.traits = Some(vec![text.to_owned()]);
    procedure.signals = vec![signal(Severity::Low, None, None)];
}

#[test]
fn every_cell_that_starts_like_a_formula_gets_a_prefix() {
    for start in DANGEROUS_STARTS {
        let mut report = common::fleet();
        let hostile = format!("{start}cmd|' /C calc'!A0");
        put_everywhere(&mut report, &hostile);

        for (name, document) in documents(&report) {
            let cells: Vec<String> = rows(&document).into_iter().flatten().collect();
            let hostile_cells: Vec<_> = cells.iter().filter(|cell| cell.contains("cmd|")).collect();

            // The cell keeps what it held, behind the prefix.
            assert!(!hostile_cells.is_empty(), "{name}: {start:?}");
            for cell in hostile_cells {
                assert_eq!(cell, &format!("'{hostile}"), "{name}: {start:?}");
            }
            // No cell is left that a spreadsheet would read as a formula.
            for cell in &cells {
                assert!(!cell.starts_with(DANGEROUS_STARTS), "{name}: {cell:?}");
            }
        }
    }
}

#[test]
fn a_cell_that_starts_otherwise_is_left_as_it_is() {
    for text in ["a=b", "#=x", " =x", "x\t=1", "'=x", "\n=x"] {
        let mut report = common::fleet();
        report.files[1].path = text.to_owned();

        let files = rows(&render::files(&report).expect("files.csv"));

        assert_eq!(files[2][0], text);
    }
}

#[test]
fn the_injection_fixture_leaves_no_cell_that_a_spreadsheet_would_run() {
    let report = common::injection();

    for (name, document) in documents(&report) {
        for cell in rows(&document).into_iter().flatten() {
            assert!(!cell.starts_with(DANGEROUS_STARTS), "{name}: {cell:?}");
        }
    }
    let files = rows(&render::files(&report).expect("files.csv"));
    assert_eq!(
        files[1][0],
        "'=HYPERLINK(\"http://x\")/<img src=x onerror=alert(1)>.xlsm"
    );
    assert_eq!(files[2][0], "'\t=cmd|' /C calc'!A0.xlsm");
    let procedures = rows(&render::procedures(&report).expect("procedures.csv"));
    assert_eq!(procedures[1][1], "'=HYPERLINK(\"http://x\")");
    // The markup is no concern of a CSV file: it is kept as text.
    assert_eq!(procedures[1][2], "<img src=x onerror=alert(1)>");
    assert_eq!(procedures[2][2], "'@SUM(1+1)");
}
