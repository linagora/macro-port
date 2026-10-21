// SPDX-License-Identifier: Apache-2.0
//! Tests of the family scores, over projects written by hand.

use super::*;
use crate::engine::CodeLine;
use crate::engine::testing::{line, members, module, procedure, project, project_of, references};
use crate::rules::{Family, Label, Matcher, ModuleType, Scoring, TextMode};

/// A signal of procedure scope, with the default cap of 3 hits, that matches
/// the members `names`.
fn signal(id: &str, names: &[&str], weight: u32) -> FamilySignal {
    FamilySignal {
        id: id.to_owned(),
        matcher: members(names),
        weight,
        max_hits_per_procedure: 3,
        scope: Scope::Procedure,
        text: TextMode::Code,
        label: None,
    }
}

impl FamilySignal {
    fn capped(mut self, cap: u32) -> Self {
        self.max_hits_per_procedure = cap;
        self
    }

    fn at(mut self, scope: Scope) -> Self {
        self.scope = scope;
        self
    }

    fn matching(mut self, matcher: Matcher) -> Self {
        self.matcher = matcher;
        self
    }
}

fn family(id: &str, signals: Vec<FamilySignal>) -> Family {
    let label = Label {
        en: String::new(),
        fr: String::new(),
    };
    Family {
        id: id.to_owned(),
        corpus_slug: id.to_owned(),
        label,
        signals,
    }
}

/// Rules with the scoring of `rules/families.yaml`.
fn rules(families: Vec<Family>) -> Families {
    Families {
        version: String::new(),
        scoring: Scoring {
            min_score: 3.0,
            secondary_ratio: 0.6,
            default_max_hits_per_procedure: 3,
        },
        families,
    }
}

/// One family `01-a` with these signals.
fn rules_of(signals: Vec<FamilySignal>) -> Families {
    rules(vec![family("01-a", signals)])
}

/// A statement on the physical line `number` that uses the member `name`.
fn uses(number: u32, name: &str) -> CodeLine {
    line(number, &format!("x.{name}")).member(name)
}

/// A procedure of the standard module `Module1` from line 1, whose body uses
/// these members, one per line from line 2.
fn hits_of(names: &[&str]) -> ProjectInput {
    let body: Vec<_> = (2..).zip(names).map(|(n, name)| uses(n, name)).collect();
    project_of(&body)
}

fn family_and_score(score: &FamilyScore) -> (&str, f64) {
    (score.family.as_str(), score.score)
}

/// The family and the score of each family score of a procedure, in order.
fn scores(found: &ProcedureFamilies) -> Vec<(&str, f64)> {
    found.families.iter().map(family_and_score).collect()
}

/// The rule and the line of each evidence of a score, in order.
fn cited(score: &FamilyScore) -> Vec<(&str, u32)> {
    let evidence = score.evidence.as_deref().unwrap_or_default();
    evidence
        .iter()
        .map(|item| (item.rule_id.as_str(), item.line))
        .collect()
}

fn lines_cited(score: &FamilyScore) -> Vec<u32> {
    cited(score).into_iter().map(|(_, line)| line).collect()
}

#[test]
fn a_signal_adds_its_weight_for_each_hit_up_to_its_cap() {
    // A signal of weight 2: the hits it finds, its cap and the score.
    for (hits, cap, score) in [(1, 3, 2), (3, 3, 6), (9, 3, 6), (3, 1, 2), (3, 2, 4)] {
        let rules = rules_of(vec![signal("a.foo", &["Foo"], 2).capped(cap)]);
        let found = &classify(&hits_of(&vec!["Foo"; hits]), &rules).modules[0][0];
        let expected = [("01-a", f64::from(score))];
        assert_eq!(scores(found), expected, "{hits} hits, cap {cap}");
    }
    // The first hits are the ones counted and cited.
    let rules = rules_of(vec![signal("a.foo", &["Foo"], 2)]);
    let found = &classify(&hits_of(&["Foo"; 5]), &rules).modules[0][0];
    assert_eq!(lines_cited(&found.families[0]), [2, 3, 4]);
}

#[test]
fn the_signals_of_a_family_add_up_with_their_evidence_in_the_order_of_the_lines() {
    let rules = rules_of(vec![
        signal("a.foo", &["Foo"], 2),
        signal("a.bar", &["Bar"], 1),
    ]);
    let found = &classify(&hits_of(&["Bar", "Foo", "Bar", "Baz"]), &rules).modules[0][0];
    assert_eq!(scores(found), [("01-a", 4.0)]);
    let evidence = &found.families[0].evidence.as_ref().unwrap();
    assert_eq!(
        cited(&found.families[0]),
        [("a.bar", 2), ("a.foo", 3), ("a.bar", 4)]
    );
    assert_eq!(evidence[0].snippet.as_deref(), Some("x.Bar"));
}

/// `Module1` uses `Foo` at lines 2 and 3 of its declarations. Its procedure
/// `First` (lines 5 to 7) uses `Foo` at line 6, and `Second` (lines 9 to 11)
/// uses `Bar` at line 10. `Module2` has `Third` (lines 7 to 9), which uses
/// `Bar` at line 8.
fn modules_with_foo() -> ProjectInput {
    let first = procedure("First", 5, vec![uses(6, "Foo")]);
    let second = procedure("Second", 9, vec![uses(10, "Bar")]);
    project(vec![
        module("Module1", ModuleType::Standard, vec![first, second])
            .with_declarations(vec![uses(2, "Foo"), uses(3, "Foo")]),
        module(
            "Module2",
            ModuleType::Standard,
            vec![procedure("Third", 7, vec![uses(8, "Bar")])],
        ),
    ])
}

#[test]
fn a_procedure_scores_its_own_lines_only() {
    let rules = rules_of(vec![signal("a.foo", &["Foo"], 3)]);
    let found = classify(&modules_with_foo(), &rules).modules;
    assert_eq!(scores(&found[0][0]), [("01-a", 3.0)]);
    // The declarations and the other procedures are not its lines.
    assert!(found[0][1].families.is_empty());
    assert!(found[1][0].families.is_empty());
}

#[test]
fn a_module_signal_gives_half_its_weight_for_each_hit_to_every_procedure_of_the_module() {
    let foo = signal("a.foo", &["Foo"], 3).at(Scope::Module);
    let found = classify(&modules_with_foo(), &rules_of(vec![foo])).modules;
    // Three hits in the module, 3 x 3 / 2 each time.
    assert_eq!(scores(&found[0][0]), [("01-a", 4.5)]);
    assert_eq!(scores(&found[0][1]), [("01-a", 4.5)]);
    // The module of another procedure is another scope.
    assert!(found[1][0].families.is_empty());
    // The cap counts the hits of the module.
    let capped = signal("a.foo", &["Foo"], 3).at(Scope::Module).capped(2);
    let found = classify(&modules_with_foo(), &rules_of(vec![capped])).modules;
    assert_eq!(scores(&found[0][1]), [("01-a", 3.0)]);
}

#[test]
fn a_module_signal_cites_the_hits_outside_the_procedure_at_its_declaration() {
    let foo = signal("a.foo", &["Foo"], 4).at(Scope::Module).capped(2);
    let found = classify(&modules_with_foo(), &rules_of(vec![foo])).modules;
    // `First` counts its own hit first, at line 6, then the first of the
    // declarations, which stands at the declaration of `First`, line 5.
    assert_eq!(lines_cited(&found[0][0].families[0]), [5, 6]);
    // `Second` has none of its own: the first two hits, at line 9.
    assert_eq!(lines_cited(&found[0][1].families[0]), [9, 9]);
}

#[test]
fn a_project_signal_gives_half_its_weight_to_every_procedure_of_every_module() {
    let kernel = signal("a.ref", &[], 4)
        .at(Scope::Project)
        .matching(references(&["Kernel"]));
    let project = modules_with_foo().with_references(&["Kernel Library", "Other Library"]);
    let found = classify(&project, &rules_of(vec![kernel])).modules;
    for found in found.iter().flatten() {
        assert_eq!(scores(found), [("01-a", 2.0)]);
    }
    // A reference has no line: the evidence cites the declaration of the
    // procedure and quotes the reference.
    let evidence = &found[1][0].families[0].evidence.as_ref().unwrap()[0];
    assert_eq!(evidence.line, 7);
    assert_eq!(evidence.snippet.as_deref(), Some("Kernel Library"));
}

/// Signals for the member `name`, one of each scope. The two that reach beyond
/// the procedure count one hit.
fn in_every_scope(prefix: &str, name: &str) -> Vec<FamilySignal> {
    let id = |scope: &str| format!("{prefix}.{scope}");
    vec![
        signal(&id("own"), &[name], 3),
        signal(&id("module"), &[name], 2)
            .at(Scope::Module)
            .capped(1),
        signal(&id("project"), &[name], 2)
            .at(Scope::Project)
            .capped(1),
    ]
}

#[test]
fn every_score_of_a_procedure_cites_lines_inside_it() {
    let rules = rules(vec![
        family("01-a", in_every_scope("a", "Foo")),
        family("02-b", in_every_scope("b", "Bar")),
    ]);
    let project = modules_with_foo();
    let found = classify(&project, &rules).modules;
    let owners = project.modules.iter().flat_map(|owner| &owner.procedures);
    for (found, owner) in found.iter().flatten().zip(owners) {
        let lines = owner.start_line..=owner.end_line;
        for score in &found.families {
            let cited = lines_cited(score);
            let inside = !cited.is_empty() && cited.iter().all(|line| lines.contains(line));
            assert!(inside, "{} of {}: {cited:?}", score.family, owner.name);
        }
    }
    // `First` does not use `Bar`, nor `Third` `Foo`: they score these families
    // from hits of their module and of `Module1`, cited at their declarations.
    assert_eq!(lines_cited(&found[0][0].families[1]), [5, 5]);
    assert_eq!(lines_cited(&found[1][0].families[1]), [7]);
    // The hits of another module count: 1 from the one `Foo` that the project
    // signal keeps, whatever the module.
    assert_eq!(scores(&found[1][0]), [("02-b", 5.0), ("01-a", 1.0)]);
}

/// Four families whose rules come in the reverse order of their ids, for a
/// procedure that uses `Foo` three times and `Bar` and `Baz` once.
///
/// - `01-a` and `03-c` have one signal of weight 1: 3 for the three hits.
/// - `02-b` has two signals, of weight 2 and 1: 3 for two hits.
/// - `04-d` has two signals of weight 1: 2.
fn tied_families() -> Families {
    let (bar, baz, foo) = (&["Bar"], &["Baz"], &["Foo"]);
    rules(vec![
        family(
            "04-d",
            vec![signal("d.bar", bar, 1), signal("d.baz", baz, 1)],
        ),
        family("03-c", vec![signal("c.foo", foo, 1)]),
        family(
            "02-b",
            vec![signal("b.bar", bar, 2), signal("b.baz", baz, 1)],
        ),
        family("01-a", vec![signal("a.foo", foo, 1)]),
    ])
}

#[test]
fn equal_scores_go_to_the_family_with_more_distinct_signals_then_to_the_lower_id() {
    let project = hits_of(&["Foo", "Foo", "Foo", "Bar", "Baz"]);
    let found = &classify(&project, &tied_families()).modules[0][0];
    // 02-b has more signals than 01-a, which has more hits and a lower id. 04-d
    // has as many signals as 02-b, and a lower score.
    let ranked = [("02-b", 3.0), ("01-a", 3.0), ("03-c", 3.0), ("04-d", 2.0)];
    assert_eq!(scores(found), ranked);
}

#[test]
fn lines_that_do_not_fit_the_procedure_never_panic_and_never_cite_line_zero() {
    let foo = signal("a.foo", &["Foo"], 2).at(Scope::Module);
    // A procedure that ends before it starts, one at line 0, hits at line 0.
    let mut backwards = procedure("Backwards", 9, vec![uses(10, "Foo")]);
    backwards.end_line = 3;
    let zero = procedure("Zero", 0, vec![uses(0, "Foo")]);
    let project = project(vec![
        module("Module1", ModuleType::Standard, vec![backwards, zero])
            .with_declarations(vec![uses(0, "Foo"), uses(5, "Foo")]),
    ]);
    let found = classify(&project, &rules_of(vec![foo]));
    for found in found.modules.iter().flatten() {
        let lines = lines_cited(&found.families[0]);
        assert!(!lines.is_empty() && lines.iter().all(|&line| line >= 1));
    }
}
