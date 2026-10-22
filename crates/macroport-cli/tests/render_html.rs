// SPDX-License-Identifier: Apache-2.0
//! The HTML report: what it shows, and what an analysed file cannot make it do
//! (SPEC section 8, A7).

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use macroport_cli::i18n::{Messages, RESOURCES};
use macroport_cli::render::html::render;
use macroport_core::report::{Evidence, FamilyScore, Lang, Level, Report, Signal, Status, Totals};
use macroport_core::rules::{RuleSet, RulesSources, Severity};
use macroport_testkit::report_problems;
use serde_json::Value;
use sha2::{Digest, Sha256};

const TEMPLATE: &str = include_str!("../templates/report.html");
const RENDERER: &str = include_str!("../src/render/html.rs");
const STYLE: &str = include_str!("../templates/report.css");
const SCRIPT: &str = include_str!("../templates/report.js");
const SCHEMA: &str = include_str!("../../../schema/report.schema.json");

const LANGS: [Lang; 2] = [Lang::En, Lang::Fr];

/// What a hostile file can put in a string of the report: the start of a
/// comment, of a script and of an element with a handler, the end of a script
/// (with a NUL inside, and in capitals), both quotes and an ampersand.
const HOSTILE: &str = "<!--<script>x</scr\0ipt></SCRIPT >\"'&<img src=x onerror=alert(1)>";

/// Where the strings that [`hostile`] replaces are in a report: JSON pointers,
/// with `*` for the index in a list. The names of families, signals and traits,
/// the places and the lines of evidence are those of [`hostile_names`].
const SHOWN: [&str; 12] = [
    "/generated_at",
    "/tool/name",
    "/tool/version",
    "/tool/rules_version",
    "/scan/roots/*",
    "/files/*/path",
    "/files/*/sha256",
    "/files/*/errors/*/code",
    "/files/*/errors/*/message",
    "/files/*/vba_project/name",
    "/files/*/vba_project/references/*",
    "/files/*/notes/*",
];

/// The element that holds the report as JSON.
const DATA_OPEN: &str = r#"<script type="application/json" id="report-data">"#;
const POLICY_OPEN: &str = r#"<meta http-equiv="Content-Security-Policy" content=""#;

fn rules() -> RuleSet {
    RuleSet::load(&RulesSources::EMBEDDED).expect("the shipped rules load")
}

/// The page of `report`, which asks for no word that does not exist.
fn page(report: &Report, lang: Lang) -> String {
    let messages = Messages::new(lang, &rules());
    let page = render(report, &messages).expect("the report renders");
    assert_eq!(messages.problems(), Vec::<String>::new(), "{lang:?}");
    page
}

/// A report on no file, with no root and no duration.
fn empty() -> Report {
    Report::empty(&rules(), "1970-01-01T00:00:00Z".to_owned(), Lang::En)
}

/// The fleet with a hostile string, each one different, in every string of
/// [`SHOWN`]. The report is no longer valid, and the strings are returned.
fn hostile() -> (Report, Vec<String>) {
    let mut value = serde_json::to_value(common::fleet()).expect("a report is JSON");
    let mut strings = Vec::new();
    make_hostile(&mut value, "", &mut strings);
    for place in SHOWN {
        let start = format!("{place}#");
        assert!(
            strings.iter().any(|string| string.starts_with(&start)),
            "the fleet has a string at {place}"
        );
    }
    let report = serde_json::from_value(value).expect("the report is still a report");
    (report, strings)
}

/// Replaces the strings of [`SHOWN`] in `value`, which is at `place`.
fn make_hostile(value: &mut Value, place: &str, strings: &mut Vec<String>) {
    match value {
        Value::String(text) if SHOWN.contains(&place) => {
            *text = format!("{place}#{}:{HOSTILE}", strings.len());
            strings.push(text.clone());
        }
        Value::Array(items) => {
            for item in items {
                make_hostile(item, &format!("{place}/*"), strings);
            }
        }
        Value::Object(members) => {
            for (key, member) in members {
                make_hostile(member, &format!("{place}/{key}"), strings);
            }
        }
        _ => {}
    }
}

/// What lies between `open` and the next `close`. `open` must occur once.
fn between<'a>(page: &'a str, open: &str, close: &str) -> &'a str {
    assert_eq!(page.matches(open).count(), 1, "{open} occurs once");
    let (_, after) = page.split_once(open).expect("open occurs");
    let (inside, _) = after
        .split_once(close)
        .unwrap_or_else(|| panic!("{open} is closed by {close}"));
    inside
}

/// The page without what lies in its `<style>`, in its data block and in its
/// `<script>`: the markup that the template writes around the text.
fn markup(page: &str) -> String {
    let mut markup = page.to_owned();
    for (open, close) in [
        ("<style>", "</style>"),
        (DATA_OPEN, "</script>"),
        ("<script>", "</script>"),
    ] {
        let element = format!("{open}{}{close}", between(page, open, close));
        markup = markup.replacen(&element, &format!("{open}{close}"), 1);
    }
    markup
}

/// The text of `markup` as a reader sees it: no tags, and the entities of the
/// escaping decoded.
fn visible_text(markup: &str) -> String {
    let mut text = String::new();
    let mut in_tag = false;
    for c in markup.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => {
                in_tag = false;
                text.push(' ');
            }
            _ if !in_tag => text.push(c),
            _ => {}
        }
    }
    text.replace("&#60;", "<")
        .replace("&#62;", ">")
        .replace("&#34;", "\"")
        .replace("&#39;", "'")
        .replace("&#38;", "&")
}

/// The words of `markup` with one space between them: what a reader sees, in
/// the order of the page.
fn words(markup: &str) -> String {
    let text = visible_text(markup);
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The source that a policy needs to allow `text` as the content of an inline
/// element. A browser reads CRLF and CR as LF before it checks a hash.
fn hash_source(text: &str) -> String {
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    format!(
        "'sha256-{}'",
        STANDARD.encode(Sha256::digest(text.as_bytes()))
    )
}

#[test]
fn the_policy_comes_first_and_allows_the_inline_style_and_script_by_hash_only() {
    for report in [common::fleet(), common::injection()] {
        let page = page(&report, Lang::En);

        // Nothing but the character set precedes the policy.
        let (before, _) = page.split_once(POLICY_OPEN).expect("a policy");
        assert_eq!(
            before,
            "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n"
        );

        // Askama writes the quotes of the policy as entities.
        let policy = between(&page, POLICY_OPEN, "\">").replace("&#39;", "'");
        let directives: Vec<(&str, Vec<&str>)> = policy
            .split("; ")
            .map(|directive| {
                let mut words = directive.split(' ');
                (words.next().unwrap_or_default(), words.collect())
            })
            .collect();
        let style = hash_source(between(&page, "<style>", "</style>"));
        let script = hash_source(between(&page, "<script>", "</script>"));
        assert_eq!(
            directives,
            [
                ("default-src", vec!["'none'"]),
                ("script-src", vec![script.as_str()]),
                ("style-src", vec![style.as_str()]),
                ("img-src", vec!["data:"]),
                ("connect-src", vec!["'none'"]),
                ("base-uri", vec!["'none'"]),
                ("form-action", vec!["'none'"]),
            ]
        );
    }
}

#[test]
fn the_page_has_one_style_and_two_scripts_the_data_block_and_the_filter() {
    let page = page(&common::injection(), Lang::En);
    let markup = markup(&page);
    assert_eq!(markup.matches("<style").count(), 1);
    assert_eq!(markup.matches("<script").count(), 2);
    assert_eq!(markup.matches("</script").count(), 2);
    assert_eq!(markup.matches("<link").count(), 1);
    // The filter is the script of this crate, as the line breaks of a browser
    // read it.
    assert_eq!(
        between(&page, "<script>", "</script>"),
        SCRIPT.replace("\r\n", "\n")
    );
}

#[test]
fn the_script_looks_for_what_the_template_writes() {
    let report = common::fleet();
    let page = page(&report, Lang::En);
    let ids: Vec<&str> = (SCRIPT.split("getElementById(\"").skip(1))
        .filter_map(|rest| rest.split_once('"'))
        .map(|(id, _)| id)
        .collect();
    assert_eq!(ids, ["toolbar", "filter", "shown", "expand", "collapse"]);
    for id in ids {
        assert_eq!(page.matches(&format!(" id=\"{id}\"")).count(), 1, "{id}");
    }

    // It matches a file on its path and on the value of each of its chips.
    assert!(SCRIPT.contains(r#"querySelectorAll("details.file")"#));
    assert!(SCRIPT.contains(r#"querySelectorAll("summary .path, summary .value")"#));
    let files = &report.files;
    let chips = files.len()
        + (files.iter().filter(|file| file.complexity_level.is_some())).count()
        + (files.iter().filter(|file| file.max_severity.is_some())).count()
        + (files.iter().filter(|file| file.primary_family.is_some())).count();
    assert_eq!(
        page.matches(r#"<details class="file">"#).count(),
        files.len()
    );
    // The chips of the signals, further down in a file, are not in a summary.
    let summaries = cut(&page, "<summary>", "</summary>").1.concat();
    assert_eq!(
        summaries.matches(r#"<span class="path">"#).count(),
        files.len()
    );
    assert_eq!(summaries.matches(r#"<span class="value">"#).count(), chips);
}

#[test]
fn the_data_block_cannot_end_its_element_and_gives_the_report_back() {
    let mut report = common::injection();
    report.files[0].path.push_str("\u{2028}\u{2029}&<>");
    let page = page(&report, Lang::En);
    let data = between(&page, DATA_OPEN, "</script>");
    for forbidden in ['<', '>', '&', '\u{2028}', '\u{2029}'] {
        assert!(!data.contains(forbidden), "{forbidden:?} is escaped");
    }
    assert_eq!(report_problems(data), Vec::<String>::new());
    let read: Report = serde_json::from_str(data).expect("the block is a report");
    assert_eq!(read, report);
}

/// The strings of the injection fixture that the page shows.
fn injected(report: &Report) -> Vec<String> {
    let first = &report.files[0];
    let second = &report.files[1];
    let project = first.vba_project.as_ref().expect("a project");
    let mut shown = vec![first.path.as_str(), second.path.as_str()];
    shown.extend(report.scan.roots.iter().flatten().map(String::as_str));
    shown.extend(project.name.as_deref());
    shown.extend(project.references.iter().flatten().map(String::as_str));
    shown.extend(second.errors.iter().flatten().map(|e| e.message.as_str()));
    let lines = (first.signals.iter().flatten()).flat_map(|s| s.evidence.iter().flatten());
    shown.extend(lines.filter_map(|line| line.snippet.as_deref()));
    assert_eq!(shown.len(), 8, "the fixture has eight strings to show");
    shown.into_iter().map(str::to_owned).collect()
}

#[test]
fn what_an_analysed_file_names_is_shown_as_text() {
    let injection = common::injection();
    let injected = injected(&injection);
    let (hostile, strings) = hostile();

    for (report, shown) in [(injection, injected), (hostile, strings)] {
        for lang in LANGS {
            let markup = markup(&page(&report, lang));
            let text = visible_text(&markup);
            for string in &shown {
                assert!(text.contains(string), "{string:?} is shown as it is");
                assert!(!markup.contains(string), "{string:?} is not markup");
            }
        }
    }
}

/// A start or end tag of the page.
struct Tag {
    name: String,
    attributes: Vec<(String, String)>,
}

/// The tags of `markup`. A raw `<` or `>` can only be the delimiter of a tag:
/// the page escapes them, and `"`, in what it writes. A page that did not would
/// fail here, or show up as a tag or an attribute that [`is_allowed`] refuses.
fn tags(markup: &str) -> Vec<Tag> {
    let mut tags = Vec::new();
    let mut rest = markup;
    while let Some((text, after)) = rest.split_once('<') {
        assert!(!text.contains('>'), "a raw > in a text: {text:?}");
        let (inside, after) = after.split_once('>').expect("a tag ends");
        assert!(!inside.contains('<'), "a raw < in a tag: {inside:?}");
        rest = after;
        if inside != "!doctype html" {
            tags.push(parse_tag(inside));
        }
    }
    assert!(!rest.contains('>'), "a raw > in a text: {rest:?}");
    tags
}

/// The tag written `<inside>`, whose attribute values are all in double quotes.
fn parse_tag(inside: &str) -> Tag {
    let inside = inside.trim_start_matches('/');
    let (name, mut rest) = inside.split_once(' ').unwrap_or((inside, ""));
    let mut attributes = Vec::new();
    while !rest.is_empty() {
        let end = rest.find(['=', ' ']).unwrap_or(rest.len());
        let (attribute, after) = rest.split_at(end);
        let value = if let Some(quoted) = after.strip_prefix("=\"") {
            let (value, after_value) = quoted
                .split_once('"')
                .unwrap_or_else(|| panic!("a value ends in <{inside}>"));
            rest = after_value.trim_start();
            value
        } else {
            assert!(!after.starts_with('='), "an unquoted value in <{inside}>");
            rest = after.trim_start();
            ""
        };
        attributes.push((attribute.to_owned(), value.to_owned()));
    }
    Tag {
        name: name.to_owned(),
        attributes,
    }
}

const ELEMENTS: [&str; 34] = [
    "html", "head", "meta", "title", "link", "style", "body", "header", "main", "section", "h1",
    "h2", "h3", "dl", "div", "dt", "dd", "p", "ul", "li", "table", "tr", "th", "td", "meter",
    "label", "input", "button", "output", "details", "summary", "span", "code", "script",
];

/// The classes that lay the page out.
const LAYOUT_CLASSES: &str = "facts notice totals distributions count toolbar file path chip \
                              hint value families signals evidence score";

/// The classes that colour a word after a value of the schema: `status-ok`,
/// `level-low`, `severity-high`.
static COLOUR_CLASSES: LazyLock<BTreeSet<String>> = LazyLock::new(|| {
    let schema: Value = serde_json::from_str(SCHEMA).expect("the schema parses");
    let mut classes = BTreeSet::new();
    for (kind, pointer) in [
        ("status", "/$defs/file/properties/status/enum"),
        ("level", "/$defs/level/enum"),
        ("severity", "/$defs/severity/enum"),
    ] {
        let values = (schema.pointer(pointer).and_then(Value::as_array))
            .unwrap_or_else(|| panic!("{pointer} is a list"));
        let values = values.iter().filter_map(Value::as_str);
        classes.extend(values.map(|value| format!("{kind}-{value}")));
    }
    classes
});

/// Whether the page may write `element` with `attribute` set to `value`. What
/// comes from the report is never among the values: the page writes only
/// fixed words, the names of values of the schema, and numbers.
fn is_allowed(element: &str, attribute: &str, value: &str) -> bool {
    let number = !value.is_empty() && value.chars().all(|c| c.is_ascii_digit());
    match (element, attribute) {
        ("html", "lang") => ["en", "fr"].contains(&value),
        ("meta", "charset") => value == "utf-8",
        ("meta", "http-equiv") => value == "Content-Security-Policy",
        ("meta", "name") => value == "viewport",
        ("meta", "content") => {
            value.starts_with("default-src ") || value == "width=device-width, initial-scale=1"
        }
        ("link", "rel") => value == "icon",
        ("link", "href") => value == "data:,",
        ("script", "type") => value == "application/json",
        ("script", "id") => value == "report-data",
        ("div", "id") => value == "toolbar",
        ("input", "id") => value == "filter",
        ("button", "id") => ["expand", "collapse"].contains(&value),
        ("output", "id") => value == "shown",
        ("div", "hidden") => value.is_empty(),
        ("label", "for") => value == "filter",
        ("input", "type") => value == "search",
        ("button", "type") => value == "button",
        ("th", "scope") => value == "row",
        ("meter", "min" | "max" | "value") => number,
        ("meter", "aria-hidden") => value == "true",
        (_, "class") => value.split(' ').all(|class| {
            LAYOUT_CLASSES.split(' ').any(|layout| layout == class)
                || COLOUR_CLASSES.contains(class)
        }),
        _ => false,
    }
}

#[test]
fn the_markup_has_only_known_elements_and_attributes_whatever_the_report_says() {
    for report in [common::fleet(), common::injection(), empty(), hostile().0] {
        for lang in LANGS {
            let tags = tags(&markup(&page(&report, lang)));
            assert!(tags.len() > 50, "the page has markup");
            for tag in tags {
                assert!(ELEMENTS.contains(&tag.name.as_str()), "<{}>", tag.name);
                for (name, value) in tag.attributes {
                    assert!(
                        is_allowed(&tag.name, &name, &value),
                        "<{} {name}=\"{value}\">",
                        tag.name
                    );
                }
            }
        }
    }
}

#[test]
fn the_style_and_the_script_make_no_request_and_cannot_end_their_element() {
    for token in ["@import", "url(", "expression(", "behavior"] {
        assert!(!STYLE.contains(token), "the style has no `{token}`");
    }
    // A tripwire, not a proof. The script reads text from the page and sets
    // properties of its elements, so it has none of the calls below, which
    // reach out, write markup or run a string; a change that adds one fails here
    // and gets a look. What holds if this list misses one is the policy of the
    // page: scripts by hash, no `unsafe-eval`, `connect-src 'none'`.
    let code = (SCRIPT.lines())
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    let banned = "fetch XMLHttpRequest WebSocket EventSource sendBeacon Worker import location \
                  window.open src href eval Function setTimeout setInterval innerHTML outerHTML \
                  insertAdjacentHTML document.write DOMParser createContextualFragment \
                  createElement append prepend setAttribute";
    for token in banned.split(' ') {
        assert!(!code.contains(token), "the script has no `{token}`");
    }
    // Both are written into the page as they are.
    for (name, text) in [("style", STYLE), ("script", SCRIPT)] {
        assert!(!text.contains("</"), "the {name} ends no element");
        assert!(!text.contains("<!--"), "the {name} opens no comment");
    }
}

/// `text` cut at the spans that start with `open` and end with `close`: what
/// lies outside them, and what lies inside each.
fn cut<'a>(text: &'a str, open: &str, close: &str) -> (String, Vec<&'a str>) {
    let mut outside = String::new();
    let mut inside = Vec::new();
    let mut rest = text;
    while let Some((before, after)) = rest.split_once(open) {
        outside.push_str(before);
        let (span, after) =
            (after.split_once(close)).unwrap_or_else(|| panic!("{open} ends in {close}"));
        inside.push(span);
        rest = after;
    }
    outside.push_str(rest);
    (outside, inside)
}

/// The template without its comments.
fn template() -> String {
    cut(TEMPLATE, "{#", "#}").0
}

#[test]
fn the_template_applies_no_filter_and_has_only_plain_blocks() {
    let template = template();
    let (_, expressions) = cut(&template, "{{", "}}");
    let (_, blocks) = cut(&template, "{%", "%}");
    assert!(
        expressions.len() >= 10 && blocks.len() >= 4,
        "the template has code"
    );

    // `|safe`, `|escape("none")`, a `filter` block and `let x = y|safe` all
    // have a filter or a keyword that the list leaves out.
    for code in expressions.iter().chain(&blocks) {
        assert!(!code.contains('|'), "a filter in {code:?}");
    }
    for block in blocks {
        let block = block.trim_matches(|c: char| c.is_whitespace() || "+-~".contains(c));
        let keyword = block.split_whitespace().next().unwrap_or_default();
        let keywords = "if else endif for endfor macro endmacro call endcall";
        assert!(
            keywords.split(' ').any(|k| k == keyword),
            "a block {block:?}"
        );
    }
}

#[test]
fn the_template_writes_no_word_of_its_own() {
    let template = template();
    let (rest, _) = cut(&template, "{{", "}}");
    let (rest, _) = cut(&rest, "{%", "%}");
    let (text, tags) = cut(&rest, "<", ">");
    assert!(tags.len() >= 20, "the template has markup");
    assert!(
        !text.chars().any(char::is_alphanumeric),
        "text outside Fluent: {text:?}"
    );
}

#[test]
fn only_the_style_the_script_and_the_data_are_marked_safe() {
    // A `Safe` value is written as it is, without escaping.
    assert_eq!(RENDERER.matches("Safe(").count(), 3);
}

/// The code of the renderer: no comment and no unit test, so that an id in
/// either does not count as asked for.
fn renderer() -> String {
    let (code, _tests) = (RENDERER.split_once("#[cfg(test)]")).unwrap_or((RENDERER, ""));
    (code.lines())
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Whether `text` is the id of a message of `html.ftl`.
fn is_html_id(text: &str) -> bool {
    (text.strip_prefix("html-")).is_some_and(|name| {
        !name.is_empty()
            && (name.chars()).all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    })
}

#[test]
fn the_page_asks_for_every_message_of_html_ftl_and_for_no_other() {
    let resource = RESOURCES
        .iter()
        .find(|resource| resource.name == "html.ftl")
        .expect("html.ftl is embedded");
    let defined: BTreeSet<&str> = (resource.en.lines())
        .filter_map(|line| line.split_once(" = "))
        .map(|(id, _)| id)
        .filter(|id| is_html_id(id))
        .collect();
    let renderer = renderer();
    let asked: BTreeSet<&str> = (TEMPLATE.split('"'))
        .chain(renderer.split('"'))
        .filter(|piece| is_html_id(piece))
        .collect();
    assert_eq!(asked, defined);

    for lang in LANGS {
        let messages = Messages::new(lang, &rules());
        for id in &asked {
            assert_ne!(messages.text(id), *id, "{id} in {lang:?}");
        }
        assert_eq!(messages.problems(), Vec::<String>::new(), "{lang:?}");
    }
}

#[test]
fn the_page_is_in_the_language_that_was_asked_for() {
    let report = common::fleet();
    let en = page(&report, Lang::En);
    let fr = page(&report, Lang::Fr);

    assert!(en.contains("<html lang=\"en\">"));
    assert!(en.contains("<title>Macro-Port report</title>"));
    let shown = words(&markup(&en));
    assert!(shown.contains("Scan duration 842 ms"), "{shown}");
    assert!(shown.contains("Status Suspicious archive"), "{shown}");

    assert!(fr.contains("<html lang=\"fr\">"));
    assert!(fr.contains("<title>Rapport Macro-Port</title>"));
    let shown = words(&markup(&fr));
    assert!(shown.contains("Durée de l’analyse 842 ms"), "{shown}");
    assert!(shown.contains("Statut Archive suspecte"), "{shown}");
}

#[test]
fn the_totals_and_the_distributions_are_those_of_the_report() {
    let report = common::fleet();
    let en = words(&markup(&page(&report, Lang::En)));
    let fr = words(&markup(&page(&report, Lang::Fr)));

    for expected in [
        "Totals Files 11 Files with macros 4 Files that failed 4 Modules 5 Procedures 7 \
         Logical lines of code 67 Possibly unused procedures 1",
        "Files by status Analysed 4 No macros 1 Encrypted 1 Corrupted 1 Unsupported 1 \
         Too large 1 Suspicious archive 1 Error 1",
        "Files by complexity Low 4 Medium 1",
        "Files by highest severity Medium 3 High 1",
        "Procedures by family Formatting and layout 1 Lookup and reference data 1 \
         Forms and data entry 2 Event-driven automation 2 Unclassified 1",
    ] {
        assert!(en.contains(expected), "{expected:?} in {en}");
    }
    for expected in [
        "Totaux Fichiers 11 Fichiers avec macros 4 Fichiers en échec 4 Modules 5 Procédures 7 \
         Lignes de code logiques 67 Procédures possiblement inutilisées 1",
        "Fichiers par statut Analysé 4 Sans macros 1 Chiffré 1 Corrompu 1 Non pris en charge 1 \
         Trop volumineux 1 Archive suspecte 1 Erreur 1",
        "Fichiers par complexité Faible 4 Moyenne 1",
        "Fichiers par gravité maximale Moyenne 3 Élevée 1",
        "Procédures par famille Mise en forme 1 Recherche et référentiel 1 \
         Formulaires de saisie 2 Événements 2 Non classée 1",
    ] {
        assert!(fr.contains(expected), "{expected:?} in {fr}");
    }

    // A full bar is the 11 files of the scan, in each of the 12 rows of the first
    // three tables, and the 7 procedures of the summary, in the 5 of the last.
    let page = page(&report, Lang::En);
    let bar =
        |value: u32| format!(r#"<meter min="0" max="11" value="{value}" aria-hidden="true">"#);
    assert_eq!(page.matches("<meter ").count(), 17);
    assert_eq!(page.matches(&bar(4)).count(), 2);
    assert_eq!(page.matches(&bar(3)).count(), 1);
    assert_eq!(page.matches(&bar(1)).count(), 9);
    assert_eq!(page.matches(r#"<meter min="0" max="7" "#).count(), 5);
}

#[test]
fn each_number_is_shown_with_its_own_label() {
    // No two numbers are alike, so a label on the wrong field would show.
    let mut report = common::fleet();
    report.scan.files_total = 31;
    report.scan.files_with_macros = 32;
    report.scan.files_failed = 33;
    report.summary.totals = Some(Totals {
        modules: Some(34),
        procedures: Some(35),
        logical_loc: Some(36),
        possibly_unused_procedures: Some(37),
    });
    report.summary.by_status = BTreeMap::from([
        (Status::Ok, 41),
        (Status::NoMacros, 42),
        (Status::Encrypted, 43),
        (Status::Corrupted, 44),
        (Status::Unsupported, 45),
        (Status::TooLarge, 46),
        (Status::SuspiciousArchive, 47),
        (Status::Error, 48),
    ]);
    report.summary.by_complexity =
        BTreeMap::from([(Level::Low, 51), (Level::Medium, 52), (Level::High, 53)]);
    report.summary.by_severity = BTreeMap::from([
        (Severity::Info, 61),
        (Severity::Low, 62),
        (Severity::Medium, 63),
        (Severity::High, 64),
    ]);
    report.summary.by_family = BTreeMap::from([
        ("01-formatting".to_owned(), 71),
        ("05-forms".to_owned(), 72),
        ("13-windows-api".to_owned(), 73),
        ("unclassified".to_owned(), 74),
    ]);

    let page = page(&report, Lang::En);
    let shown = words(&markup(&page));
    for expected in [
        "Totals Files 31 Files with macros 32 Files that failed 33 Modules 34 Procedures 35 \
         Logical lines of code 36 Possibly unused procedures 37",
        "Files by status Analysed 41 No macros 42 Encrypted 43 Corrupted 44 Unsupported 45 \
         Too large 46 Suspicious archive 47 Error 48",
        "Files by complexity Low 51 Medium 52 High 53",
        "Files by highest severity Info 61 Low 62 Medium 63 High 64",
        "Procedures by family Formatting and layout 71 Forms and data entry 72 \
         Windows API, shell and ActiveX 73 Unclassified 74",
    ] {
        assert!(shown.contains(expected), "{expected:?} in {shown}");
    }
    // A bar is a share of the files of the scan, or of the procedures that the
    // rows of the families add up to (71 + 72 + 73 + 74).
    assert_eq!(page.matches(r#"<meter min="0" max="31" "#).count(), 15);
    assert_eq!(page.matches(r#"<meter min="0" max="290" "#).count(), 4);
}

#[test]
fn the_totals_that_the_summary_lacks_are_left_out() {
    let mut report = common::fleet();
    report.summary.totals = None;
    let shown = words(&markup(&page(&report, Lang::En)));
    assert!(
        shown.contains("Totals Files 11 Files with macros 4 Files that failed 4 Files by status"),
        "{shown}"
    );

    report.summary.totals = Some(Totals {
        modules: Some(5),
        procedures: None,
        logical_loc: Some(67),
        possibly_unused_procedures: None,
    });
    let shown = words(&markup(&page(&report, Lang::En)));
    assert!(
        shown.contains(
            "Totals Files 11 Files with macros 4 Files that failed 4 Modules 5 \
             Logical lines of code 67 Files by status"
        ),
        "{shown}"
    );
}

#[test]
fn each_file_has_its_path_chips_and_facts() {
    let shown = words(&markup(&page(&common::fleet(), Lang::En)));

    // The first file of the fleet: a workbook with a project that is not
    // protected, and no error.
    assert!(shown.contains(
        "examples/14-01-auto-open-shell.xlsm Status Analysed Complexity Low \
         Highest severity High Family Event-driven automation Format XLSM"
    ));
    // A file with no family has no chip for it.
    assert!(shown.contains(
        "legacy/xlm-only.xlsb Status Analysed Complexity Low Highest severity Medium Format"
    ));
    assert!(shown.contains(
        "Format XLSM Size 14321 bytes SHA-256 \
         f01be4b0b98c36f5ad66974216461d080278970f204e50c92f41ada6cf24187e \
         VBA project VBAProject References Visual Basic For Applications \
         Microsoft Excel 16.0 Object Library OLE Automation Microsoft Office 16.0 Object Library"
    ));
    // A protected project, Excel 4.0 macros and their sheets, a workbook that is
    // encrypted, an error and a note.
    assert!(shown.contains("VBA project BudgetTools Protected Yes"));
    assert!(shown.contains("Excel 4.0 macros Yes Excel 4.0 macro sheets 2"));
    assert!(shown.contains("Workbook encrypted Yes"));
    assert!(shown.contains("Errors ooxml.encrypted The package is password-protected"));
    assert!(shown.contains("Notes encrypted_default_password_possible"));
    // Each one is shown for the one file of the fleet that has it, and not as a
    // "No" for the others.
    for fact in ["Protected", "Excel 4.0 macros", "Workbook encrypted"] {
        assert_eq!(shown.matches(fact).count(), 1, "{fact}");
    }
}

#[test]
fn a_file_lists_its_traits_families_and_signals_with_their_labels() {
    let report = common::fleet();
    let en = words(&markup(&page(&report, Lang::En)));
    let fr = words(&markup(&page(&report, Lang::Fr)));

    // Two families with one score and a chain that two lines raised; the
    // families of a file in the order of the report, which is that of their
    // scores; a trait; a signal with no place and no line.
    for expected in [
        "Families Event-driven automation score 3 Windows API, shell and ActiveX score 3 \
         Security signals High Auto-execution combined with program launch \
         sec.chain.autoexec-execution sec.auto-exec Line 4 sec.shell Line 6",
        "Families Forms and data entry score 8 Formatting and layout score 7 \
         Lookup and reference data score 6 Event-driven automation score 5 \
         Report generation and printing score 4",
        "Traits Recorded macro style",
        "Security signals Medium Contains Excel 4.0 (XLM) macro sheets sec.xlm-macrosheet",
    ] {
        assert!(en.contains(expected), "{expected:?} in {en}");
    }
    for expected in [
        "Familles Événements score 3 API Windows et ActiveX score 3 Signaux de sécurité \
         Élevée Exécution automatique combinée au lancement d'un programme \
         sec.chain.autoexec-execution sec.auto-exec Ligne 4 sec.shell Ligne 6",
        "Caractéristiques Macro enregistrée",
    ] {
        assert!(fr.contains(expected), "{expected:?} in {fr}");
    }
    // A section is there for the files that have something to list in it.
    for (heading, files) in [("Families", 3), ("Security signals", 3), ("Traits", 1)] {
        assert_eq!(en.matches(heading).count(), files, "{heading}");
    }
}

#[test]
fn the_lines_that_raised_a_family_or_a_signal_are_shown_with_where_they_are() {
    let evidence = |rule_id: &str, line, snippet: Option<&str>| Evidence {
        rule_id: rule_id.to_owned(),
        line,
        snippet: snippet.map(str::to_owned),
    };
    let mut report = common::fleet();
    let families = report.files[0].families.as_mut().expect("families");
    families[0].evidence = Some(vec![
        evidence("evt.auto-macros", 12, Some("Sub Auto_Open()")),
        evidence("evt.handler", 30, None),
    ]);
    // A signal raised in a procedure of a module, in a module, in a procedure.
    for (file, module, procedure) in [
        (0, Some("M_14_01"), Some("Auto_Open")),
        (2, Some("XlmSheet"), None),
        (7, None, Some("Unpack")),
    ] {
        let signal = &mut report.files[file].signals.as_mut().expect("a signal")[0];
        signal.module = module.map(str::to_owned);
        signal.procedure = procedure.map(str::to_owned);
    }
    let json = serde_json::to_string(&report).expect("a report is JSON");
    assert_eq!(report_problems(&json), Vec::<String>::new());

    let en = words(&markup(&page(&report, Lang::En)));
    for expected in [
        "Families Event-driven automation score 3 evt.auto-macros Line 12 Sub Auto_Open() \
         evt.handler Line 30 Windows API, shell and ActiveX score 3",
        "sec.chain.autoexec-execution M_14_01.Auto_Open sec.auto-exec Line 4",
        "sec.xlm-macrosheet XlmSheet locked/payroll.xlsm",
        "sec.suspicious-archive Unpack huge/archive.xlsb",
    ] {
        assert!(en.contains(expected), "{expected:?} in {en}");
    }
}

/// A hostile string for `place`, which `strings` holds from then on.
fn hostile_for(strings: &mut Vec<String>, place: &str) -> String {
    let string = format!("{place}#{}:{HOSTILE}", strings.len());
    strings.push(string.clone());
    string
}

/// The fleet with a hostile string, each one different, as the name of a family,
/// a signal and a trait, as a place and as a line of evidence, in the first
/// file and in the summary. The rules know none of those names. The report is
/// returned with the strings.
fn hostile_names() -> (Report, Vec<String>) {
    let mut strings = Vec::new();
    let evidence_of = |strings: &mut Vec<String>, place: &str| {
        Some(vec![Evidence {
            rule_id: hostile_for(strings, &format!("{place} rule")),
            line: 7,
            snippet: Some(hostile_for(strings, &format!("{place} snippet"))),
        }])
    };
    let mut report = common::fleet();
    let file = &mut report.files[0];
    file.primary_family = Some(hostile_for(&mut strings, "primary"));
    file.traits = Some(vec![hostile_for(&mut strings, "trait")]);
    file.families = Some(vec![FamilyScore {
        family: hostile_for(&mut strings, "family"),
        score: 1.5,
        evidence: evidence_of(&mut strings, "family"),
    }]);
    file.signals = Some(vec![Signal {
        signal_id: hostile_for(&mut strings, "signal"),
        severity: Severity::High,
        module: Some(hostile_for(&mut strings, "module")),
        procedure: Some(hostile_for(&mut strings, "procedure")),
        evidence: evidence_of(&mut strings, "signal"),
    }]);
    let family = hostile_for(&mut strings, "by family");
    report.summary.by_family.insert(family, 1);
    (report, strings)
}

#[test]
fn the_names_that_a_file_gives_to_families_signals_and_places_are_shown_as_text() {
    let (report, shown) = hostile_names();
    // A score is written as the language writes a number.
    for (lang, score) in [(Lang::En, "score 1.5"), (Lang::Fr, "score 1,5")] {
        let messages = Messages::new(lang, &rules());
        let markup = markup(&render(&report, &messages).expect("the report renders"));
        let text = visible_text(&markup);
        for string in &shown {
            assert!(text.contains(string), "{string:?} is shown as it is");
            assert!(!markup.contains(string), "{string:?} is not markup");
        }
        for tag in tags(&markup) {
            assert!(ELEMENTS.contains(&tag.name.as_str()), "<{}>", tag.name);
            for (name, value) in tag.attributes {
                assert!(
                    is_allowed(&tag.name, &name, &value),
                    "<{} {name}>",
                    tag.name
                );
            }
        }
        assert!(words(&markup).contains(score), "{score}");

        // The page asked for a label of each of the five names that carry one
        // (three families, a signal and a trait), and for no word that is missing.
        let problems = messages.problems();
        assert_eq!(problems.len(), 5, "{problems:?}");
        assert!(
            problems.iter().all(|p| p.contains(" label '")),
            "{problems:?}"
        );
    }
}

#[test]
fn the_toolbar_is_hidden_until_the_script_shows_it() {
    let page = page(&common::fleet(), Lang::En);
    assert!(page.contains(r#"<div id="toolbar" class="toolbar" hidden>"#));
    assert!(page.contains(r#"<output id="shown">11 / 11</output>"#));
    // The script is what shows it. No test runs the script, so this guards the
    // line only by its text.
    assert!(SCRIPT.contains("toolbar.hidden = false;"));
}

#[test]
fn a_report_on_no_file_says_so() {
    let report = empty();
    let en_markup = markup(&page(&report, Lang::En));
    let en = words(&en_markup);
    // The four distributions have no row, and the list has no file, so it has
    // nothing to filter either.
    assert_eq!(en.matches("None.").count(), 5, "{en}");
    assert!(!en_markup.contains("toolbar"));
    assert!(
        en.contains(
            "Totals Files 0 Files with macros 0 Files that failed 0 Modules 0 Procedures 0 \
             Logical lines of code 0 Possibly unused procedures 0"
        ),
        "{en}"
    );
    let fr = words(&markup(&page(&report, Lang::Fr)));
    assert_eq!(fr.matches("Aucun.").count(), 5, "{fr}");
}

#[test]
fn a_redacted_report_says_so() {
    let mut report = common::fleet();
    let plain = visible_text(&markup(&page(&report, Lang::En)));
    report.redacted = true;
    let redacted = visible_text(&markup(&page(&report, Lang::En)));
    assert!(!plain.contains("Redacted report"));
    assert!(redacted.contains("Redacted report"));
    assert!(visible_text(&markup(&page(&report, Lang::Fr))).contains("Rapport caviardé"));
}

#[test]
fn the_page_is_the_same_each_time() {
    let report = common::fleet();
    for lang in LANGS {
        assert_eq!(page(&report, lang), page(&report, lang));
    }
}

#[test]
fn the_body_of_the_page_on_the_fleet() {
    let markup = markup(&page(&common::fleet(), Lang::En));
    let (_, body) = markup.split_once("<body>").expect("a body");
    insta::assert_snapshot!("fleet_en", body);
}
