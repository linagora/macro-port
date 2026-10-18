// SPDX-License-Identifier: Apache-2.0
//! Loading the rules files into typed rules.

use macroport_core::rules::{Matcher, Problem, RuleSet, RulesSources, Scope, TextMode};

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

fn load(text: &str) -> Result<RuleSet, Vec<Problem>> {
    RuleSet::load(&RulesSources { families: text })
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
    let rules = load(&families_yaml(&[
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
    let rules = load(&families_yaml(&[
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
    let problems = load(&families_yaml(&[
        "{ id: fmt.magic, match: structural, pattern: [udf_candidate, magic], weight: 1 }",
        "{ id: fmt.sheet, match: module_type, pattern: sheet, weight: 1 }",
        "{ id: fmt.loop, match: regex, pattern: '(For', weight: 1 }",
    ]))
    .expect_err("the rules are refused");

    let shown: Vec<String> = problems.iter().map(ToString::to_string).collect();
    assert_eq!(
        shown,
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
    let problems = load(&families_yaml(&[
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
