// SPDX-License-Identifier: Apache-2.0
//! Projects written by hand, for the tests of the engine and of the code that
//! builds on it.

use regex::Regex;

use super::eval::Evaluator;
use super::hit::Hit;
use super::input::{
    CallSite, CodeLine, MemberUse, ModuleInput, NameSet, ParserPredicates, ProcedureInput,
    ProjectInput,
};
use crate::report::ProcedureKind;
use crate::rules::{Matcher, ModuleType, StructuralPredicate, TextMode};

/// A statement on the physical line `number`, whose text is the VBA `raw`.
/// Its `code`, `comment` and `strings` are read from `raw`; the rest is empty
/// and the builder methods add what the parser would have found.
pub(crate) fn line(number: u32, raw: &str) -> CodeLine {
    let mut line = CodeLine {
        line: number,
        raw: raw.to_owned(),
        ..CodeLine::default()
    };
    read_text(&mut line);
    line
}

/// Fills `code`, `comment` and `strings` from `raw`. This is a simplified
/// reading for the hand-made lines of the tests, not the lexer of M2: a string
/// literal becomes `""` in the code and an entry of `strings`, a doubled quote
/// inside a literal is one quote, and a `'` outside a literal, or a statement
/// that starts with the word `Rem`, starts the comment, which runs to the end
/// of the line. A test gives one statement per line, with no continuation.
fn read_text(line: &mut CodeLine) {
    if let Some(comment) = rem_comment(&line.raw) {
        line.comment = Some(comment.to_owned());
        return;
    }
    let mut chars = line.raw.chars().peekable();
    while let Some(next) = chars.next() {
        match next {
            '"' => {
                let mut literal = String::new();
                while let Some(inner) = chars.next() {
                    if inner == '"' && chars.next_if_eq(&'"').is_none() {
                        break;
                    }
                    literal.push(inner);
                }
                line.code.push_str("\"\"");
                line.strings.push(literal);
            }
            '\'' => line.comment = Some(chars.by_ref().collect()),
            _ => line.code.push(next),
        }
    }
}

/// The text after `Rem` when the statement is a `Rem` comment. `Rem` is a word
/// of its own: `Remark = 1` is not a comment.
fn rem_comment(statement: &str) -> Option<&str> {
    let statement = statement.trim_start();
    let word = statement.get(..3)?;
    let rest = statement.get(3..)?;
    let own_word = rest.is_empty() || rest.starts_with([' ', '\t']);
    (word.eq_ignore_ascii_case("rem") && own_word).then_some(rest)
}

impl CodeLine {
    /// Replaces the string literals by the values that the parser would give
    /// after folding, as in `"WScr" & "ipt.Shell"`.
    pub(crate) fn with_strings(mut self, folded: &[&str]) -> Self {
        self.strings = folded.iter().map(|value| (*value).to_owned()).collect();
        self
    }

    /// Adds an identifier used as a member or as a standalone name.
    pub(crate) fn member(mut self, name: &str) -> Self {
        self.members.push(MemberUse {
            name: name.to_owned(),
            named_argument: false,
        });
        self
    }

    /// Adds an identifier that names an argument, as in `Name:=value`.
    pub(crate) fn named_argument(mut self, name: &str) -> Self {
        self.members.push(MemberUse {
            name: name.to_owned(),
            named_argument: true,
        });
        self
    }

    /// Adds a member chain, given dot-separated.
    pub(crate) fn chain(mut self, chain: &str) -> Self {
        self.chains
            .push(chain.split('.').map(str::to_owned).collect());
        self
    }

    /// Adds a name invoked on its own.
    pub(crate) fn call(mut self, name: &str) -> Self {
        self.calls.push(CallSite {
            name: name.to_owned(),
            after_dot: false,
        });
        self
    }

    /// Adds a name invoked after `.`.
    pub(crate) fn call_after_dot(mut self, name: &str) -> Self {
        self.calls.push(CallSite {
            name: name.to_owned(),
            after_dot: true,
        });
        self
    }

    /// Adds a ProgID passed to `CreateObject` or `GetObject`.
    pub(crate) fn progid(mut self, progid: &str) -> Self {
        self.progids.push(progid.to_owned());
        self
    }
}

/// A `Sub` of the given name on the physical line `start_line`, holding
/// `body`. The declaration and the `End Sub` line are added around it.
pub(crate) fn procedure(name: &str, start_line: u32, body: Vec<CodeLine>) -> ProcedureInput {
    let end_line = body
        .last()
        .map_or(start_line, |last| last.line)
        .saturating_add(1);
    let mut lines = vec![line(start_line, &format!("Sub {name}()"))];
    lines.extend(body);
    lines.push(line(end_line, "End Sub"));
    ProcedureInput {
        name: name.to_owned(),
        kind: ProcedureKind::Sub,
        start_line,
        end_line,
        lines,
        predicates: ParserPredicates::default(),
    }
}

/// A module with these procedures and nothing else.
pub(crate) fn module(
    name: &str,
    module_type: ModuleType,
    procedures: Vec<ProcedureInput>,
) -> ModuleInput {
    ModuleInput {
        name: name.to_owned(),
        module_type,
        with_events: NameSet::default(),
        controls: NameSet::default(),
        declarations: Vec::new(),
        procedures,
        parse_recovered: false,
    }
}

impl ModuleInput {
    /// Sets the lines outside the procedures.
    pub(crate) fn with_declarations(mut self, declarations: Vec<CodeLine>) -> Self {
        self.declarations = declarations;
        self
    }

    /// Marks the module as one that the parser recovered in.
    pub(crate) fn recovered(mut self) -> Self {
        self.parse_recovered = true;
        self
    }

    /// Sets the names of the `WithEvents` variables.
    pub(crate) fn with_events(mut self, names: &[&str]) -> Self {
        self.with_events = names.iter().collect();
        self
    }

    /// Sets the names of the controls of a form.
    pub(crate) fn with_controls(mut self, names: &[&str]) -> Self {
        self.controls = names.iter().collect();
        self
    }
}

/// A project of these modules, with no reference.
pub(crate) fn project(modules: Vec<ModuleInput>) -> ProjectInput {
    ProjectInput {
        references: Vec::new(),
        modules,
    }
}

impl ProjectInput {
    /// Sets the display names of the references of the project.
    pub(crate) fn with_references(mut self, references: &[&str]) -> Self {
        self.references = owned(references);
        self
    }
}

/// A project of one standard module that holds one procedure, whose body is
/// `body`.
pub(crate) fn project_of(body: &[CodeLine]) -> ProjectInput {
    project(vec![module(
        "Module1",
        ModuleType::Standard,
        vec![procedure("Main", 1, body.to_vec())],
    )])
}

fn owned(list: &[&str]) -> Vec<String> {
    list.iter().map(|item| (*item).to_owned()).collect()
}

/// A `member` matcher for these names.
pub(crate) fn members(names: &[&str]) -> Matcher {
    Matcher::Member(owned(names))
}

/// A `chain` matcher for these dot-separated patterns.
pub(crate) fn chains(patterns: &[&str]) -> Matcher {
    Matcher::Chain(owned(patterns))
}

/// A `call` matcher for these names.
pub(crate) fn calls(names: &[&str]) -> Matcher {
    Matcher::Call(owned(names))
}

/// A `progid` matcher for these glob patterns.
pub(crate) fn progids(patterns: &[&str]) -> Matcher {
    Matcher::ProgId(owned(patterns))
}

/// The physical lines of the hits that have one.
pub(crate) fn lines_of(hits: &[Hit<'_>]) -> Vec<u32> {
    hits.iter()
        .filter_map(|hit| hit.place)
        .map(|place| place.line)
        .collect()
}

/// The module and the line of each hit that has a place.
pub(crate) fn places(hits: &[Hit<'_>]) -> Vec<(usize, u32)> {
    hits.iter()
        .filter_map(|hit| hit.place)
        .map(|place| (place.module, place.line))
        .collect()
}

/// The snippet of each hit.
pub(crate) fn snippets(hits: &[Hit<'_>]) -> Vec<String> {
    hits.iter().map(Hit::snippet).collect()
}

/// The lines of the hits of `matcher` in a procedure whose body is `body`,
/// with the `text` key of the signal.
pub(crate) fn lines_hit_with_text(
    body: &[CodeLine],
    matcher: &Matcher,
    text: TextMode,
) -> Vec<u32> {
    let project = project_of(body);
    lines_of(&Evaluator::new(&project).procedure_hits(0, 0, matcher, text))
}

/// The lines of the hits of `matcher` in a procedure whose body is `body`.
pub(crate) fn lines_hit(body: &[CodeLine], matcher: &Matcher) -> Vec<u32> {
    lines_hit_with_text(body, matcher, TextMode::Code)
}

/// The regexes of a rule, as the matchers receive them. The rules loader makes
/// them case-insensitive and the matchers add nothing to them: a test that
/// needs the case ignored says `(?i)`.
fn compiled(patterns: &[&str]) -> Vec<Regex> {
    patterns
        .iter()
        .map(|pattern| Regex::new(pattern).unwrap())
        .collect()
}

/// A `procedure_name` matcher for these regexes.
pub(crate) fn procedure_names(patterns: &[&str]) -> Matcher {
    Matcher::ProcedureName(compiled(patterns))
}

/// A `reference` matcher for these regexes.
pub(crate) fn references(patterns: &[&str]) -> Matcher {
    Matcher::Reference(compiled(patterns))
}

/// A `regex` matcher for these regexes.
pub(crate) fn regexes(patterns: &[&str]) -> Matcher {
    Matcher::Regex(compiled(patterns))
}

/// A `string` matcher for these regexes.
pub(crate) fn strings(patterns: &[&str]) -> Matcher {
    Matcher::String(compiled(patterns))
}

/// A `comment` matcher for these regexes.
pub(crate) fn comments(patterns: &[&str]) -> Matcher {
    Matcher::Comment(compiled(patterns))
}

/// A `structural` matcher for these predicates.
pub(crate) fn structurals(predicates: &[StructuralPredicate]) -> Matcher {
    Matcher::Structural(predicates.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_is_read_into_its_code_its_strings_and_its_comment() {
        // An apostrophe in a literal does not start the comment, and a doubled
        // quote in a literal is one quote.
        let read = line(1, r#"x = "a ' b" & "c""d" ' end"#);
        assert_eq!(read.code, r#"x = "" & "" "#);
        assert_eq!(read.strings, ["a ' b", r#"c"d"#]);
        assert_eq!(read.comment.as_deref(), Some(" end"));
        let apostrophe = line(1, r#"x = "it's""#);
        assert_eq!(apostrophe.strings, ["it's"]);
        assert_eq!(apostrophe.comment, None);
        let plain = line(1, "x = 1");
        assert_eq!(plain.code, "x = 1");
        assert!(plain.strings.is_empty());
        assert_eq!(plain.comment, None);
    }

    #[test]
    fn rem_starts_a_comment_only_as_a_word_of_its_own() {
        let note = line(1, "  rem a note");
        assert_eq!(note.comment.as_deref(), Some(" a note"));
        assert_eq!(note.code, "");
        assert_eq!(line(1, "REM").comment.as_deref(), Some(""));
        let word = line(1, "Remark = 1");
        assert_eq!(word.comment, None);
        assert_eq!(word.code, "Remark = 1");
    }
}
