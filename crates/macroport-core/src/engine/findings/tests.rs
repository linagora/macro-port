// SPDX-License-Identifier: Apache-2.0
//! Tests of the engine on a whole file, over the shipped rules. Each engine has
//! tests of its own: these check how they fit together, which is the order in
//! which they run, the alignment of their results with the modules and the
//! procedures, and the metrics that go to the complexity rules.

use super::{ProjectMetrics, run};
use crate::engine::testing::{line, module, procedure, project};
use crate::engine::{ProcedureInput, ProjectInput};
use crate::report::{FamilyScore, FileReport, Level, Metrics, Report, Signal};
use crate::rules::{ModuleType, RuleSet, RulesSources, Severity};
use Severity::{High, Medium};

fn shipped() -> RuleSet {
    RuleSet::load(&RulesSources::EMBEDDED).expect("the shipped rules load")
}

/// Metrics of this many logical lines, and none of the optional metrics.
fn metrics(logical_loc: u32) -> Metrics {
    Metrics {
        loc: logical_loc,
        logical_loc,
        procedures: None,
        cyclomatic: None,
        cyclomatic_total: None,
        max_nesting: None,
        parameters: None,
        goto_count: None,
        forms: None,
        declare_count: None,
        external_dependencies: None,
    }
}

fn ids(signals: &[Signal]) -> Vec<&str> {
    signals
        .iter()
        .map(|signal| signal.signal_id.as_str())
        .collect()
}

fn family_ids(families: &[FamilyScore]) -> Vec<&str> {
    families.iter().map(|score| score.family.as_str()).collect()
}

/// The report of the sample 14-01 as the schema ships it (ACCEPTANCE A3).
fn example() -> FileReport {
    let text = include_str!("../../../../../schema/examples/report.example.json");
    let report: Report = serde_json::from_str(text).expect("the model reads the example");
    (report.files.into_iter().next()).expect("the example has a file")
}

/// The `Auto_Open` of the sample 14-01, as its example report has it: on
/// line 4, and calling `Shell` on line 6.
fn auto_open() -> ProcedureInput {
    let body = vec![
        line(5, "Dim r As Double"),
        line(6, r#"r = Shell("notepad.exe", vbNormalFocus)"#).call("Shell"),
    ];
    procedure("Auto_Open", 4, body)
}

#[test]
fn the_engine_finds_in_the_sample_14_01_what_its_example_report_has() {
    let expected = example();
    let in_project = expected.vba_project.as_ref().expect("a VBA project");
    let in_module = &in_project.modules[0];
    let in_procedure = &in_module.procedures[0];
    let sample = project(vec![
        module("M_14_01", ModuleType::Standard, vec![auto_open()])
            .with_declarations(vec![line(2, "Option Explicit")]),
    ]);
    let measured = ProjectMetrics {
        procedures: vec![vec![in_procedure.metrics.clone()]],
        file: expected.metrics.clone().expect("the metrics of the file"),
    };
    let found = run(&sample, &shipped(), &measured, &[]);

    assert_eq!(found.modules.len(), 1);
    assert_eq!(found.modules[0].procedures.len(), 1);
    let (module, procedure) = (&found.modules[0], &found.modules[0].procedures[0]);
    assert_eq!(procedure.primary_family, in_procedure.primary_family);
    let secondary = in_procedure.secondary_families.as_ref();
    assert_eq!(Some(&procedure.secondary_families), secondary);
    assert_eq!(procedure.families, in_procedure.families);
    assert_eq!(procedure.signals, in_procedure.signals);
    assert_eq!(Some(&procedure.traits), in_procedure.traits.as_ref());
    assert_eq!(Some(&module.signals), in_module.signals.as_ref());

    let file = &found.file;
    assert_eq!(Some(&file.primary_family), expected.primary_family.as_ref());
    assert_eq!(Some(&file.families), expected.families.as_ref());
    assert_eq!(Some(&file.signals), expected.signals.as_ref());
    assert_eq!(file.max_severity, expected.max_severity);
    assert_eq!(Some(file.complexity_level), expected.complexity_level);
    assert_eq!(Some(&file.traits), expected.traits.as_ref());
}

/// Three modules, whose procedures nothing but their place and their code tell
/// apart:
///
/// - `Module1` has `Auto_Open`, of the events and the Windows API, which
///   launches a program, and `Ranger`, of no family.
/// - `Api` has no procedure, only a `Declare` statement.
/// - `Courrier` has `Envoyer`, of the Office interop, which sends mail;
///   `Relancer`, of no family, which calls `.Send` all the same and is unused;
///   and `Lancer`, of the Windows API and, less, of the Office interop, which
///   launches programs and sends mail.
fn three_modules() -> ProjectInput {
    let ranger = procedure("Ranger", 9, vec![line(10, "x = 1")]);
    let declare = line(
        2,
        r#"Declare Function GetTickCount Lib "kernel32" () As Long"#,
    );
    let outlook = line(5, r#"Set ol = CreateObject("Outlook.Application")"#)
        .call("CreateObject")
        .progid("Outlook.Application");
    let envoyer = procedure("Envoyer", 4, vec![outlook, line(6, "ol.Send")]);
    let mut relancer = procedure("Relancer", 9, vec![line(10, "rappel.Send")]);
    relancer.predicates.possibly_unused = true;
    let lancer = procedure(
        "Lancer",
        14,
        vec![
            line(15, "Set m = ol.CreateItem(0)").member("CreateItem"),
            line(16, "m.HTMLBody = body").member("HTMLBody"),
            line(17, "m.Send"),
            line(18, r#"Shell "a.exe""#).call("Shell"),
            line(19, r#"Shell "b.exe""#).call("Shell"),
        ],
    );
    project(vec![
        module("Module1", ModuleType::Standard, vec![auto_open(), ranger]),
        module("Api", ModuleType::Standard, Vec::new()).with_declarations(vec![declare]),
        module(
            "Courrier",
            ModuleType::Standard,
            vec![envoyer, relancer, lancer],
        ),
    ])
}

#[test]
fn each_result_stays_with_the_module_and_the_procedure_it_was_found_for() {
    // The file is large and its procedures are not. `Api` has no entry.
    let measured = ProjectMetrics {
        procedures: vec![
            vec![metrics(4), metrics(2)],
            Vec::new(),
            vec![metrics(5), metrics(5), metrics(5)],
        ],
        file: metrics(160),
    };
    let found = run(&three_modules(), &shipped(), &measured, &[]);

    assert_eq!(found.modules.len(), 3);
    let (module1, api, courrier) = (&found.modules[0], &found.modules[1], &found.modules[2]);
    let counts = [module1.procedures.len(), api.procedures.len()];
    assert_eq!((counts, courrier.procedures.len()), ([2, 0], 3));
    let (auto_open, ranger) = (&module1.procedures[0], &module1.procedures[1]);
    let (envoyer, relancer, lancer) = (
        &courrier.procedures[0],
        &courrier.procedures[1],
        &courrier.procedures[2],
    );

    assert_eq!(auto_open.primary_family, "09-events");
    assert_eq!(auto_open.secondary_families, ["13-windows-api"]);
    assert_eq!(ids(&auto_open.signals), ["sec.auto-exec", "sec.shell"]);
    assert!(auto_open.traits.is_empty());

    assert_eq!(ranger.primary_family, "unclassified");
    assert!(ranger.families.is_empty() && ranger.signals.is_empty());
    assert_eq!(ranger.traits, ["trivial"]);

    // The signals of module scope stay with their module, the one with no
    // procedure included.
    assert_eq!(ids(&api.signals), ["sec.declare"]);
    assert_eq!(api.signals[0].module.as_deref(), Some("Api"));
    assert!(module1.signals.is_empty() && courrier.signals.is_empty());

    assert_eq!(envoyer.primary_family, "12-office-interop");
    assert!(envoyer.secondary_families.is_empty());
    assert_eq!(ids(&envoyer.signals), ["sec.mail-send"]);
    assert!(envoyer.traits.is_empty());

    // `requires_family` read the families that the first step gave: the same
    // call is no mail where the procedure is of no family, and it is mail where
    // the family is a secondary one.
    assert_eq!(relancer.primary_family, "unclassified");
    assert!(relancer.signals.is_empty());
    assert_eq!(relancer.traits, ["possibly_unused"]);
    assert_eq!(lancer.primary_family, "13-windows-api");
    assert_eq!(lancer.secondary_families, ["12-office-interop"]);
    assert_eq!(ids(&lancer.signals), ["sec.shell", "sec.mail-send"]);
    assert_eq!(lancer.signals[1].module.as_deref(), Some("Courrier"));
    assert_eq!(lancer.signals[1].procedure.as_deref(), Some("Lancer"));
    assert!(lancer.traits.is_empty());

    // The file has the sums of the families, every signal and its own metrics,
    // not those of a procedure.
    let file = &found.file;
    assert_eq!(file.primary_family, "13-windows-api");
    let sums = ["13-windows-api", "12-office-interop", "09-events"];
    assert_eq!(family_ids(&file.families), sums);
    assert_eq!(ids(&file.signals), ["sec.chain.autoexec-execution"]);
    assert_eq!(file.max_severity, Some(High));
    assert_eq!(file.complexity_level, Level::Medium);
    // A structural trait holds of the file when it holds anywhere in it: the
    // unused `Relancer` makes the file possibly unused, and the file is not
    // trivial because `Ranger` is.
    assert_eq!(file.traits, ["possibly_unused"]);
}

#[test]
fn the_container_indicators_count_for_a_file_with_no_code() {
    let measured = ProjectMetrics {
        procedures: Vec::new(),
        file: metrics(0),
    };
    let indicators = ["sec.xlm-macrosheet", "sec.vbaproject-in-xlsx"];
    let found = run(&ProjectInput::default(), &shipped(), &measured, &indicators);
    assert!(found.modules.is_empty());
    assert_eq!(found.file.primary_family, "unclassified");
    assert!(found.file.families.is_empty());
    assert_eq!(
        ids(&found.file.signals),
        ["sec.xlm-macrosheet", "sec.vbaproject-in-xlsx"]
    );
    assert_eq!(found.file.max_severity, Some(High));
    assert_eq!(found.file.complexity_level, Level::Low);
    // The metrics of a file with no code give it the trait: whether to report
    // it is for the caller.
    assert_eq!(found.file.traits, ["trivial"]);

    let found = run(&ProjectInput::default(), &shipped(), &measured, &[]);
    assert!(found.file.signals.is_empty());
    assert_eq!(found.file.max_severity, None);
}

#[test]
fn the_highest_severity_may_come_from_the_signal_of_a_procedure() {
    // A program launched by a procedure that nothing runs by itself: there is
    // no chain, so the file has no signal of its own.
    let launching = procedure("Lancer", 1, vec![line(2, r#"Shell "a.exe""#).call("Shell")]);
    let sample = project(vec![module(
        "Module1",
        ModuleType::Standard,
        vec![launching],
    )]);
    let measured = ProjectMetrics {
        procedures: vec![vec![metrics(1)]],
        file: metrics(1),
    };
    let found = run(&sample, &shipped(), &measured, &[]);
    assert_eq!(ids(&found.modules[0].procedures[0].signals), ["sec.shell"]);
    assert!(found.file.signals.is_empty());
    assert_eq!(found.file.max_severity, Some(Medium));
}

#[test]
fn metrics_that_do_not_line_up_with_the_project_are_read_as_zero_or_left_out() {
    let sample = project(vec![
        module("Module1", ModuleType::Standard, vec![auto_open()]),
        module(
            "Module2",
            ModuleType::Standard,
            vec![procedure("Ranger", 3, vec![line(4, "x = 1")])],
        ),
    ]);
    // Measured: the first procedure only, and a module that the project lacks.
    let measured = ProjectMetrics {
        procedures: vec![vec![metrics(4)], Vec::new(), vec![metrics(1)]],
        file: metrics(5),
    };
    let found = run(&sample, &shipped(), &measured, &[]);
    assert_eq!(found.modules.len(), 2);
    assert!(found.modules[0].procedures[0].traits.is_empty());
    // A procedure that was not measured has no line: it is trivial.
    assert_eq!(found.modules[1].procedures[0].traits, ["trivial"]);
}
