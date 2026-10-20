// SPDX-License-Identifier: Apache-2.0
//! Loading the rules files into typed rules.

use macroport_core::rules::{
    Bound, ChainTerm, Condition, FileLevels, Matcher, MetricCondition, Problem, RuleSet,
    RulesSources, Scope, Severity, StructuralPredicate, TextMode, TraitRule, parse_value,
};

/// A `families.yaml` with one family holding `signals`, one flow mapping per line.
fn families_yaml(signals: &[&str]) -> String {
    let mut text = String::from(
        "version: 0.1.0\n\
         scoring: { min_score: 3, secondary_ratio: 0.6, default_max_hits_per_procedure: 3 }\n\
         families:\n  \
           - id: 01-formatting\n    \
             corpus_slug: 01-mise-en-forme\n    \
             label: { en: Formatting, fr: Mise en forme }\n    \
             signals:\n",
    );
    for signal in signals {
        text.push_str("      - ");
        text.push_str(signal);
        text.push('\n');
    }
    text
}

/// Loads `text` as `families.yaml`, beside a `security.yaml` that holds no
/// rule and a `complexity.yaml` that holds no trait.
fn load_families(text: &str) -> Result<RuleSet, Vec<Problem>> {
    RuleSet::load(&RulesSources {
        families: text,
        security: &security_yaml(&[], &[], &[]),
        complexity: &complexity_yaml(MEDIUM, HIGH, &[]),
    })
}

/// A `security.yaml` with these signals, chains and container indicators,
/// one flow mapping per line from line 3.
fn security_yaml(signals: &[String], chains: &[String], indicators: &[String]) -> String {
    let mut text = String::from("version: 0.1.0\n");
    for (key, items) in [
        ("signals", signals),
        ("chains", chains),
        ("container_indicators", indicators),
    ] {
        text.push_str(key);
        text.push_str(if items.is_empty() { ": []\n" } else { ":\n" });
        for item in items {
            text.push_str("  - ");
            text.push_str(item);
            text.push('\n');
        }
    }
    text
}

/// A security signal with the keys that the test does not care about.
fn signal(id: &str, keys: &str) -> String {
    format!("{{ id: {id}, category: test, severity: low, label: {{ en: X, fr: X }}, {keys} }}")
}

/// A chain of the terms `all`.
fn chain(id: &str, all: &str) -> String {
    format!("{{ id: {id}, all: {all}, severity: high, label: {{ en: X, fr: X }} }}")
}

fn indicator(id: &str) -> String {
    format!("{{ id: {id}, severity: high, label: {{ en: X, fr: X }} }}")
}

/// Loads `text` as `security.yaml`, beside a `families.yaml` whose only
/// family is `01-formatting` and a `complexity.yaml` that holds no trait.
fn load_security(text: &str) -> Result<RuleSet, Vec<Problem>> {
    RuleSet::load(&RulesSources {
        families: &families_yaml(&["{ id: fmt.a, match: member, pattern: [A], weight: 1 }"]),
        security: text,
        complexity: &complexity_yaml(MEDIUM, HIGH, &[]),
    })
}

/// A `complexity.yaml` whose metrics are `logical_loc` and `procedures`,
/// with these `medium` and `high` conditions and these traits, one flow
/// mapping per line from line 7.
fn complexity_yaml(medium: &str, high: &str, traits: &[String]) -> String {
    let mut text = format!(
        "version: 0.1.0\n\
         file_levels:\n  \
           medium: {{ any: {medium} }}\n  \
           high: {{ any: {high} }}\n\
         metric_definitions: {{ logical_loc: Lines, procedures: Procedures }}\n"
    );
    text.push_str(if traits.is_empty() {
        "traits: []\n"
    } else {
        "traits:\n"
    });
    for item in traits {
        text.push_str("  - ");
        text.push_str(item);
        text.push('\n');
    }
    text
}

/// The level conditions of the tests that do not check them.
const MEDIUM: &str = "[{ metric: logical_loc, gte: 150 }]";
const HIGH: &str = "[{ metric: procedures, gte: 50 }]";

/// A trait with the keys that the test does not care about.
fn complexity_trait(id: &str, keys: &str) -> String {
    format!("{{ id: {id}, label: {{ en: X, fr: X }}, description: X, {keys} }}")
}

/// Loads `text` as `complexity.yaml`, beside a `families.yaml` whose only
/// family is `01-formatting` and a `security.yaml` that holds no rule.
fn load_complexity(text: &str) -> Result<RuleSet, Vec<Problem>> {
    RuleSet::load(&RulesSources {
        families: &families_yaml(&["{ id: fmt.a, match: member, pattern: [A], weight: 1 }"]),
        security: &security_yaml(&[], &[], &[]),
        complexity: text,
    })
}

fn shown(problems: &[Problem]) -> Vec<String> {
    problems.iter().map(ToString::to_string).collect()
}

#[test]
fn embedded_rules_load() {
    let rules = RuleSet::load(&RulesSources::EMBEDDED).expect("the shipped rules load");

    let ids: Vec<&str> = rules
        .families
        .families
        .iter()
        .map(|family| family.id.as_str())
        .collect();
    assert_eq!(
        ids,
        [
            "01-formatting",
            "02-consolidation",
            "03-cleaning",
            "04-lookup",
            "05-forms",
            "06-reporting",
            "07-pivots-charts",
            "08-udf",
            "09-events",
            "10-files",
            "11-external-data",
            "12-office-interop",
            "13-windows-api",
        ]
    );
}

#[test]
fn regexes_ignore_case_unless_an_inline_flag_says_otherwise() {
    let rules = load_families(&families_yaml(&[
        r"{ id: fmt.loop, match: regex, pattern: '\bFor\s+Each\s+\w+\s+In\s+Worksheets\b', weight: 1 }",
        "{ id: fmt.style, match: string, pattern: ['(?-i)^Normal$', '(?i)^heading'], weight: 1 }",
    ]))
    .expect("the rules load");
    let signals = &rules.families.families[0].signals;

    let Matcher::Regex(loop_regexes) = &signals[0].matcher else {
        panic!("a regex matcher: {:?}", signals[0].matcher);
    };
    assert!(loop_regexes[0].is_match("for each ws in worksheets"));

    let Matcher::String(style_regexes) = &signals[1].matcher else {
        panic!("a string matcher: {:?}", signals[1].matcher);
    };
    assert!(style_regexes[0].is_match("Normal"));
    assert!(!style_regexes[0].is_match("normal"));
    assert!(style_regexes[1].is_match("HEADING 1"));
}

#[test]
fn omitted_keys_take_their_defaults() {
    let rules = load_families(&families_yaml(&[
        "{ id: fmt.cell-style, match: member, pattern: [Interior], weight: 1 }",
        "{ id: fmt.sheets, match: chain, pattern: Sheets.Add, weight: 2, \
           max_hits_per_procedure: 1, scope: module, text: raw }",
    ]))
    .expect("the rules load");
    let signals = &rules.families.families[0].signals;

    // The cap comes from scoring.default_max_hits_per_procedure.
    assert_eq!(signals[0].max_hits_per_procedure, 3);
    assert_eq!(signals[0].scope, Scope::Procedure);
    assert_eq!(signals[0].text, TextMode::Code);
    assert_eq!(signals[0].label, None);

    assert_eq!(signals[1].max_hits_per_procedure, 1);
    assert_eq!(signals[1].scope, Scope::Module);
    assert_eq!(signals[1].text, TextMode::Raw);
}

#[test]
fn every_invalid_pattern_is_a_problem() {
    let problems = load_families(&families_yaml(&[
        "{ id: fmt.magic, match: structural, pattern: [udf_candidate, magic], weight: 1 }",
        "{ id: fmt.sheet, match: module_type, pattern: sheet, weight: 1 }",
        "{ id: fmt.loop, match: regex, pattern: '(For', weight: 1 }",
    ]))
    .expect_err("the rules are refused");

    assert_eq!(
        shown(&problems),
        [
            "families.yaml: fmt.magic: unknown structural predicate 'magic', expected one of: \
             udf_candidate, event_handler, auto_exec, chr_obfuscation, \
             long_hex_or_base64_literal, nested_loop_compare, possibly_unused, parse_recovered \
             (at /families/0/signals/0/pattern/1)",
            "families.yaml: fmt.sheet: unknown module type 'sheet', expected one of: standard, \
             class, document, form (at /families/0/signals/1/pattern)",
            "families.yaml: fmt.loop: invalid regex '(For': unclosed group \
             (at /families/0/signals/2/pattern)",
        ]
    );
}

#[test]
fn yaml_errors_give_their_position() {
    let problems = load_families(&families_yaml(&[
        "{ id: fmt.cell-style, match: member, pattern: [Interior], weight: 1, weight: 2 }",
    ]))
    .expect_err("the rules are refused");

    assert_eq!(
        problems,
        [Problem {
            file: "families.yaml",
            rule: None,
            location: String::new(),
            // The second `weight` key.
            message: "duplicate mapping key: weight at line 8, column 78".to_owned(),
        }]
    );
}

#[test]
fn whole_numbers_load_however_yaml_spells_them() {
    // The schemas' `integer` accepts 2.0, 1e1, 010 and 0x10: a file that
    // passes its schema loads with the same values.
    let text = families_yaml(&[
        "{ id: fmt.a, match: member, pattern: [A], weight: 3.0, max_hits_per_procedure: 1e1 }",
        "{ id: fmt.b, match: member, pattern: [B], weight: 010, max_hits_per_procedure: 0x10 }",
        "{ id: fmt.c, match: member, pattern: [C], weight: 1 }",
    ])
    .replace(
        "default_max_hits_per_procedure: 3 }",
        "default_max_hits_per_procedure: 2.0 }",
    );
    let rules = load_families(&text).expect("the rules load");

    let numbers: Vec<(u32, u32)> = rules.families.families[0]
        .signals
        .iter()
        .map(|signal| (signal.weight, signal.max_hits_per_procedure))
        .collect();
    assert_eq!(numbers, [(3, 10), (10, 16), (1, 2)]);
}

#[test]
fn weights_are_whole_numbers_that_fit_32_bits() {
    let refusals: Vec<String> = ["2.5", "-1", "4294967296", "'3'", "~"]
        .iter()
        .map(|weight| {
            let signal = format!("{{ id: fmt.a, match: member, pattern: [A], weight: {weight} }}");
            let problems = load_families(&families_yaml(&[&signal])).expect_err(weight);
            assert_eq!(problems.len(), 1, "{problems:?}");
            problems[0].message.clone()
        })
        .collect();

    // Each position is the `weight` key's.
    assert_eq!(
        refusals,
        [
            "invalid value: floating point `2.5`, expected a whole number at line 8, column 51",
            "invalid value: integer `-1`, expected a whole number at line 8, column 51",
            "invalid value: integer `4294967296`, expected a whole number at line 8, column 51",
            "invalid type: string \"3\", expected a whole number at line 8, column 51",
            "invalid type: unit value, expected a whole number at line 8, column 51",
        ]
    );
}

#[test]
fn yaml_1_1_booleans_stay_strings() {
    let rules = load_families(&families_yaml(&[
        "{ id: fmt.words, match: member, pattern: [yes, no, on, off, y, n], weight: 1 }",
    ]))
    .expect("the rules load");

    let matcher = &rules.families.families[0].signals[0].matcher;
    let Matcher::Member(names) = matcher else {
        panic!("a member matcher: {matcher:?}");
    };
    assert_eq!(names, &["yes", "no", "on", "off", "y", "n"]);
    assert_eq!(
        parse_value("pattern: [yes, no, on, off, y, n]"),
        Ok(serde_json::json!({ "pattern": ["yes", "no", "on", "off", "y", "n"] }))
    );
}

#[test]
fn custom_tags_are_refused() {
    let problems = load_families(&families_yaml(&[
        "{ id: fmt.a, match: member, pattern: !include more.yaml, weight: 1 }",
    ]))
    .expect_err("the rules are refused");

    assert_eq!(
        problems,
        [Problem::in_file(
            "families.yaml",
            // The tagged scalar.
            "unsupported tag `!include` at line 8, column 55".to_owned()
        )]
    );
}

#[test]
fn alias_bombs_are_refused() {
    // Ten levels of ten aliases: ten billion nodes once expanded.
    let mut bomb = String::from("a0: &a0 [x, x, x, x, x, x, x, x, x, x]\n");
    for (previous, level) in (0..9).zip(1..10) {
        let aliases = vec![format!("*a{previous}"); 10].join(", ");
        bomb.push_str(&format!("a{level}: &a{level} [{aliases}]\n"));
    }

    let refusal = parse_value(&bomb).expect_err("the document is refused");
    assert!(refusal.starts_with("budget breached"), "{refusal}");
}

#[test]
fn a_second_document_is_refused() {
    let text = families_yaml(&["{ id: fmt.a, match: member, pattern: [A], weight: 1 }"])
        + "---\nversion: 0.2.0\n";

    assert_eq!(
        load_families(&text).expect_err("the rules are refused"),
        [Problem::in_file(
            "families.yaml",
            // The first line of the second document.
            "multiple YAML documents detected at line 10, column 1".to_owned()
        )]
    );
}

#[test]
fn embedded_security_rules_load() {
    let rules = RuleSet::load(&RulesSources::EMBEDDED).expect("the shipped rules load");
    let security = &rules.security;

    let chains: Vec<&str> = security
        .chains
        .iter()
        .map(|chain| chain.id.as_str())
        .collect();
    assert_eq!(
        chains,
        [
            "sec.chain.autoexec-execution",
            "sec.chain.download-write",
            "sec.chain.obfuscated-execution",
        ]
    );
    assert_eq!(
        security.chains[0].all,
        [
            ChainTerm::Signal("sec.auto-exec".to_owned()),
            ChainTerm::Any(vec![
                ChainTerm::Signal("sec.shell".to_owned()),
                ChainTerm::Signal("sec.wscript-run".to_owned()),
                ChainTerm::Signal("sec.shell-execute-api".to_owned()),
            ]),
        ]
    );
    let indicators: Vec<(&str, Severity)> = security
        .container_indicators
        .iter()
        .map(|indicator| (indicator.id.as_str(), indicator.severity))
        .collect();
    assert_eq!(
        indicators,
        [
            ("sec.source-missing", Severity::High),
            ("sec.xlm-macrosheet", Severity::Medium),
            ("sec.vbaproject-in-xlsx", Severity::High),
            ("sec.suspicious-archive", Severity::Medium),
        ]
    );
}

#[test]
fn security_signals_take_their_defaults() {
    let rules = load_security(&security_yaml(
        &[
            signal("sec.a", "match: call, pattern: [Shell]"),
            signal(
                "sec.b",
                "match: regex, pattern: '\\.Send\\b', scope: module, text: raw, \
                 requires_family: 01-formatting, note: ignored",
            ),
        ],
        &[],
        &[],
    ))
    .expect("the rules load");
    let signals = &rules.security.signals;

    assert_eq!(signals[0].scope, Scope::Procedure);
    assert_eq!(signals[0].text, TextMode::Code);
    assert_eq!(signals[0].requires_family, None);

    assert_eq!(signals[1].scope, Scope::Module);
    assert_eq!(signals[1].text, TextMode::Raw);
    assert_eq!(signals[1].requires_family.as_deref(), Some("01-formatting"));
}

#[test]
fn ids_are_unique_across_signals_chains_and_container_indicators() {
    let call = "match: call, pattern: [Shell]";
    let problems = load_security(&security_yaml(
        &[
            signal("sec.a", call),
            signal("sec.a", call),
            signal("sec.c", call),
        ],
        &[chain("sec.a", "[sec.a]"), chain("sec.b", "[sec.a]")],
        &[indicator("sec.b"), indicator("sec.c")],
    ))
    .expect_err("the rules are refused");

    assert_eq!(
        shown(&problems),
        [
            "security.yaml: sec.a: duplicate id, first defined at /signals/0 (at /signals/1/id)",
            "security.yaml: sec.a: duplicate id, first defined at /signals/0 (at /chains/0/id)",
            "security.yaml: sec.b: duplicate id, first defined at /chains/1 \
             (at /container_indicators/0/id)",
            "security.yaml: sec.c: duplicate id, first defined at /signals/2 \
             (at /container_indicators/1/id)",
        ]
    );
}

#[test]
fn chain_terms_name_signals() {
    let problems = load_security(&security_yaml(
        &[
            signal("sec.a", "match: call, pattern: [Shell]"),
            signal("sec.broken", "match: regex, pattern: '(x'"),
        ],
        &[chain(
            "sec.chain",
            "[sec.a, sec.nope, { any: [sec.broken, { any: [sec.source-missing] }] }]",
        )],
        &[indicator("sec.source-missing")],
    ))
    .expect_err("the rules are refused");

    // A signal with an invalid pattern still exists; a container indicator
    // is not a signal.
    assert_eq!(
        shown(&problems),
        [
            "security.yaml: sec.broken: invalid regex '(x': unclosed group (at /signals/1/pattern)",
            "security.yaml: sec.chain: unknown signal 'sec.nope' (at /chains/0/all/1)",
            "security.yaml: sec.chain: unknown signal 'sec.source-missing' \
             (at /chains/0/all/2/any/1/any/0)",
        ]
    );
}

#[test]
fn required_families_exist() {
    let problems = load_security(&security_yaml(
        &[
            signal(
                "sec.a",
                "match: call, pattern: [Shell], requires_family: 01-formatting",
            ),
            signal(
                "sec.b",
                "match: call, pattern: [Shell], requires_family: 01-mise-en-forme",
            ),
        ],
        &[],
        &[],
    ))
    .expect_err("the rules are refused");

    // A corpus slug is not a family id.
    assert_eq!(
        shown(&problems),
        ["security.yaml: sec.b: unknown family '01-mise-en-forme' (at /signals/1/requires_family)"]
    );
}

#[test]
fn required_families_are_checked_against_families_with_problems() {
    let problems = RuleSet::load(&RulesSources {
        families: &families_yaml(&["{ id: fmt.loop, match: regex, pattern: '(For', weight: 1 }"]),
        security: &security_yaml(
            &[
                signal(
                    "sec.a",
                    "match: call, pattern: [Shell], requires_family: 01-formatting",
                ),
                signal(
                    "sec.b",
                    "match: call, pattern: [Shell], requires_family: 01-mise-en-forme",
                ),
            ],
            &[],
            &[],
        ),
        complexity: &complexity_yaml(MEDIUM, HIGH, &[]),
    })
    .expect_err("the rules are refused");

    // 01-formatting exists, though one of its signals is invalid.
    assert_eq!(
        shown(&problems),
        [
            "families.yaml: fmt.loop: invalid regex '(For': unclosed group \
             (at /families/0/signals/0/pattern)",
            "security.yaml: sec.b: unknown family '01-mise-en-forme' (at /signals/1/requires_family)",
        ]
    );
}

#[test]
fn required_families_wait_for_families_yaml_to_parse() {
    let problems = RuleSet::load(&RulesSources {
        families: "version: 0.1.0\n",
        security: &security_yaml(
            &[signal(
                "sec.a",
                "match: call, pattern: [Shell], requires_family: 01-formatting",
            )],
            &[],
            &[],
        ),
        complexity: &complexity_yaml(MEDIUM, HIGH, &[]),
    })
    .expect_err("the rules are refused");

    // Whether 01-formatting exists is unknown: no problem about it.
    assert_eq!(
        shown(&problems),
        ["families.yaml: missing field `scoring` at line 1, column 1"]
    );
}

#[test]
fn malformed_security_rules_are_refused() {
    let call = "match: call, pattern: [Shell]";
    let one_signal = |keys: &str| security_yaml(&[signal("sec.a", keys)], &[], &[]);
    let one_chain =
        |all: &str| security_yaml(&[signal("sec.a", call)], &[chain("sec.chain", all)], &[]);
    let texts = [
        one_signal(&format!("{call}, weight: 1")),
        one_signal(&format!("{call}, max_hits_per_procedure: 3")),
        one_signal("match: membre, pattern: [Shell]"),
        one_signal(&format!("{call}, scope: workbook")),
        one_signal(call).replace("severity: low", "severity: critical"),
        one_signal("match: structural, pattern: [magic]"),
        one_signal(call).replace("{ en: X, fr: X }", "{ en: X }"),
        one_chain("[sec.a]").replace("high, label: { en: X, fr: X }", "high, label: { en: X }"),
        security_yaml(&[], &[], &[indicator("sec.i")]).replace("{ en: X, fr: X }", "{ fr: X }"),
        one_chain("[{ anyy: [sec.a] }]"),
        one_chain("[]"),
        one_chain("[sec.a, { any: [] }]"),
    ];

    let refusals: Vec<String> = texts
        .iter()
        .map(|text| {
            let problems = load_security(text).expect_err(text);
            assert_eq!(problems.len(), 1, "{problems:?}");
            problems[0].to_string()
        })
        .collect();

    // Each position is that of the value or key at fault, or of the first
    // key of the mapping that misses one.
    assert_eq!(
        refusals,
        [
            "security.yaml: unknown field `weight`, expected one of id, category, match, \
             pattern, severity, scope, text, requires_family, label, note at line 3, column 105",
            "security.yaml: unknown field `max_hits_per_procedure`, expected one of id, category, \
             match, pattern, severity, scope, text, requires_family, label, note \
             at line 3, column 105",
            "security.yaml: unknown variant `membre`, expected one of member, chain, call, \
             progid, procedure_name, module_type, reference, regex, string, comment, structural \
             at line 3, column 81",
            "security.yaml: unknown variant `workbook`, expected one of procedure, module, \
             project at line 3, column 112",
            "security.yaml: unknown variant `critical`, expected one of info, low, medium, high \
             at line 3, column 44",
            "security.yaml: sec.a: unknown structural predicate 'magic', expected one of: \
             udf_candidate, event_handler, auto_exec, chr_obfuscation, \
             long_hex_or_base64_literal, nested_loop_compare, possibly_unused, parse_recovered \
             (at /signals/0/pattern/0)",
            "security.yaml: missing field `fr` at line 3, column 58",
            "security.yaml: missing field `fr` at line 5, column 61",
            "security.yaml: missing field `en` at line 5, column 43",
            "security.yaml: invalid chain term, expected a signal id or { any: [terms] } \
             at line 5, column 28",
            "security.yaml: sec.chain: empty list of terms (at /chains/0/all)",
            "security.yaml: sec.chain: empty list of terms (at /chains/0/all/1/any)",
        ]
    );
}

#[test]
fn deeply_nested_chain_terms_are_refused() {
    // Each `{ any: [...] }` nests two collections: 40 of them go past the
    // depth budget of 64.
    let mut term = String::from("sec.a");
    for _ in 0..40 {
        term = format!("{{ any: [{term}] }}");
    }
    let problems = load_security(&security_yaml(
        &[signal("sec.a", "match: call, pattern: [Shell]")],
        &[chain("sec.chain", &format!("[{term}]"))],
        &[],
    ))
    .expect_err("the rules are refused");

    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(
        problems[0].message.starts_with("budget breached: Depth"),
        "{problems:?}"
    );
}

#[test]
fn problems_of_every_file_are_reported_together() {
    let problems = RuleSet::load(&RulesSources {
        families: &families_yaml(&["{ id: fmt.loop, match: regex, pattern: '(For', weight: 1 }"]),
        security: &security_yaml(
            &[signal("sec.loop", "match: regex, pattern: '(For'")],
            &[],
            &[],
        ),
        complexity: &complexity_yaml("[{ metric: lines, gte: 1 }]", HIGH, &[]),
    })
    .expect_err("the rules are refused");

    assert_eq!(
        shown(&problems),
        [
            "families.yaml: fmt.loop: invalid regex '(For': unclosed group \
             (at /families/0/signals/0/pattern)",
            "security.yaml: sec.loop: invalid regex '(For': unclosed group \
             (at /signals/0/pattern)",
            "complexity.yaml: metric 'lines' is not defined in metric_definitions \
             (at /file_levels/medium/any/0/metric)",
        ]
    );
}

#[test]
fn embedded_complexity_rules_load() {
    let rules = RuleSet::load(&RulesSources::EMBEDDED).expect("the shipped rules load");
    let complexity = &rules.complexity;

    assert_eq!(complexity.file_levels.medium.len(), 6);
    assert_eq!(
        complexity.file_levels.high.last(),
        Some(&MetricCondition {
            metric: "declare_count".to_owned(),
            bound: Bound::AtLeast(1),
        })
    );
    let traits: Vec<&str> = complexity
        .traits
        .iter()
        .map(|item| item.id.as_str())
        .collect();
    assert_eq!(
        traits,
        [
            "recorded_macro",
            "trivial",
            "possibly_unused",
            "compile_error_suspected",
        ]
    );
    let TraitRule::All(trivial) = &complexity.traits[1].rule else {
        panic!("all conditions: {:?}", complexity.traits[1].rule);
    };
    assert!(
        matches!(
            &trivial[1],
            Condition::Metric(MetricCondition { metric, bound: Bound::AtMost(0) })
                if metric == "external_dependencies"
        ),
        "{trivial:?}"
    );
    assert!(
        matches!(
            complexity.traits[3].rule,
            TraitRule::Structural(StructuralPredicate::ParseRecovered)
        ),
        "{:?}",
        complexity.traits[3].rule
    );
}

#[test]
fn trait_conditions_load_in_every_form() {
    let rules = load_complexity(&complexity_yaml(
        MEDIUM,
        HIGH,
        &[complexity_trait(
            "t",
            "any: [{ metric: logical_loc, lte: 3.0 }, \
                   { match: comment, pattern: 'macro recorded' }, \
                   { match: procedure_name, pattern: ['^Macro\\d+$', '(?-i)^Test'] }, \
                   { match: ratio, of: [Select, ActiveWindow.ScrollRow], per_logical_loc_gte: 0.3 }]",
        )],
    ))
    .expect("the rules load");

    let TraitRule::Any(conditions) = &rules.complexity.traits[0].rule else {
        panic!("any condition: {:?}", rules.complexity.traits[0].rule);
    };
    let [
        Condition::Metric(metric),
        Condition::Comment(comments),
        Condition::ProcedureName(names),
        Condition::Ratio {
            of,
            per_logical_loc_gte,
        },
    ] = conditions.as_slice()
    else {
        panic!("the four forms: {conditions:?}");
    };
    // Thresholds are whole numbers however YAML spells them, as in families.yaml.
    assert_eq!(
        metric,
        &MetricCondition {
            metric: "logical_loc".to_owned(),
            bound: Bound::AtMost(3),
        }
    );
    assert!(comments[0].is_match("Macro Recorded 01/02/2003"));
    assert!(names[0].is_match("macro12"));
    assert!(!names[1].is_match("test"));
    assert_eq!(of, &["Select", "ActiveWindow.ScrollRow"]);
    assert!((per_logical_loc_gte - 0.3).abs() < f64::EPSILON);
}

#[test]
fn level_thresholds_are_whole_numbers_however_yaml_spells_them() {
    let rules = load_complexity(&complexity_yaml(
        "[{ metric: logical_loc, gte: 1.5e2 }]",
        "[{ metric: procedures, lte: 0x10 }]",
        &[],
    ))
    .expect("the rules load");

    assert_eq!(
        rules.complexity.file_levels,
        FileLevels {
            medium: vec![MetricCondition {
                metric: "logical_loc".to_owned(),
                bound: Bound::AtLeast(150),
            }],
            high: vec![MetricCondition {
                metric: "procedures".to_owned(),
                bound: Bound::AtMost(16),
            }],
        }
    );
}

#[test]
fn metrics_are_those_of_metric_definitions() {
    let problems = load_complexity(&complexity_yaml(
        MEDIUM,
        "[{ metric: procedures, gte: 50 }, { metric: max_cyclomatic, gte: 25 }]",
        &[complexity_trait(
            "t",
            "all: [{ metric: logical_loc, lte: 3 }, { metric: loc, lte: 3 }]",
        )],
    ))
    .expect_err("the rules are refused");

    assert_eq!(
        shown(&problems),
        [
            "complexity.yaml: metric 'max_cyclomatic' is not defined in metric_definitions \
             (at /file_levels/high/any/1/metric)",
            "complexity.yaml: t: metric 'loc' is not defined in metric_definitions \
             (at /traits/0/all/1/metric)",
        ]
    );
}

#[test]
fn metric_conditions_take_exactly_one_bound() {
    let problems = load_complexity(&complexity_yaml(
        "[{ metric: logical_loc }]",
        "[{ metric: procedures, gte: 1, lte: 3 }]",
        &[complexity_trait("t", "all: [{ metric: logical_loc }]")],
    ))
    .expect_err("the rules are refused");

    assert_eq!(
        shown(&problems),
        [
            "complexity.yaml: needs exactly one of: gte, lte (at /file_levels/medium/any/0)",
            "complexity.yaml: needs exactly one of: gte, lte (at /file_levels/high/any/0)",
            "complexity.yaml: t: needs exactly one of: gte, lte (at /traits/0/all/0)",
        ]
    );
}

#[test]
fn traits_take_exactly_one_rule() {
    let problems = load_complexity(&complexity_yaml(
        MEDIUM,
        HIGH,
        &[
            "{ id: t.none, label: { en: X, fr: X }, description: X }".to_owned(),
            complexity_trait(
                "t.two",
                "any: [{ metric: logical_loc, lte: 3 }], structural: possibly_unused",
            ),
            complexity_trait(
                "t.three",
                "any: [{ metric: logical_loc, lte: 3 }], all: [{ metric: loc, lte: 3 }], \
                 structural: possibly_unused",
            ),
        ],
    ))
    .expect_err("the rules are refused");

    // The lists are checked all the same.
    assert_eq!(
        shown(&problems),
        [
            "complexity.yaml: t.none: needs exactly one of: any, all, structural (at /traits/0)",
            "complexity.yaml: t.two: needs exactly one of: any, all, structural (at /traits/1)",
            "complexity.yaml: t.three: needs exactly one of: any, all, structural (at /traits/2)",
            "complexity.yaml: t.three: metric 'loc' is not defined in metric_definitions \
             (at /traits/2/all/0/metric)",
        ]
    );
}

#[test]
fn condition_lists_are_never_empty() {
    let problems = load_complexity(&complexity_yaml(
        "[]",
        HIGH,
        &[
            complexity_trait("t.any", "any: []"),
            complexity_trait("t.all", "all: []"),
        ],
    ))
    .expect_err("the rules are refused");

    // An empty `all` would give the trait to every procedure.
    assert_eq!(
        shown(&problems),
        [
            "complexity.yaml: empty list of conditions (at /file_levels/medium/any)",
            "complexity.yaml: t.any: empty list of conditions (at /traits/0/any)",
            "complexity.yaml: t.all: empty list of conditions (at /traits/1/all)",
        ]
    );
}

#[test]
fn trait_ids_are_unique() {
    let rule = "structural: possibly_unused";
    let problems = load_complexity(&complexity_yaml(
        MEDIUM,
        HIGH,
        &[
            complexity_trait("t.a", rule),
            complexity_trait("t.b", rule),
            complexity_trait("t.a", rule),
        ],
    ))
    .expect_err("the rules are refused");

    assert_eq!(
        shown(&problems),
        ["complexity.yaml: t.a: duplicate trait id, first defined at /traits/0 (at /traits/2/id)"]
    );
}

#[test]
fn invalid_trait_patterns_are_problems() {
    let problems = load_complexity(&complexity_yaml(
        MEDIUM,
        HIGH,
        &[
            complexity_trait(
                "t.regex",
                "any: [{ match: comment, pattern: '(macro' }, \
                       { match: procedure_name, pattern: ['^Macro', '[Macro'] }]",
            ),
            complexity_trait("t.structural", "structural: unused"),
        ],
    ))
    .expect_err("the rules are refused");

    assert_eq!(
        shown(&problems),
        [
            "complexity.yaml: t.regex: invalid regex '(macro': unclosed group \
             (at /traits/0/any/0/pattern)",
            "complexity.yaml: t.regex: invalid regex '[Macro': unclosed character class \
             (at /traits/0/any/1/pattern/1)",
            "complexity.yaml: t.structural: unknown structural predicate 'unused', expected one \
             of: udf_candidate, event_handler, auto_exec, chr_obfuscation, \
             long_hex_or_base64_literal, nested_loop_compare, possibly_unused, parse_recovered \
             (at /traits/1/structural)",
        ]
    );
}

#[test]
fn malformed_complexity_rules_are_refused() {
    let any = "any: [{ metric: logical_loc, lte: 3 }]";
    let no_trait = complexity_yaml(MEDIUM, HIGH, &[]);
    let one_trait = |keys: &str| complexity_yaml(MEDIUM, HIGH, &[complexity_trait("t", keys)]);
    let texts = [
        no_trait.replace("traits: []", "traits: []\nthresholds: {}"),
        no_trait.replace(
            "metric_definitions: { logical_loc: Lines, procedures: Procedures }\n",
            "",
        ),
        no_trait.replace("  high:", "  low: { any: [] }\n  high:"),
        complexity_yaml("[{ metric: logical_loc, gte: 150, note: x }]", HIGH, &[]),
        complexity_yaml("[{ metric: logical_loc, gte: -1 }]", HIGH, &[]),
        one_trait(&format!("{any}, weight: 1")),
        one_trait("any: [{ match: membre, pattern: x }]"),
        one_trait("any: [{ metric: logical_loc, lte: 3, note: x }]"),
        one_trait(any).replace("{ en: X, fr: X }", "{ en: X }"),
    ];

    let refusals: Vec<String> = texts
        .iter()
        .map(|text| {
            let problems = load_complexity(text).expect_err(text);
            assert_eq!(problems.len(), 1, "{problems:?}");
            problems[0].to_string()
        })
        .collect();

    // Each position is that of the value or key at fault, of the condition
    // that has none of the three forms, or of the last key of the mapping
    // that misses one.
    let condition = "complexity.yaml: invalid condition, expected { metric, gte | lte }, \
                     { match: comment | procedure_name, pattern } \
                     or { match: ratio, of, per_logical_loc_gte } at line 7, column 61";
    assert_eq!(
        refusals,
        [
            "complexity.yaml: unknown field `thresholds`, expected one of version, file_levels, \
             metric_definitions, traits at line 7, column 1",
            "complexity.yaml: missing field `metric_definitions` at line 5, column 1",
            "complexity.yaml: unknown field `low`, expected one of medium, high \
             at line 4, column 3",
            "complexity.yaml: unknown field `note`, expected one of metric, gte, lte \
             at line 3, column 52",
            "complexity.yaml: invalid value: integer `-1`, expected a whole number \
             at line 3, column 42",
            "complexity.yaml: unknown field `weight`, expected one of id, label, description, \
             any, all, structural at line 7, column 95",
            condition,
            condition,
            "complexity.yaml: missing field `fr` at line 7, column 23",
        ]
    );
}
