// SPDX-License-Identifier: Apache-2.0
//! The signals that the engine must detect in the samples of family 14
//! (ACCEPTANCE A5), over the shipped rules.
//!
//! Each project is written by hand in the shape that the parser gives for its
//! sample: the lines of the corpus file that the signals read, at their
//! physical line numbers, with the facts that the parser finds in them. The
//! lines in between are left out, so the `End Sub` of a procedure follows the
//! last line given. These tests hold the lines that the signals cite; the
//! corpus test, which will read the samples themselves, is still to come.

use super::{chain_signal, families};
use crate::engine::ModuleInput;
use crate::engine::security::{FileSignals, ProcedureFamilies, evaluate};
use crate::engine::testing::{line, module, procedure, project};
use crate::report::Signal;
use crate::rules::{ModuleType, RuleSet, RulesSources, Severity};
use Severity::{High, Medium};

/// The signals of a project of this module over the shipped rules, when the
/// procedures have these families.
fn detect(module: ModuleInput, held: &[Vec<ProcedureFamilies>]) -> FileSignals {
    let rules = RuleSet::load(&RulesSources::EMBEDDED).expect("the shipped rules load");
    evaluate(&project(vec![module]), &rules.security, held, &[])
}

/// Each signal with the lines that it cites.
fn cited_lines(signals: &[Signal]) -> Vec<(&str, Vec<u32>)> {
    let cited = |signal: &Signal| {
        let evidence = signal.evidence.iter().flatten();
        evidence.map(|item| item.line).collect()
    };
    (signals.iter())
        .map(|signal| (signal.signal_id.as_str(), cited(signal)))
        .collect()
}

#[test]
fn sample_14_01_auto_open_that_calls_shell() {
    let body = vec![
        line(12, "Dim r As Double"),
        line(13, r#"r = Shell("notepad.exe", vbNormalFocus)"#).call("Shell"),
    ];
    let auto_open = procedure("Auto_Open", 11, body);
    let found = detect(
        module("Module1", ModuleType::Standard, vec![auto_open]),
        &[],
    );
    let expected = [("sec.auto-exec", vec![11]), ("sec.shell", vec![13])];
    assert_eq!(cited_lines(&found.modules[0].procedures[0]), expected);
    let cited = [("sec.auto-exec", 11), ("sec.shell", 13)];
    assert_eq!(
        found.file,
        [chain_signal("sec.chain.autoexec-execution", &cited)]
    );
    assert_eq!(found.max_severity(), Some(High));
}

#[test]
fn sample_14_02_workbook_open_that_builds_a_command_from_character_codes() {
    let body = vec![
        line(13, "Dim o As Object, c As String"),
        line(
            14,
            r#"c = Chr(110) & Chr(111) & "te" & "pad" & Chr(46) & StrReverse("exe")"#,
        ),
        line(15, r#"Set o = CreateObject("WScr" & "ipt.Sh" & "ell")"#)
            .with_strings(&["WScript.Shell"])
            .call("CreateObject")
            .progid("WScript.Shell"),
        line(16, "o.Run c, 1, False").call_after_dot("Run"),
    ];
    let mut open = procedure("Workbook_Open", 12, body);
    // The parser cites the statement of line 14, the third one of the procedure.
    open.predicates.chr_obfuscation = vec![2];
    let found = detect(
        module("ThisWorkbook", ModuleType::Document, vec![open]),
        &[],
    );
    let expected = [
        ("sec.auto-exec", vec![12]),
        ("sec.wscript-run", vec![15]),
        ("sec.chr-obfuscation", vec![14]),
    ];
    assert_eq!(cited_lines(&found.modules[0].procedures[0]), expected);
    let to_run = [("sec.auto-exec", 12), ("sec.wscript-run", 15)];
    let obfuscated = [("sec.chr-obfuscation", 14), ("sec.wscript-run", 15)];
    let chains = [
        chain_signal("sec.chain.autoexec-execution", &to_run),
        chain_signal("sec.chain.obfuscated-execution", &obfuscated),
    ];
    assert_eq!(found.file, chains);
    assert_eq!(found.max_severity(), Some(High));
}

/// The module of the sample 14-03: a download written to the temporary folder.
fn download() -> ModuleInput {
    let body = vec![
        line(13, "Dim http As Object, stm As Object, chemin As String"),
        line(14, r#"chemin = Environ("TEMP") & "\referentiel.csv""#).call("Environ"),
        line(15, r#"Set http = CreateObject("MSXML2.XMLHTTP")"#)
            .call("CreateObject")
            .progid("MSXML2.XMLHTTP"),
        line(
            16,
            r#"http.Open "GET", "https://example.com/referentiel.csv", False"#,
        ),
        line(17, "http.send"),
        line(23, "stm.SaveToFile chemin, 2").member("SaveToFile"),
    ];
    let download = procedure("TelechargerReferentiel", 12, body);
    module("Module1", ModuleType::Standard, vec![download])
}

#[test]
fn sample_14_03_download_written_to_the_temporary_folder() {
    let found = detect(download(), &[]);
    let expected = [
        ("sec.http", vec![15]),
        ("sec.url-literal", vec![16]),
        ("sec.stream-save", vec![23]),
        ("sec.temp-path", vec![14]),
    ];
    assert_eq!(cited_lines(&found.modules[0].procedures[0]), expected);
    let cited = [("sec.http", 15), ("sec.stream-save", 23)];
    assert_eq!(
        found.file,
        [chain_signal("sec.chain.download-write", &cited)]
    );
    assert_eq!(found.max_severity(), Some(High));
}

#[test]
fn sample_14_03_send_is_a_mail_only_in_a_procedure_of_office_interop() {
    let lookup = [vec![families("11-external-data", &[])]];
    let found = detect(download(), &lookup);
    let mail = ("sec.mail-send", vec![17]);
    assert!(!cited_lines(&found.modules[0].procedures[0]).contains(&mail));
    let interop = [vec![families("11-external-data", &["12-office-interop"])]];
    let found = detect(download(), &interop);
    let signals = cited_lines(&found.modules[0].procedures[0]);
    assert_eq!(signals.last(), Some(&mail));
}

#[test]
fn sample_14_04_password_in_a_connection_string() {
    let connection = "Provider=SQLOLEDB;Data Source=srv-finance01;Initial Catalog=COMPTA;User ID=sa;Password=Fictif2026!;";
    let declaration = line(
        11,
        &format!("Private Const CNX As String = \"{connection}\""),
    );
    let body = vec![
        line(15, r#"Set cn = CreateObject("ADODB.Connection")"#)
            .call("CreateObject")
            .progid("ADODB.Connection"),
        line(16, "cn.Open CNX"),
    ];
    let load = procedure("ChargerEcritures", 13, body);
    let sample = module("Module1", ModuleType::Standard, vec![load]);
    let found = detect(sample.with_declarations(vec![declaration]), &[]);
    // The signal is one of the module, as it reads the declarations too.
    assert_eq!(
        cited_lines(&found.modules[0].signals),
        [("sec.hardcoded-credential", vec![11])]
    );
    assert!(found.modules[0].procedures[0].is_empty());
    assert!(found.file.is_empty());
    assert_eq!(found.max_severity(), Some(Medium));
}

#[test]
fn sample_14_05_access_to_the_vba_project() {
    let export = procedure(
        "ExporterModules",
        12,
        vec![
            line(16, "For Each comp In ThisWorkbook.VBProject.VBComponents")
                .member("ThisWorkbook")
                .member("VBProject")
                .member("VBComponents"),
        ],
    );
    // The statement goes on to line 29, which the parser joins to it.
    let inject = procedure(
        "InjecterProcedure",
        27,
        vec![line(
            28,
            r#"ThisWorkbook.VBProject.VBComponents("Module1").CodeModule.AddFromString "Sub Bonjour()""#,
        )
        .member("ThisWorkbook")
        .member("VBProject")
        .member("VBComponents")
        .member("CodeModule")
        .member("AddFromString")],
    );
    let found = detect(
        module("Module1", ModuleType::Standard, vec![export, inject]),
        &[],
    );
    let procedures = &found.modules[0].procedures;
    assert_eq!(cited_lines(&procedures[0]), [("sec.vbproject", vec![16])]);
    assert_eq!(cited_lines(&procedures[1]), [("sec.vbproject", vec![28])]);
    assert!(found.file.is_empty());
    assert_eq!(found.max_severity(), Some(Medium));
}

#[test]
fn sample_14_06_xlm_macro_and_very_hidden_sheet() {
    let xlm = |number, text| {
        line(number, text)
            .chain("Application.ExecuteExcel4Macro")
            .call_after_dot("ExecuteExcel4Macro")
    };
    let body = vec![
        line(
            12,
            r#"ThisWorkbook.Worksheets("Parametres").Visible = xlSheetVeryHidden"#,
        ),
        xlm(
            13,
            r#"Application.ExecuteExcel4Macro "MESSAGE(TRUE,""Calcul en cours"")""#,
        ),
        xlm(14, r#"Application.ExecuteExcel4Macro "MESSAGE(FALSE)""#),
    ];
    let hide = procedure("MasquerParametres", 11, body);
    let found = detect(module("Module1", ModuleType::Standard, vec![hide]), &[]);
    // A name after a dot is no call, so the signal of the call is not raised.
    let expected = [("sec.xlm", vec![13, 14]), ("sec.very-hidden", vec![12])];
    assert_eq!(cited_lines(&found.modules[0].procedures[0]), expected);
    assert!(found.file.is_empty());
    assert_eq!(found.max_severity(), Some(Medium));
}
