// SPDX-License-Identifier: Apache-2.0
//! Tests of the chains: each form of term, the scopes that the signals come
//! from, and where the chains go among the signals of the file.

use super::{
    bare, calling, chain_signal, families, ids, indicator, label, mail, project_calling, rule,
    scoped, security,
};
use crate::engine::security::evaluate;
use crate::engine::testing::references;
use crate::report::Signal;
use crate::rules::{ChainTerm, Scope, Security, SecurityChain, SecuritySignal, Severity};
use Severity::{High, Low, Medium};

fn chain_rule(id: &str, all: Vec<ChainTerm>) -> SecurityChain {
    SecurityChain {
        id: id.to_owned(),
        all,
        severity: High,
        label: label(),
    }
}

/// A term that names the signal `id`.
fn term(id: &str) -> ChainTerm {
    ChainTerm::Signal(id.to_owned())
}

fn any(terms: Vec<ChainTerm>) -> ChainTerm {
    ChainTerm::Any(terms)
}

/// Rules with these signals and this chain.
fn with_chain(signals: Vec<SecuritySignal>, chain: SecurityChain) -> Security {
    Security {
        chains: vec![chain],
        ..security(signals)
    }
}

/// Rules with a signal for each call of `A`, `B`, `C` and `D`, named `t.a` and
/// so on, and with this chain.
fn rules_with(chain: SecurityChain) -> Security {
    let signals =
        ["a", "b", "c", "d"].map(|name| calling(&format!("t.{name}"), &name.to_ascii_uppercase()));
    with_chain(signals.to_vec(), chain)
}

/// The signals of the file when the procedures call these names, one name for
/// each: the first is called on line 12, the second on line 22, and so on.
fn raised(rules: &Security, called: &[&str]) -> Vec<Signal> {
    evaluate(&project_calling(called), rules, &[], &[]).file
}

#[test]
fn a_chain_holds_when_each_of_its_signals_is_raised() {
    let rules = rules_with(chain_rule("t.chain", vec![term("t.a"), term("t.b")]));
    let cited = [("t.a", 12), ("t.b", 22)];
    assert_eq!(
        raised(&rules, &["A", "B"]),
        [chain_signal("t.chain", &cited)]
    );
    assert!(raised(&rules, &["A"]).is_empty());
    assert!(raised(&rules, &["B", "C"]).is_empty());
    assert!(raised(&rules, &[]).is_empty());
}

#[test]
fn a_list_of_terms_holds_when_one_of_them_does_and_cites_each_one_that_holds() {
    let list = any(vec![term("t.b"), term("t.c"), term("t.d")]);
    let rules = rules_with(chain_rule("t.chain", vec![term("t.a"), list]));
    // D is raised before C, but the chain lists C before D.
    let cited = [("t.a", 12), ("t.c", 32), ("t.d", 22)];
    assert_eq!(
        raised(&rules, &["A", "D", "C"]),
        [chain_signal("t.chain", &cited)]
    );
    let cited = [("t.a", 12), ("t.b", 22)];
    assert_eq!(
        raised(&rules, &["A", "B"]),
        [chain_signal("t.chain", &cited)]
    );
    assert!(raised(&rules, &["A"]).is_empty());
    assert!(raised(&rules, &["B", "C", "D"]).is_empty());
}

#[test]
fn a_chain_of_two_lists_needs_one_signal_of_each() {
    let first = any(vec![term("t.a"), term("t.b")]);
    let second = any(vec![term("t.c"), term("t.d")]);
    let rules = rules_with(chain_rule("t.chain", vec![first, second]));
    let cited = [("t.b", 12), ("t.c", 22)];
    assert_eq!(
        raised(&rules, &["B", "C"]),
        [chain_signal("t.chain", &cited)]
    );
    assert!(raised(&rules, &["A", "B"]).is_empty());
    assert!(raised(&rules, &["C", "D"]).is_empty());
}

#[test]
fn a_list_of_terms_may_hold_another_list() {
    let inner = any(vec![term("t.b"), term("t.c")]);
    let chain = chain_rule("t.chain", vec![any(vec![term("t.a"), inner])]);
    let rules = rules_with(chain);
    let cited = chain_signal("t.chain", &[("t.c", 12)]);
    assert_eq!(raised(&rules, &["C"]), [cited]);
    assert!(raised(&rules, &["D"]).is_empty());
}

#[test]
fn a_signal_named_twice_is_cited_once() {
    let list = any(vec![term("t.a"), term("t.b")]);
    let rules = rules_with(chain_rule("t.chain", vec![term("t.a"), list]));
    let cited = [("t.a", 12), ("t.b", 22)];
    assert_eq!(
        raised(&rules, &["A", "B"]),
        [chain_signal("t.chain", &cited)]
    );
}

#[test]
fn a_chain_reads_the_signals_of_every_scope() {
    let signals = vec![
        calling("t.a", "A"),
        scoped(Scope::Module, calling("t.b", "B")),
        scoped(Scope::Project, calling("t.c", "C")),
    ];
    let terms = vec![term("t.a"), term("t.b"), term("t.c")];
    let rules = with_chain(signals, chain_rule("t.chain", terms));
    let found = evaluate(&project_calling(&["A", "B", "C"]), &rules, &[], &[]);
    // The signal of the module is in the module, the one of the project in the file.
    assert_eq!(ids(&found.modules[0].signals), ["t.b"]);
    assert_eq!(ids(&found.file), ["t.c", "t.chain"]);
    let cited = [("t.a", 12), ("t.b", 22), ("t.c", 32)];
    assert_eq!(found.file[1], chain_signal("t.chain", &cited));
}

#[test]
fn a_signal_that_the_family_removes_does_not_make_a_chain_hold() {
    let signals = vec![calling("t.a", "A"), mail(Scope::Procedure)];
    let chain = chain_rule("t.chain", vec![term("t.a"), term("t.mail")]);
    let rules = with_chain(signals, chain);
    let project = project_calling(&["A", "Send"]);
    // The second procedure raises t.mail if it is a procedure of office interop.
    let interop = [vec![
        families("04-lookup", &[]),
        families("12-office-interop", &[]),
    ]];
    let found = evaluate(&project, &rules, &interop, &[]);
    let cited = chain_signal("t.chain", &[("t.a", 12), ("t.mail", 22)]);
    assert_eq!(found.file, [cited]);
    let lookup = [vec![families("04-lookup", &[]), families("04-lookup", &[])]];
    assert!(evaluate(&project, &rules, &lookup, &[]).file.is_empty());
}

#[test]
fn a_signal_raised_in_several_places_is_cited_at_the_first_of_them() {
    let rules = rules_with(chain_rule("t.chain", vec![term("t.a"), term("t.b")]));
    let cited = [("t.a", 12), ("t.b", 22)];
    assert_eq!(
        raised(&rules, &["A", "B", "A"]),
        [chain_signal("t.chain", &cited)]
    );
    let cited = [("t.a", 22), ("t.b", 12)];
    assert_eq!(
        raised(&rules, &["B", "A", "A"]),
        [chain_signal("t.chain", &cited)]
    );
}

#[test]
fn a_signal_that_cites_no_line_makes_a_chain_hold_and_is_not_cited() {
    let outlook = scoped(
        Scope::Project,
        rule("t.outlook", references(&["Outlook"]), Low),
    );
    let signals = vec![outlook, calling("t.a", "A")];
    let chain = chain_rule("t.chain", vec![term("t.outlook"), term("t.a")]);
    let rules = with_chain(signals, chain);
    let project =
        project_calling(&["A"]).with_references(&["Microsoft Outlook 16.0 Object Library"]);
    let found = evaluate(&project, &rules, &[], &[]);
    let cited = chain_signal("t.chain", &[("t.a", 12)]);
    assert_eq!(found.file, [bare("t.outlook", Low), cited]);
}

#[test]
fn the_chains_come_last_among_the_signals_of_the_file_and_count_for_its_severity() {
    let signals = vec![
        calling("t.a", "A"),
        scoped(Scope::Project, calling("t.b", "B")),
    ];
    let chains = vec![
        chain_rule("t.second", vec![term("t.b")]),
        chain_rule("t.first", vec![term("t.a")]),
    ];
    let rules = Security {
        chains,
        container_indicators: vec![indicator("t.container", Low)],
        ..security(signals)
    };
    let project = project_calling(&["A", "B"]);
    let found = evaluate(&project, &rules, &[], &["t.container"]);
    // The indicator, the signal of the project, and the chains in the order of the rules.
    assert_eq!(
        ids(&found.file),
        ["t.container", "t.b", "t.second", "t.first"]
    );
    assert_eq!(found.max_severity(), Some(High));
    // The signals alone are of medium severity.
    let plain = Security {
        chains: Vec::new(),
        ..rules
    };
    let found = evaluate(&project, &plain, &[], &["t.container"]);
    assert_eq!(found.max_severity(), Some(Medium));
}
