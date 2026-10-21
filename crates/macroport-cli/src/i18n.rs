// SPDX-License-Identifier: Apache-2.0
//! The words of the reports, in English and French.
//!
//! The vocabulary is Fluent: `i18n/en/*.ftl` and `i18n/fr/*.ftl`, embedded at
//! build time. The labels of families, signals and traits are not Fluent
//! messages but the `label` fields of the rules, which hold both languages;
//! `unclassified`, which no rule defines, is the one family that has a message.
//! No string from an analysed file goes through Fluent: a renderer prints those
//! as they are, escaped for its format, next to the words that [`Messages`]
//! gives.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use fluent_bundle::{FluentBundle, FluentResource};
use macroport_core::report::{Container, Lang, Level, ProcedureKind, Status, Visibility};
use macroport_core::rules::{Label, ModuleType, RuleSet, Severity};
use serde::Serialize;
use unic_langid::LanguageIdentifier;

use crate::wire::wire_name;

/// The id of the family of what no family claims, as the schema writes it.
const UNCLASSIFIED: &str = "unclassified";

/// One Fluent file, in both languages.
///
/// The two texts are embedded together, so a file cannot exist in one language
/// only.
#[derive(Debug, Clone, Copy)]
pub struct Resource {
    /// The name of the file, the same in `i18n/en/` and `i18n/fr/`.
    pub name: &'static str,
    /// Its English text.
    pub en: &'static str,
    /// Its French text.
    pub fr: &'static str,
}

impl Resource {
    /// The text of the file in `lang`.
    pub fn source(&self, lang: Lang) -> &'static str {
        match lang {
            Lang::En => self.en,
            Lang::Fr => self.fr,
        }
    }
}

/// Every Fluent file. To add one, put it in `i18n/en/` and `i18n/fr/` and list
/// the pair here: the tests compare the two directories with this list.
pub const RESOURCES: &[Resource] = &[
    Resource {
        name: "report.ftl",
        en: include_str!("../../../i18n/en/report.ftl"),
        fr: include_str!("../../../i18n/fr/report.ftl"),
    },
    Resource {
        name: "html.ftl",
        en: include_str!("../../../i18n/en/html.ftl"),
        fr: include_str!("../../../i18n/fr/html.ftl"),
    },
    Resource {
        name: "cli.ftl",
        en: include_str!("../../../i18n/en/cli.ftl"),
        fr: include_str!("../../../i18n/fr/cli.ftl"),
    },
];

/// The words of one language: the Fluent messages and the labels of the rules.
///
/// A word that does not exist is given as its id, so that a report stays
/// readable, and is recorded in [`problems`](Self::problems). The tests of a
/// renderer render a report in both languages and check that the list is empty.
pub struct Messages {
    lang: Lang,
    bundle: FluentBundle<FluentResource>,
    labels: Labels,
    problems: RefCell<BTreeSet<String>>,
}

impl Messages {
    /// The words of `lang`, with the labels of `rules`.
    ///
    /// The Fluent files are embedded and the tests load them strictly: a syntax
    /// error, a message defined twice or a message that refers to one that does
    /// not exist cannot reach a release. So here what parsed is kept, a later
    /// definition overrides an earlier one, and formatting errors are not
    /// collected.
    pub fn new(lang: Lang, rules: &RuleSet) -> Self {
        let mut bundle = FluentBundle::new(vec![language_id(lang)]);
        // The words are printed as plain text: no Unicode isolation marks
        // (U+2068, U+2069) around placeables (ADR-0002).
        bundle.set_use_isolating(false);
        for resource in RESOURCES {
            let source = resource.source(lang).to_owned();
            let parsed = FluentResource::try_new(source).unwrap_or_else(|(partial, _)| partial);
            bundle.add_resource_overriding(parsed);
        }
        Self {
            lang,
            bundle,
            labels: Labels::new(lang, rules),
            problems: RefCell::default(),
        }
    }

    /// The language of the words.
    pub fn lang(&self) -> Lang {
        self.lang
    }

    /// The message `id`, or `id` itself when there is none.
    pub fn text(&self, id: &str) -> String {
        let pattern = self
            .bundle
            .get_message(id)
            .and_then(|message| message.value());
        match pattern {
            Some(pattern) => self
                .bundle
                .format_pattern(pattern, None, &mut Vec::new())
                .into_owned(),
            None => self.missing("message", id),
        }
    }

    /// The outcome of the analysis of a file.
    pub fn status(&self, status: Status) -> String {
        self.word("status", &status)
    }

    /// The format of a file.
    pub fn container(&self, container: Container) -> String {
        self.word("container", &container)
    }

    /// The complexity level of a file.
    pub fn level(&self, level: Level) -> String {
        self.word("level", &level)
    }

    /// The severity of a security signal.
    pub fn severity(&self, severity: Severity) -> String {
        self.word("severity", &severity)
    }

    /// The kind of a procedure.
    pub fn procedure_kind(&self, kind: ProcedureKind) -> String {
        self.word("kind", &kind)
    }

    /// The access modifier of a procedure.
    pub fn visibility(&self, visibility: Visibility) -> String {
        self.word("visibility", &visibility)
    }

    /// The type of a VBA module.
    pub fn module_type(&self, module_type: ModuleType) -> String {
        self.word("module-type", &module_type)
    }

    /// The label of the family `id`: that of the rules, or the message for
    /// `unclassified`.
    pub fn family(&self, id: &str) -> String {
        if id == UNCLASSIFIED {
            return self.text("family-unclassified");
        }
        self.label("family label", &self.labels.families, id)
    }

    /// The label of a security signal, a chain or a container indicator.
    pub fn signal(&self, id: &str) -> String {
        self.label("signal label", &self.labels.signals, id)
    }

    /// The label of a complexity trait.
    pub fn trait_label(&self, id: &str) -> String {
        self.label("trait label", &self.labels.traits, id)
    }

    /// A number as the language writes it: French has a decimal comma.
    pub fn number(&self, number: f64) -> String {
        let text = number.to_string();
        match self.lang {
            Lang::En => text,
            Lang::Fr => text.replace('.', ","),
        }
    }

    /// The words that were asked for and do not exist, sorted.
    pub fn problems(&self) -> Vec<String> {
        self.problems.borrow().iter().cloned().collect()
    }

    /// The message for a value of an enumeration of the schema, named after
    /// the enumeration and the value: `status-no-macros` for `no_macros`.
    fn word<T: Serialize>(&self, enumeration: &str, value: &T) -> String {
        self.text(&format!(
            "{enumeration}-{}",
            wire_name(value).replace('_', "-")
        ))
    }

    /// The label of `id` among `labels`, or `id` itself, recorded as a missing
    /// `what`.
    fn label(&self, what: &str, labels: &BTreeMap<String, String>, id: &str) -> String {
        let label = labels.get(id);
        label.cloned().unwrap_or_else(|| self.missing(what, id))
    }

    /// Records that the `what` named `id` does not exist, and gives `id` in
    /// its place.
    fn missing(&self, what: &str, id: &str) -> String {
        self.problems
            .borrow_mut()
            .insert(format!("no {what} '{id}'"));
        id.to_owned()
    }
}

/// The labels of the rules in one language, by id.
struct Labels {
    families: BTreeMap<String, String>,
    signals: BTreeMap<String, String>,
    traits: BTreeMap<String, String>,
}

impl Labels {
    fn new(lang: Lang, rules: &RuleSet) -> Self {
        let pick = |label: &Label| match lang {
            Lang::En => label.en.clone(),
            Lang::Fr => label.fr.clone(),
        };
        let security = &rules.security;
        let signals = security.signals.iter().map(|s| (&s.id, &s.label));
        let chains = security.chains.iter().map(|c| (&c.id, &c.label));
        let indicators = (security.container_indicators.iter()).map(|i| (&i.id, &i.label));
        Self {
            families: (rules.families.families.iter())
                .map(|family| (family.id.clone(), pick(&family.label)))
                .collect(),
            signals: signals
                .chain(chains)
                .chain(indicators)
                .map(|(id, label)| (id.clone(), pick(label)))
                .collect(),
            traits: (rules.complexity.traits.iter())
                .map(|item| (item.id.clone(), pick(&item.label)))
                .collect(),
        }
    }
}

fn language_id(lang: Lang) -> LanguageIdentifier {
    wire_name(&lang).parse().unwrap_or_default()
}
