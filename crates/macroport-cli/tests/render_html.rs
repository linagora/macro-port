// SPDX-License-Identifier: Apache-2.0
//! The HTML report: what it shows, and what an analysed file cannot make it do
//! (SPEC section 8, A7).

mod common;

use std::collections::BTreeSet;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use macroport_cli::i18n::{Messages, RESOURCES};
use macroport_cli::render::html::render;
use macroport_core::report::{Lang, Report};
use macroport_core::rules::{RuleSet, RulesSources};
use macroport_testkit::report_problems;
use sha2::{Digest, Sha256};

const TEMPLATE: &str = include_str!("../templates/report.html");
const RENDERER: &str = include_str!("../src/render/html.rs");
const STYLE: &str = include_str!("../templates/report.css");

const LANGS: [Lang; 2] = [Lang::En, Lang::Fr];

/// What a hostile file can put in a string of the report: the start of a
/// comment, of a script and of an element with a handler, the end of a script
/// (with a NUL inside, and in capitals), both quotes and an ampersand.
const HOSTILE: &str = "<!--<script>x</scr\0ipt></SCRIPT >\"'&<img src=x onerror=alert(1)>";

/// The element that holds the report as JSON.
const DATA_OPEN: &str = r#"<script type="application/json" id="report-data">"#;
const POLICY_OPEN: &str = r#"<meta http-equiv="Content-Security-Policy" content=""#;

/// The page of `report`, which asks for no message that does not exist.
fn page(report: &Report, lang: Lang) -> String {
    let messages = Messages::new(lang);
    let page = render(report, &messages).expect("the report renders");
    assert_eq!(messages.problems(), Vec::<String>::new(), "{lang:?}");
    page
}

/// A report on no file, with no root and no duration.
fn empty() -> Report {
    let rules = RuleSet::load(&RulesSources::EMBEDDED).expect("the shipped rules load");
    Report::empty(&rules, "1970-01-01T00:00:00Z".to_owned(), Lang::En)
}

/// The fleet with a hostile string, each one different, in every string that
/// the page shows. The report is no longer valid, and the strings are returned.
fn hostile() -> (Report, Vec<String>) {
    let mut report = common::fleet();
    let mut strings = Vec::new();
    let mut hostile = |field: &str| {
        let string = format!("{field}:{HOSTILE}");
        strings.push(string.clone());
        string
    };
    report.generated_at = hostile("generated_at");
    report.tool.name = hostile("tool.name");
    report.tool.version = hostile("tool.version");
    report.tool.rules_version = hostile("tool.rules_version");
    report.scan.roots = Some(vec![hostile("root")]);
    (report, strings)
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

/// The page without what lies in its `<style>` and in its data block: the
/// markup that the template writes around the text.
fn markup(page: &str) -> String {
    let mut markup = page.to_owned();
    for (open, close) in [("<style>", "</style>"), (DATA_OPEN, "</script>")] {
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
fn the_policy_comes_first_and_allows_the_inline_style_by_hash_only() {
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
        assert_eq!(
            directives,
            [
                ("default-src", vec!["'none'"]),
                ("style-src", vec![style.as_str()]),
                ("img-src", vec!["data:"]),
                ("base-uri", vec!["'none'"]),
                ("form-action", vec!["'none'"]),
            ]
        );
    }
}

#[test]
fn the_page_has_one_style_and_no_script_but_the_data_block() {
    let page = page(&common::injection(), Lang::En);
    let markup = markup(&page);
    assert_eq!(markup.matches("<style").count(), 1);
    assert_eq!(markup.matches("<script").count(), 1);
    assert_eq!(markup.matches("</script").count(), 1);
    assert_eq!(markup.matches("<link").count(), 1);
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

#[test]
fn what_an_analysed_file_names_is_shown_as_text() {
    let injection = common::injection();
    let roots = injection.scan.roots.clone().expect("the fixture has roots");
    assert_eq!(roots, ["<img src=x onerror=alert(1)>"]);
    let (hostile, strings) = hostile();

    for (report, shown) in [(injection, roots), (hostile, strings)] {
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

const ELEMENTS: [&str; 18] = [
    "html", "head", "meta", "title", "link", "style", "body", "header", "h1", "dl", "div", "dt",
    "dd", "p", "ul", "li", "code", "script",
];

/// Whether the page may write `element` with `attribute` set to `value`. What
/// comes from the report is never among the values: the page writes only fixed
/// words.
fn is_allowed(element: &str, attribute: &str, value: &str) -> bool {
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
        (_, "class") => ["facts", "notice"].contains(&value),
        _ => false,
    }
}

#[test]
fn the_markup_has_only_known_elements_and_attributes_whatever_the_report_says() {
    for report in [common::fleet(), common::injection(), empty(), hostile().0] {
        for lang in LANGS {
            let tags = tags(&markup(&page(&report, lang)));
            assert!(tags.len() > 30, "the page has markup");
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
fn the_style_makes_no_request() {
    for token in ["@import", "url(", "expression(", "behavior"] {
        assert!(!STYLE.contains(token), "the style has no `{token}`");
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
        assert!(
            ["if", "else", "endif", "for", "endfor"].contains(&keyword),
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
fn only_the_style_and_the_data_are_marked_safe() {
    // A `Safe` value is written as it is, without escaping.
    assert_eq!(RENDERER.matches("Safe(").count(), 2);
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
    let asked: BTreeSet<&str> = (TEMPLATE.split('"'))
        .filter(|piece| is_html_id(piece))
        .collect();
    assert_eq!(asked, defined);

    for lang in LANGS {
        let messages = Messages::new(lang);
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
    assert!(visible_text(&markup(&en)).contains("Scan duration"));
    assert!(fr.contains("<html lang=\"fr\">"));
    assert!(fr.contains("<title>Rapport Macro-Port</title>"));
    assert!(visible_text(&markup(&fr)).contains("Durée de l’analyse"));
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
