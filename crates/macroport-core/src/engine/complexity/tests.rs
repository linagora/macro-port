// SPDX-License-Identifier: Apache-2.0
//! Tests of the complexity levels and of the traits.

use super::*;
use crate::engine::testing::{line, module, procedure, project, project_of};
use crate::engine::{CodeLine, ParserPredicates, ProcedureInput};
use crate::rules::{ModuleType, RuleSet, RulesSources, StructuralPredicate};

/// The rules shipped in `rules/`.
fn shipped() -> RuleSet {
    RuleSet::load(&RulesSources::EMBEDDED).expect("the shipped rules load")
}

/// Every metric, defined: the conditions of the tests name any of them.
const DEFINITIONS: &str = "{ loc: X, logical_loc: X, procedures: X, cyclomatic: X, \
                           cyclomatic_total: X, max_nesting: X, parameters: X, goto_count: X, \
                           forms: X, declare_count: X, external_dependencies: X }";

/// The levels of the tests that do not look at them.
const MEDIUM: &str = "{ metric: logical_loc, gte: 100 }";
const HIGH: &str = "{ metric: logical_loc, gte: 1000 }";

/// A `complexity.yaml` whose medium and high levels have these conditions and
/// whose traits are these (an id and the rule of the trait), loaded beside the
/// shipped rules.
fn complexity(medium: &str, high: &str, traits: &[(&str, &str)]) -> Complexity {
    let items: Vec<String> = traits
        .iter()
        .map(|(id, rule)| {
            format!("{{ id: {id}, label: {{ en: X, fr: X }}, description: X, {rule} }}")
        })
        .collect();
    let text = format!(
        "version: {}\n\
         file_levels: {{ medium: {{ any: [{medium}] }}, high: {{ any: [{high}] }} }}\n\
         metric_definitions: {DEFINITIONS}\n\
         traits: [{}]\n",
        shipped().version(),
        items.join(", ")
    );
    let sources = RulesSources {
        complexity: &text,
        ..RulesSources::EMBEDDED
    };
    RuleSet::load(&sources).expect("the rules load").complexity
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

/// The traits of the only procedure of a one-module project, whose body is
/// `body`.
fn traits_of(rules: &Complexity, body: &[CodeLine], metrics: &Metrics) -> Vec<String> {
    let project = project_of(body);
    procedure_traits(rules, &Evaluator::new(&project), 0, 0, metrics)
}

/// Rules with one trait, `t`, that holds from `threshold` uses of the names
/// and chains `of` per logical line.
fn ratio(of: &str, threshold: &str) -> Complexity {
    let rule = format!("any: [{{ match: ratio, of: [{of}], per_logical_loc_gte: {threshold} }}]");
    complexity(MEDIUM, HIGH, &[("t", rule.as_str())])
}

/// A statement on the line `number` that uses the member `name`.
fn uses(number: u32, name: &str) -> CodeLine {
    line(number, &format!("Cells.{name}"))
        .member("Cells")
        .member(name)
}

#[test]
fn a_file_is_low_until_a_condition_of_a_level_holds() {
    let rules = complexity(
        "{ metric: logical_loc, gte: 100 }, { metric: procedures, gte: 5 }",
        "{ metric: logical_loc, gte: 1000 }, { metric: forms, gte: 3 }",
        &[],
    );
    let level = |file: Metrics| complexity_level(&rules, &file);

    // A bound holds from its value.
    assert_eq!(level(metrics(99)), Level::Low);
    assert_eq!(level(metrics(100)), Level::Medium);
    assert_eq!(level(metrics(999)), Level::Medium);
    assert_eq!(level(metrics(1000)), Level::High);
    // One condition is enough, whichever metric it bounds.
    let procedures = |count| Metrics {
        procedures: Some(count),
        ..metrics(0)
    };
    assert_eq!(level(procedures(4)), Level::Low);
    assert_eq!(level(procedures(5)), Level::Medium);
    let forms = |count| Metrics {
        forms: Some(count),
        ..metrics(0)
    };
    assert_eq!(level(forms(2)), Level::Low);
    assert_eq!(level(forms(3)), Level::High);
    // The highest level that is reached wins.
    let both = Metrics {
        procedures: Some(5),
        forms: Some(3),
        ..metrics(100)
    };
    assert_eq!(level(both), Level::High);
}

#[test]
fn an_upper_bound_holds_up_to_its_value_and_a_missing_metric_is_zero() {
    let rules = complexity(
        "{ metric: procedures, lte: 2 }",
        "{ metric: forms, gte: 1 }",
        &[],
    );
    let level = |file: Metrics| complexity_level(&rules, &file);
    let procedures = |count| Metrics {
        procedures: Some(count),
        ..metrics(0)
    };

    assert_eq!(level(procedures(2)), Level::Medium);
    assert_eq!(level(procedures(3)), Level::Low);
    // The metrics are counts, and a count of nothing is left out of the report.
    assert_eq!(level(metrics(0)), Level::Medium);
    let one_form = Metrics {
        forms: Some(1),
        ..metrics(0)
    };
    assert_eq!(level(one_form), Level::High);
}

#[test]
fn every_metric_reads_the_field_of_the_report_that_bears_its_name() {
    let report = Metrics {
        loc: 1,
        logical_loc: 2,
        procedures: Some(3),
        cyclomatic: Some(4),
        cyclomatic_total: Some(5),
        max_nesting: Some(6),
        parameters: Some(7),
        goto_count: Some(8),
        forms: Some(9),
        declare_count: Some(10),
        external_dependencies: Some(11),
    };
    let written = serde_json::to_value(&report).expect("the metrics serialise");

    for metric in Metric::ALL {
        let in_report = written[metric.name()].as_u64();
        assert_eq!(
            Some(u64::from(metric_value(&report, metric))),
            in_report,
            "{}",
            metric.name()
        );
    }
}

#[test]
fn a_metric_left_out_of_the_report_is_zero() {
    for metric in Metric::ALL {
        assert_eq!(metric_value(&metrics(0), metric), 0, "{}", metric.name());
    }
}

#[test]
fn the_shipped_levels_read_the_cyclomatic_metric_of_the_report() {
    let rules = shipped().complexity;
    let level = |file: Metrics| complexity_level(&rules, &file);

    assert_eq!(level(metrics(20)), Level::Low);
    assert_eq!(level(metrics(100_000)), Level::High);
    // The highest cyclomatic complexity of the file is `cyclomatic` (#29).
    let complicated = Metrics {
        cyclomatic: Some(1000),
        ..metrics(20)
    };
    assert_eq!(level(complicated), Level::High);
}

#[test]
fn a_trait_on_metrics_bounds_the_metrics_of_the_procedure() {
    let rules = complexity(
        MEDIUM,
        HIGH,
        &[(
            "trivial",
            "all: [{ metric: logical_loc, lte: 3 }, { metric: external_dependencies, lte: 0 }]",
        )],
    );
    let traits = |procedure: &Metrics| traits_of(&rules, &[], procedure);

    assert_eq!(traits(&metrics(3)), ["trivial"]);
    assert!(traits(&metrics(4)).is_empty());
    // A dependency is one too many, and none reported is none.
    let depends = Metrics {
        external_dependencies: Some(1),
        ..metrics(3)
    };
    assert!(traits(&depends).is_empty());
    let alone = Metrics {
        external_dependencies: Some(0),
        ..metrics(3)
    };
    assert_eq!(traits(&alone), ["trivial"]);
}

#[test]
fn any_needs_one_condition_and_all_needs_every_one_in_the_order_of_the_rules() {
    let both = "[{ metric: logical_loc, lte: 3 }, { metric: parameters, gte: 4 }]";
    let rules = complexity(
        MEDIUM,
        HIGH,
        &[
            ("either", &format!("any: {both}")),
            ("both", &format!("all: {both}")),
        ],
    );
    let traits = |logical_loc, parameters| {
        let procedure = Metrics {
            parameters: Some(parameters),
            ..metrics(logical_loc)
        };
        traits_of(&rules, &[], &procedure)
    };

    assert_eq!(traits(3, 4), ["either", "both"]);
    assert_eq!(traits(3, 1), ["either"]);
    assert_eq!(traits(9, 4), ["either"]);
    assert!(traits(9, 1).is_empty());
}

#[test]
fn a_comment_condition_reads_the_comments_of_the_procedure() {
    let rules = complexity(
        MEDIUM,
        HIGH,
        &[("t", "any: [{ match: comment, pattern: 'macro recorded' }]")],
    );
    // The comment of the declarations belongs to no procedure.
    let project = project(vec![
        module(
            "Module1",
            ModuleType::Standard,
            vec![
                procedure(
                    "Recorded",
                    3,
                    vec![line(4, "' Macro recorded 01/02/2003 by Anne")],
                ),
                procedure("Plain", 8, vec![line(9, "x = 1 ' a note")]),
            ],
        )
        .with_declarations(vec![line(1, "' macro recorded, but not in a procedure")]),
    ]);
    let evaluator = Evaluator::new(&project);
    let traits = |index| procedure_traits(&rules, &evaluator, 0, index, &metrics(1));

    assert_eq!(traits(0), ["t"]);
    assert!(traits(1).is_empty());
}

#[test]
fn a_procedure_name_condition_reads_the_name_of_the_procedure() {
    let rules = complexity(
        MEDIUM,
        HIGH,
        &[(
            "t",
            "any: [{ match: procedure_name, pattern: '^Macro\\d+$' }]",
        )],
    );
    let named = ["Macro1", "macro12", "MyMacro1", "Macro", "Macro1b"];
    let procedures = named
        .iter()
        .map(|name| procedure(name, 1, Vec::new()))
        .collect();
    let project = project(vec![module("Module1", ModuleType::Standard, procedures)]);
    let evaluator = Evaluator::new(&project);

    let holds: Vec<bool> = (0..named.len())
        .map(|index| !procedure_traits(&rules, &evaluator, 0, index, &metrics(1)).is_empty())
        .collect();
    assert_eq!(holds, [true, true, false, false, false]);
}

#[test]
fn a_ratio_holds_from_its_threshold() {
    let rules = ratio("Select", "0.3");
    let selects = |count| vec![uses(2, "Select"); count];

    // 3 uses in 10 logical lines are 0.3: the threshold, and an exact one.
    assert_eq!(traits_of(&rules, &selects(3), &metrics(10)), ["t"]);
    assert_eq!(traits_of(&rules, &selects(4), &metrics(10)), ["t"]);
    assert!(traits_of(&rules, &selects(2), &metrics(10)).is_empty());
    assert!(traits_of(&rules, &selects(3), &metrics(11)).is_empty());
    // Nothing is divided by no line, and nothing then reaches the threshold.
    assert!(traits_of(&rules, &selects(3), &metrics(0)).is_empty());

    // 7 uses in 100 lines are 0.07, which a product of the threshold and the
    // lines would miss.
    let rules = ratio("Select", "0.07");
    assert_eq!(traits_of(&rules, &selects(7), &metrics(100)), ["t"]);
    assert!(traits_of(&rules, &selects(7), &metrics(101)).is_empty());
}

#[test]
fn a_ratio_counts_members_by_use_and_chains_by_chain() {
    let rules = ratio("Select, ActiveWindow.ScrollRow", "0.4");
    let body = [
        // One member twice in a statement is two uses.
        line(2, "Cells.Select.Select")
            .member("Cells")
            .member("Select")
            .member("Select"),
        // A name of `of` that has a dot is a chain: its parts do not count alone.
        line(3, "ActiveWindow.ScrollRow = 3")
            .member("ActiveWindow")
            .member("ScrollRow")
            .chain("ActiveWindow.ScrollRow"),
        line(4, "x = ActiveWindow.ScrollColumn")
            .member("ActiveWindow")
            .member("ScrollColumn")
            .chain("ActiveWindow.ScrollColumn"),
    ];

    // Three uses: 3 / 7 reaches 0.4, 3 / 8 does not.
    assert_eq!(traits_of(&rules, &body, &metrics(7)), ["t"]);
    assert!(traits_of(&rules, &body, &metrics(8)).is_empty());

    // The parts of the chain, found apart from the chain, are not the chain.
    let parts = [line(2, "ActiveWindow.ScrollRow")
        .member("ActiveWindow")
        .member("ScrollRow")];
    let chain = ratio("ActiveWindow.ScrollRow", "0.1");
    assert!(traits_of(&chain, &parts, &metrics(5)).is_empty());
}

#[test]
fn a_name_given_to_an_argument_is_no_use() {
    let rules = ratio("Select", "0.1");
    let named = [line(2, "Cells.Sort Select:=1")
        .member("Cells")
        .named_argument("Select")];

    assert!(traits_of(&rules, &named, &metrics(5)).is_empty());
}

/// A procedure that `predicate` holds for and no other predicate does. The
/// match has no wildcard arm: a predicate added to the closed list needs its
/// procedure here.
fn holding(predicate: StructuralPredicate) -> ProcedureInput {
    // The statement at position 1 among the lines of the procedure, which the
    // parser cites for the predicates about statements.
    let body = || vec![line(2, "x = 1")];
    let parsed = |name: &str, predicates: ParserPredicates| ProcedureInput {
        predicates,
        ..procedure(name, 1, body())
    };
    match predicate {
        // The engine decides these three itself, from names and literals.
        StructuralPredicate::EventHandler => procedure("Worksheet_Change", 1, body()),
        StructuralPredicate::AutoExec => procedure("Auto_Open", 1, body()),
        StructuralPredicate::LongHexOrBase64Literal => {
            let literal = "4D5A9000".repeat(25);
            procedure(
                "Payload",
                1,
                vec![line(2, "s = 1").with_strings(&[&literal])],
            )
        }
        StructuralPredicate::UdfCandidate => parsed(
            "Candidate",
            ParserPredicates {
                udf_candidate: true,
                ..ParserPredicates::default()
            },
        ),
        StructuralPredicate::ChrObfuscation => parsed(
            "Obfuscated",
            ParserPredicates {
                chr_obfuscation: vec![1],
                ..ParserPredicates::default()
            },
        ),
        StructuralPredicate::NestedLoopCompare => parsed(
            "Nested",
            ParserPredicates {
                nested_loop_compare: vec![1],
                ..ParserPredicates::default()
            },
        ),
        StructuralPredicate::PossiblyUnused => parsed(
            "Unused",
            ParserPredicates {
                possibly_unused: true,
                ..ParserPredicates::default()
            },
        ),
        StructuralPredicate::ParseRecovered => parsed(
            "Recovered",
            ParserPredicates {
                parse_recovered: true,
                ..ParserPredicates::default()
            },
        ),
    }
}

#[test]
fn every_structural_predicate_is_a_trait_of_the_procedure_and_of_the_file() {
    // One rule per predicate, named like it, and one procedure per predicate in
    // a document module, which handles the events of the worksheets.
    let names = StructuralPredicate::ALL.map(StructuralPredicate::name);
    let texts = names.map(|name| format!("structural: {name}"));
    let pairs: Vec<(&str, &str)> = names
        .into_iter()
        .zip(texts.iter().map(String::as_str))
        .collect();
    let rules = complexity(MEDIUM, HIGH, &pairs);
    let procedures = StructuralPredicate::ALL.map(holding).into_iter().collect();
    let project = project(vec![module("Sheet1", ModuleType::Document, procedures)]);
    let evaluator = Evaluator::new(&project);

    for (index, name) in names.into_iter().enumerate() {
        let traits = procedure_traits(&rules, &evaluator, 0, index, &metrics(1));
        assert_eq!(traits, [name]);
    }
    assert_eq!(file_traits(&rules, &evaluator, &metrics(1)), names);
}

#[test]
fn an_index_out_of_range_has_nothing_to_read_but_the_metrics() {
    let rules = complexity(
        MEDIUM,
        HIGH,
        &[
            ("small", "all: [{ metric: logical_loc, lte: 3 }]"),
            ("recorded", "any: [{ match: comment, pattern: 'macro' }]"),
            ("unused", "structural: possibly_unused"),
            (
                "ratio",
                "any: [{ match: ratio, of: [Cells], per_logical_loc_gte: 0.1 }]",
            ),
        ],
    );
    let project = project_of(&[line(2, "' macro").member("Cells")]);
    let evaluator = Evaluator::new(&project);

    assert_eq!(
        procedure_traits(&rules, &evaluator, 0, 0, &metrics(1)),
        ["small", "recorded", "ratio"]
    );
    for (owner, index) in [(0, 1), (1, 0), (7, 7)] {
        let traits = procedure_traits(&rules, &evaluator, owner, index, &metrics(1));
        assert_eq!(traits, ["small"], "{owner}, {index}");
    }
}

#[test]
fn a_file_trait_bounds_the_metrics_it_is_given() {
    let rules = complexity(
        MEDIUM,
        HIGH,
        &[("trivial", "all: [{ metric: logical_loc, lte: 3 }]")],
    );
    let project = project(vec![module(
        "Module1",
        ModuleType::Standard,
        vec![
            procedure("Tiny", 1, Vec::new()),
            procedure("Long", 5, Vec::new()),
        ],
    )]);
    let evaluator = Evaluator::new(&project);

    assert_eq!(
        procedure_traits(&rules, &evaluator, 0, 0, &metrics(2)),
        ["trivial"]
    );
    assert!(procedure_traits(&rules, &evaluator, 0, 1, &metrics(40)).is_empty());
    // The file has the lines of both.
    assert!(file_traits(&rules, &evaluator, &metrics(42)).is_empty());
    assert_eq!(file_traits(&rules, &evaluator, &metrics(3)), ["trivial"]);
}

#[test]
fn a_file_trait_reads_the_whole_project() {
    let rules = complexity(
        MEDIUM,
        HIGH,
        &[
            (
                "commented",
                "any: [{ match: comment, pattern: 'macro recorded' }]",
            ),
            (
                "named",
                "any: [{ match: procedure_name, pattern: '^Macro\\d+$' }]",
            ),
            ("unused", "structural: possibly_unused"),
        ],
    );
    let mut unused = procedure("Helper", 1, Vec::new());
    unused.predicates.possibly_unused = true;
    // The first module holds none of them: the comment is in the declarations
    // of the second, the name and the flag are in the third.
    let project = project(vec![
        module(
            "Module1",
            ModuleType::Standard,
            vec![procedure("Main", 1, vec![line(2, "x = 1")])],
        ),
        module("Module2", ModuleType::Standard, Vec::new())
            .with_declarations(vec![line(1, "' macro recorded long ago")]),
        module(
            "Module3",
            ModuleType::Standard,
            vec![procedure("Macro7", 1, Vec::new()), unused],
        ),
    ]);
    let evaluator = Evaluator::new(&project);

    assert_eq!(
        file_traits(&rules, &evaluator, &metrics(9)),
        ["commented", "named", "unused"]
    );
    assert!(procedure_traits(&rules, &evaluator, 0, 0, &metrics(9)).is_empty());
}

#[test]
fn a_file_ratio_counts_the_uses_of_every_module_over_the_lines_of_the_file() {
    let rules = ratio("Select", "0.2");
    let project = project(vec![
        module(
            "Module1",
            ModuleType::Standard,
            vec![procedure("First", 1, vec![uses(2, "Select")])],
        ),
        module(
            "Module2",
            ModuleType::Standard,
            vec![procedure("Second", 1, vec![uses(2, "Select")])],
        ),
    ]);
    let evaluator = Evaluator::new(&project);

    // Two uses in the file, one in a procedure.
    assert_eq!(file_traits(&rules, &evaluator, &metrics(10)), ["t"]);
    assert!(file_traits(&rules, &evaluator, &metrics(11)).is_empty());
    assert_eq!(
        procedure_traits(&rules, &evaluator, 0, 0, &metrics(5)),
        ["t"]
    );
    assert!(procedure_traits(&rules, &evaluator, 0, 0, &metrics(6)).is_empty());
}

#[test]
fn the_shipped_traits_apply() {
    let rules = shipped().complexity;
    let mut unused = procedure("Report", 20, vec![uses(21, "Value")]);
    unused.predicates.possibly_unused = true;
    let mut broken = procedure("Broken", 30, Vec::new());
    broken.predicates.parse_recovered = true;
    let project = project(vec![module(
        "Module1",
        ModuleType::Standard,
        vec![
            // The name the macro recorder gives, and three lines.
            procedure("Macro1", 1, vec![line(2, "x = 1")]),
            // The comment the macro recorder writes.
            procedure(
                "Shortcut",
                4,
                vec![line(5, "' Keyboard Shortcut: Ctrl+Shift+A")],
            ),
            // A name of its own, but most of its lines select.
            procedure(
                "Tidy",
                10,
                vec![
                    line(11, "ActiveWindow.ScrollRow = 3")
                        .member("ActiveWindow")
                        .member("ScrollRow")
                        .chain("ActiveWindow.ScrollRow"),
                    uses(12, "Select"),
                    uses(13, "Select"),
                ],
            ),
            unused,
            broken,
        ],
    )]);
    let evaluator = Evaluator::new(&project);
    let traits =
        |index, logical_loc| procedure_traits(&rules, &evaluator, 0, index, &metrics(logical_loc));

    assert_eq!(traits(0, 3), ["recorded_macro", "trivial"]);
    assert_eq!(traits(1, 8), ["recorded_macro"]);
    assert_eq!(traits(2, 5), ["recorded_macro"]);
    assert_eq!(traits(3, 50), ["possibly_unused"]);
    assert_eq!(traits(4, 50), ["compile_error_suspected"]);
}

#[test]
fn a_file_with_no_code_is_trivial() {
    // Whether such a file has traits is for the caller to decide.
    let rules = shipped().complexity;
    let project = project(Vec::new());

    assert_eq!(
        file_traits(&rules, &Evaluator::new(&project), &metrics(0)),
        ["trivial"]
    );
}
