// SPDX-License-Identifier: Apache-2.0
//! Tests of the signals of each scope, `requires_family`, the container
//! indicators and `max_severity`, over rules written for each test. The
//! chains and the samples of the corpus have their own modules, which read the
//! helpers of this one.

mod chains;
mod samples;

use super::{MAX_EVIDENCE_PER_SIGNAL, ProcedureFamilies, evaluate};
use crate::engine::testing::{calls, line, module, procedure, project, project_of, references};
use crate::engine::{CodeLine, ModuleInput, ProjectInput};
use crate::report::{Evidence, Signal};
use crate::rules::{
    ContainerIndicator, Label, Matcher, ModuleType, Scope, Security, SecuritySignal, Severity,
    TextMode,
};
use Severity::{High, Info, Low, Medium};

fn label() -> Label {
    Label {
        en: String::new(),
        fr: String::new(),
    }
}

fn rule(id: &str, matcher: Matcher, severity: Severity) -> SecuritySignal {
    SecuritySignal {
        id: id.to_owned(),
        category: "test".to_owned(),
        matcher,
        severity,
        scope: Scope::Procedure,
        text: TextMode::Code,
        requires_family: None,
        label: label(),
    }
}

/// A rule of procedure scope and medium severity for a call of `name`.
fn calling(id: &str, name: &str) -> SecuritySignal {
    rule(id, calls(&[name]), Medium)
}

fn scoped(scope: Scope, rule: SecuritySignal) -> SecuritySignal {
    SecuritySignal { scope, ..rule }
}

fn security(signals: Vec<SecuritySignal>) -> Security {
    Security {
        version: "test".to_owned(),
        signals,
        chains: Vec::new(),
        container_indicators: Vec::new(),
    }
}

fn indicator(id: &str, severity: Severity) -> ContainerIndicator {
    let label = label();
    ContainerIndicator {
        id: id.to_owned(),
        severity,
        label,
    }
}

/// The statement `x = <name>()` on a line: it calls `name`.
fn statement(number: u32, name: &str) -> CodeLine {
    line(number, &format!("x = {name}()")).call(name)
}

/// A standard module whose only procedure, `Main`, calls `name` on line 4.
fn module_calling(module_name: &str, name: &str) -> ModuleInput {
    let main = procedure("Main", 3, vec![statement(4, name)]);
    module(module_name, ModuleType::Standard, vec![main])
}

/// A project of one standard module with a procedure for each name, which
/// calls it: the first procedure `P10` starts on line 10 and calls on line 12,
/// the next ones are `P20` and `P30` on lines 20 and 22, and so on.
fn project_calling(names: &[&str]) -> ProjectInput {
    let starts = (1u32..).map(|step| step.saturating_mul(10));
    let procedures = (names.iter().zip(starts))
        .map(|(name, start)| {
            let call = statement(start.saturating_add(2), name);
            procedure(&format!("P{start}"), start, vec![call])
        })
        .collect();
    project(vec![module("Module1", ModuleType::Standard, procedures)])
}

/// A signal with no module, no procedure and no evidence.
fn bare(id: &str, severity: Severity) -> Signal {
    Signal {
        signal_id: id.to_owned(),
        severity,
        module: None,
        procedure: None,
        evidence: None,
    }
}

/// A signal of medium severity as the rules of these tests give it, citing
/// these lines and snippets.
fn signal_citing(
    id: &str,
    module: &str,
    procedure: Option<&str>,
    evidence: &[(u32, &str)],
) -> Signal {
    let cite = |&(line, snippet): &(u32, &str)| Evidence {
        rule_id: id.to_owned(),
        line,
        snippet: Some(snippet.to_owned()),
    };
    Signal {
        module: Some(module.to_owned()),
        procedure: procedure.map(str::to_owned),
        evidence: Some(evidence.iter().map(cite).collect()),
        ..bare(id, Medium)
    }
}

/// A chain that holds, of high severity as the chains of these tests are. It
/// cites the first line of these signals, with no snippet.
fn chain_signal(id: &str, cited: &[(&str, u32)]) -> Signal {
    let item = |&(rule_id, line): &(&str, u32)| Evidence {
        rule_id: rule_id.to_owned(),
        line,
        snippet: None,
    };
    Signal {
        evidence: Some(cited.iter().map(item).collect()),
        ..bare(id, High)
    }
}

fn ids(signals: &[Signal]) -> Vec<&str> {
    signals
        .iter()
        .map(|signal| signal.signal_id.as_str())
        .collect()
}

#[test]
fn a_procedure_signal_is_raised_in_each_procedure_that_matches() {
    let project = project_calling(&["Shell", "Other", "Shell"]);
    let rules = security(vec![calling("t.shell", "Shell")]);
    let found = evaluate(&project, &rules, &[], &[]);
    let call = "x = Shell()";
    let signals = &found.modules[0].procedures;
    let at =
        |procedure, line| signal_citing("t.shell", "Module1", Some(procedure), &[(line, call)]);
    assert_eq!(signals[0], [at("P10", 12)]);
    assert!(signals[1].is_empty());
    assert_eq!(signals[2], [at("P30", 32)]);
    assert!(found.modules[0].signals.is_empty());
    assert!(found.file.is_empty());
}

#[test]
fn the_signals_of_a_procedure_follow_the_order_of_the_rules() {
    let project = project_of(&[line(2, "A: B").call("A").call("B")]);
    let rules = security(vec![calling("t.b", "B"), calling("t.a", "A")]);
    let found = evaluate(&project, &rules, &[], &[]);
    assert_eq!(ids(&found.modules[0].procedures[0]), ["t.b", "t.a"]);
}

#[test]
fn a_module_signal_reads_the_declarations_and_the_procedures_and_is_raised_once() {
    let first = procedure("First", 5, vec![statement(6, "Shell")]);
    let project = project(vec![
        module("Module1", ModuleType::Standard, vec![first])
            .with_declarations(vec![statement(2, "Shell")]),
        module_calling("Module2", "Other"),
    ]);
    let rules = security(vec![scoped(Scope::Module, calling("t.shell", "Shell"))]);
    let found = evaluate(&project, &rules, &[], &[]);
    let call = "x = Shell()";
    let expected = signal_citing("t.shell", "Module1", None, &[(2, call), (6, call)]);
    assert_eq!(found.modules[0].signals, [expected]);
    assert!(found.modules[0].procedures[0].is_empty());
    assert!(found.modules[1].signals.is_empty());
    assert!(found.file.is_empty());
}

#[test]
fn a_project_signal_is_a_signal_of_the_file_for_each_module_that_has_hits() {
    let project = project(vec![
        module_calling("Module1", "Shell"),
        module_calling("Module2", "Other"),
        module_calling("Module3", "Shell"),
    ]);
    let rules = security(vec![scoped(Scope::Project, calling("t.shell", "Shell"))]);
    let found = evaluate(&project, &rules, &[], &[]);
    let call = "x = Shell()";
    assert_eq!(
        found.file,
        [
            signal_citing("t.shell", "Module1", None, &[(4, call)]),
            signal_citing("t.shell", "Module3", None, &[(4, call)]),
        ]
    );
    assert_eq!(found.signals().count(), 2);
}

#[test]
fn evidence_cites_each_line_once_in_source_order_up_to_the_cap() {
    // Two calls on line 2, then one on each of the lines 3 to 14.
    let mut body = vec![line(2, "x = A() + A()").call("A").call("A")];
    body.extend((3..=14).map(|number| statement(number, "A")));
    let rules = security(vec![calling("t.a", "A")]);
    let found = evaluate(&project_of(&body), &rules, &[], &[]);
    let evidence = found.modules[0].procedures[0][0].evidence.iter().flatten();
    let cited: Vec<u32> = evidence.map(|item| item.line).collect();
    let first_lines: Vec<u32> = (2..).take(MAX_EVIDENCE_PER_SIGNAL).collect();
    assert_eq!(cited, first_lines);
}

fn families(primary: &str, secondary: &[&str]) -> ProcedureFamilies {
    ProcedureFamilies {
        primary: primary.to_owned(),
        secondary: secondary.iter().copied().map(str::to_owned).collect(),
    }
}

/// A signal for a call of `Send` that is raised for an office interop
/// procedure only.
fn mail(scope: Scope) -> SecuritySignal {
    SecuritySignal {
        requires_family: Some("12-office-interop".to_owned()),
        ..scoped(scope, calling("t.mail", "Send"))
    }
}

#[test]
fn a_signal_that_requires_a_family_needs_it_as_the_primary_or_a_secondary_family() {
    let held = [vec![
        families("12-office-interop", &[]),
        families("04-lookup", &["12-office-interop"]),
        families("04-lookup", &["09-events"]),
        families("", &[]),
    ]];
    let rules = security(vec![mail(Scope::Procedure)]);
    let found = evaluate(&project_calling(&["Send"; 5]), &rules, &held, &[]);
    // The fifth procedure has no entry at all.
    let signals = found.modules[0].procedures.iter();
    let raised: Vec<bool> = signals.map(|signals| !signals.is_empty()).collect();
    assert_eq!(raised, [true, true, false, false, false]);
}

#[test]
fn a_module_has_a_family_when_one_of_its_procedures_does() {
    let first = procedure("First", 3, vec![statement(4, "Send")]);
    let second = procedure("Second", 6, vec![statement(7, "Send")]);
    let project = project(vec![
        module("Module1", ModuleType::Standard, vec![first, second]),
        module_calling("Module2", "Send"),
    ]);
    // Of the three procedures, only the first one has the family.
    let held = [
        vec![
            families("12-office-interop", &[]),
            families("04-lookup", &[]),
        ],
        vec![families("04-lookup", &[])],
    ];
    let call = "x = Send()";
    let signal = signal_citing("t.mail", "Module1", None, &[(4, call), (7, call)]);
    let expected = std::slice::from_ref(&signal);
    let rules = security(vec![mail(Scope::Module)]);
    let found = evaluate(&project, &rules, &held, &[]);
    assert_eq!(found.modules[0].signals, expected);
    assert!(found.modules[1].signals.is_empty());
    let rules = security(vec![mail(Scope::Project)]);
    assert_eq!(evaluate(&project, &rules, &held, &[]).file, expected);
    assert!(evaluate(&project, &rules, &[], &[]).file.is_empty());
}

#[test]
fn a_reference_raises_a_project_signal_that_cites_no_line() {
    let outlook = |id| scoped(Scope::Project, rule(id, references(&["Outlook"]), Low));
    let interop = SecuritySignal {
        requires_family: Some("12-office-interop".to_owned()),
        ..outlook("t.interop")
    };
    let rules = security(vec![outlook("t.outlook"), interop]);
    let project =
        project_calling(&["Other"]).with_references(&["Microsoft Outlook 16.0 Object Library"]);
    let raised = |held: &[Vec<ProcedureFamilies>]| evaluate(&project, &rules, held, &[]).file;
    assert_eq!(raised(&[]), [bare("t.outlook", Low)]);
    // A hit that belongs to no module needs a procedure of the project to have
    // the family.
    let held = [vec![families("12-office-interop", &[])]];
    let both = [bare("t.outlook", Low), bare("t.interop", Low)];
    assert_eq!(raised(&held), both);
}

#[test]
fn the_container_indicators_raised_come_first_among_the_signals_of_the_file() {
    let rules = Security {
        container_indicators: vec![
            indicator("sec.xlm-macrosheet", Medium),
            indicator("sec.vbaproject-in-xlsx", High),
            indicator("sec.suspicious-archive", Medium),
        ],
        ..security(vec![scoped(Scope::Project, calling("t.shell", "Shell"))])
    };
    // Out of order, twice, and one that the rules do not list.
    let raised = [
        "sec.vbaproject-in-xlsx",
        "sec.unknown",
        "sec.xlm-macrosheet",
        "sec.vbaproject-in-xlsx",
    ];
    let found = evaluate(&project_calling(&["Shell"]), &rules, &[], &raised);
    // In the order of the rules, with no module and no evidence.
    assert_eq!(
        found.file[..2],
        [
            bare("sec.xlm-macrosheet", Medium),
            bare("sec.vbaproject-in-xlsx", High)
        ]
    );
    assert_eq!(ids(&found.file[2..]), ["t.shell"]);
}

#[test]
fn the_file_has_the_highest_severity_of_all_its_signals() {
    let info = rule("t.info", calls(&["I"]), Info);
    let low = scoped(Scope::Module, rule("t.low", calls(&["L"]), Low));
    let medium = scoped(Scope::Project, calling("t.medium", "M"));
    let rules = Security {
        container_indicators: vec![indicator("sec.source-missing", High)],
        ..security(vec![info, low, medium])
    };
    let max = |called: &[&str], raised: &[&str]| {
        evaluate(&project_calling(called), &rules, &[], raised).max_severity()
    };
    assert_eq!(max(&[], &[]), None);
    assert_eq!(max(&["Other"], &[]), None);
    // One signal for each scope, procedure then module then project.
    assert_eq!(max(&["I"], &[]), Some(Info));
    assert_eq!(max(&["I", "L"], &[]), Some(Low));
    assert_eq!(max(&["I", "L", "M"], &[]), Some(Medium));
    // An indicator counts with no other signal, and above the others.
    assert_eq!(max(&[], &["sec.source-missing"]), Some(High));
    assert_eq!(max(&["M"], &["sec.source-missing"]), Some(High));
}
