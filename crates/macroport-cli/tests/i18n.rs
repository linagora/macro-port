// SPDX-License-Identifier: Apache-2.0
//! The words of the reports (A7): the same messages in English and French, and
//! a word for every value of the enumerations of the schema.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use fluent_bundle::{FluentBundle, FluentResource};
use macroport_cli::i18n::{Messages, RESOURCES};
use macroport_core::report::{Container, Lang, Level, ProcedureKind, Status, Visibility};
use macroport_core::rules::{ModuleType, Severity};
use serde::de::DeserializeOwned;
use serde_json::Value;
use unic_langid::LanguageIdentifier;

const LANGS: [Lang; 2] = [Lang::En, Lang::Fr];

/// The name of the directory of `lang` in `i18n/`, which is also the name of
/// the language in the schema.
fn directory_name(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "en",
        Lang::Fr => "fr",
    }
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(path: &Path) -> String {
    fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("{} is readable: {error}", path.display()))
}

/// The ids of the messages (`id`), terms (`-id`) and attributes (`id.name`) of
/// a Fluent file.
///
/// An entry starts on a line that begins in the first column with `id =` or
/// `-id =`. Its attributes are the indented lines that follow, blank lines
/// apart, and begin with `.name =`.
fn keys(source: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let mut entry: Option<&str> = None;
    for line in source.lines().filter(|line| !line.trim().is_empty()) {
        let name = line.split_once('=').map(|(name, _)| name.trim());
        if line.starts_with(char::is_whitespace) {
            let attribute = name
                .and_then(|name| name.strip_prefix('.'))
                .filter(|name| is_identifier(name));
            if let (Some(entry), Some(attribute)) = (entry, attribute) {
                found.insert(format!("{entry}.{attribute}"));
            }
        } else {
            entry = name.filter(|name| is_identifier(name.trim_start_matches('-')));
            found.extend(entry.map(str::to_owned));
        }
    }
    found
}

fn is_identifier(text: &str) -> bool {
    text.starts_with(|first: char| first.is_ascii_alphabetic())
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
}

/// The Fluent files of `lang` loaded strictly: a file that does not parse, or
/// a message that two files define, fails.
fn strict_bundle(lang: Lang) -> FluentBundle<FluentResource> {
    let mut bundle = FluentBundle::new(vec![LanguageIdentifier::default()]);
    for resource in RESOURCES {
        let parsed = FluentResource::try_new(resource.source(lang).to_owned()).unwrap_or_else(
            |(_, errors)| panic!("{} ({lang:?}) does not parse: {errors:?}", resource.name),
        );
        bundle.add_resource(parsed).unwrap_or_else(|errors| {
            panic!(
                "{} ({lang:?}) redefines a message: {errors:?}",
                resource.name
            )
        });
    }
    bundle
}

#[test]
fn the_scan_of_a_fluent_file_finds_messages_terms_and_attributes() {
    let source = "\
# A comment with an = sign.
## A group

plain = Text
long =
    A value on
    two lines

    .title = An attribute
-term = A term
    .case = Its attribute
count = { $number ->
    [one] a file with = in it
   *[other] files
}
";
    let found: Vec<String> = keys(source).into_iter().collect();
    assert_eq!(
        found.join(" "),
        "-term -term.case count long long.title plain"
    );
}

#[test]
fn the_files_that_the_binary_embeds_are_those_of_the_two_directories() {
    let embedded: BTreeSet<String> = RESOURCES
        .iter()
        .map(|resource| resource.name.to_owned())
        .collect();
    for lang in LANGS {
        let name = directory_name(lang);
        let directory = repo_root().join("i18n").join(name);
        let entries = fs::read_dir(&directory)
            .unwrap_or_else(|error| panic!("{} is readable: {error}", directory.display()));
        let on_disk: BTreeSet<String> = entries
            .flatten()
            .filter_map(|entry| entry.file_name().into_string().ok())
            .filter(|name| name.ends_with(".ftl"))
            .collect();
        assert_eq!(
            on_disk, embedded,
            "the .ftl files of i18n/{name}/ and RESOURCES"
        );
        for resource in RESOURCES {
            assert_eq!(
                read(&directory.join(resource.name)),
                resource.source(lang),
                "{} is the file of i18n/{name}/",
                resource.name
            );
        }
    }
}

#[test]
fn english_and_french_define_the_same_messages_terms_and_attributes() {
    for resource in RESOURCES {
        let en = keys(resource.en);
        let fr = keys(resource.fr);
        assert!(!en.is_empty(), "{} has messages", resource.name);
        assert_ne!(resource.en, resource.fr, "{} is translated", resource.name);
        let only_en: Vec<&String> = en.difference(&fr).collect();
        let only_fr: Vec<&String> = fr.difference(&en).collect();
        assert!(
            only_en.is_empty() && only_fr.is_empty(),
            "{}: only in English: {only_en:?}; only in French: {only_fr:?}",
            resource.name
        );
    }
}

#[test]
fn the_files_are_valid_fluent_and_define_no_message_twice() {
    for lang in LANGS {
        strict_bundle(lang);
    }
}

#[test]
fn every_message_formats_to_a_text_without_an_error() {
    for lang in LANGS {
        let bundle = strict_bundle(lang);
        for resource in RESOURCES {
            let ids: Vec<String> = keys(resource.source(lang))
                .into_iter()
                .filter(|key| !key.starts_with('-') && !key.contains('.'))
                .collect();
            assert!(!ids.is_empty(), "{} ({lang:?}) has messages", resource.name);
            for id in ids {
                let pattern = bundle
                    .get_message(&id)
                    .and_then(|message| message.value())
                    .unwrap_or_else(|| panic!("{id} ({lang:?}) has a value"));
                let mut errors = Vec::new();
                let text = bundle.format_pattern(pattern, None, &mut errors);
                assert!(errors.is_empty(), "{id} ({lang:?}): {errors:?}");
                assert!(!text.trim().is_empty(), "{id} ({lang:?}) is empty");
            }
        }
    }
}

/// Asks for the word of every value that `schema` lists at `pointer`, through
/// the typed method that renderers use.
fn assert_words_at<T: DeserializeOwned>(
    schema: &Value,
    pointer: &str,
    messages: &Messages,
    word: fn(&Messages, T) -> String,
) {
    let values = schema
        .pointer(pointer)
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("{pointer} is a list"));
    assert!(!values.is_empty(), "{pointer} has values");
    for value in values {
        let value: T = serde_json::from_value(value.clone())
            .unwrap_or_else(|error| panic!("{pointer}: {value} is not in the model: {error}"));
        word(messages, value);
    }
}

/// [`assert_words_at`] for each `pointer`, with the method that names its
/// values. Gives the number of enumerations.
macro_rules! assert_every_word {
    ($schema:expr, $messages:expr, $($pointer:literal => $method:ident,)+) => {{
        let mut enumerations = 0;
        $(
            assert_words_at($schema, $pointer, $messages, Messages::$method);
            enumerations += 1;
        )+
        enumerations
    }};
}

#[test]
fn every_value_of_the_enumerations_of_the_schema_has_a_word_in_both_languages() {
    let text = read(&repo_root().join("schema/report.schema.json"));
    let schema: Value =
        serde_json::from_str(&text).unwrap_or_else(|error| panic!("the schema parses: {error}"));
    // The language of the report is the one enumeration that holds no words.
    let languages = schema
        .pointer("/properties/lang/enum")
        .and_then(Value::as_array);
    let languages: Vec<&str> = languages
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    assert_eq!(languages, LANGS.map(directory_name), "the languages");

    for lang in LANGS {
        let messages = Messages::new(lang);
        let enumerations = assert_every_word!(
            &schema,
            &messages,
            "/$defs/file/properties/status/enum" => status,
            "/$defs/file/properties/container/enum" => container,
            "/$defs/level/enum" => level,
            "/$defs/severity/enum" => severity,
            "/$defs/procedure/properties/kind/enum" => procedure_kind,
            "/$defs/procedure/properties/visibility/enum" => visibility,
            "/$defs/module/properties/type/enum" => module_type,
        );
        assert_eq!(
            text.matches("\"enum\"").count(),
            enumerations + 1,
            "the schema has an enumeration that no word covers"
        );
        assert_eq!(messages.problems(), Vec::<String>::new(), "{lang:?}");
    }
}

#[test]
fn the_words_are_those_of_the_language() {
    let en = Messages::new(Lang::En);
    let fr = Messages::new(Lang::Fr);

    assert_eq!(en.status(Status::NoMacros), "No macros");
    assert_eq!(fr.status(Status::NoMacros), "Sans macros");
    assert_eq!(en.container(Container::TextModule), "Text module");
    assert_eq!(fr.container(Container::TextModule), "Module texte");
    assert_eq!(en.level(Level::Medium), "Medium");
    assert_eq!(fr.level(Level::Medium), "Moyenne");
    assert_eq!(en.severity(Severity::High), "High");
    assert_eq!(fr.severity(Severity::High), "Élevée");
    assert_eq!(
        en.procedure_kind(ProcedureKind::PropertyGet),
        "Property Get"
    );
    assert_eq!(
        fr.procedure_kind(ProcedureKind::PropertyGet),
        "Property Get"
    );
    assert_eq!(en.visibility(Visibility::Default), "Default");
    assert_eq!(fr.visibility(Visibility::Default), "Par défaut");
    assert_eq!(en.module_type(ModuleType::Form), "Form");
    assert_eq!(fr.module_type(ModuleType::Form), "Formulaire");

    assert_eq!(en.problems(), Vec::<String>::new());
    assert_eq!(fr.problems(), Vec::<String>::new());
}

#[test]
fn a_message_that_does_not_exist_is_given_as_its_id_and_recorded() {
    for lang in LANGS {
        let messages = Messages::new(lang);
        assert_eq!(messages.problems(), Vec::<String>::new());
        assert_eq!(messages.text("no-such-message"), "no-such-message");
        assert_eq!(
            messages.problems(),
            ["no message 'no-such-message'"],
            "{lang:?}"
        );
    }
}
