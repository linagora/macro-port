// SPDX-License-Identifier: Apache-2.0
//! The input of the rule engine, from what the front end makes of a module.
//!
//! [`adapt`] turns the lines and the procedures of a module into the model
//! that the engine reads ([`ModuleInput`]), and measures the procedures on the
//! way. Three conventions matter to the engine:
//!
//! - The line numbers are the physical lines of the file, never renumbered
//!   (SPEC section 5): the parser has kept them, so they are copied.
//! - Every logical line outside a procedure goes to the declarations of the
//!   module, the statements and the comments with the declarations proper
//!   (ADR-0004): the modules that have no procedure are scored on them.
//! - The strings of a statement are the ones that constant folding gives, read
//!   with the constants of the module, whichever procedure uses them.
//!
//! What the front end does not read yet is left empty, not guessed: the
//! members, the chains, the calls and the ProgIDs of the statements, and the
//! predicates `udf_candidate`, `chr_obfuscation` and `possibly_unused`.

use std::ops::Range;

use super::modules::Analysis;
use crate::engine::{CodeLine, ModuleInput, NameSet, ParserPredicates, ProcedureInput};
use crate::report::Metrics;
use crate::rules::ModuleType;
use crate::vba::{
    ConstTable, DeclarationKind, LogicalKind, LogicalLine, Procedure, StatementReader, Token,
    TokenKind, measure_procedure, tokenize,
};

/// A module as the rule engine and the metrics read it.
pub(super) struct Adapted {
    /// What the rule engine reads.
    pub(super) input: ModuleInput,
    /// What the metrics of the project need of the module.
    pub(super) measured: Measured,
}

impl Adapted {
    /// A module that was not read: no source, or too many lines to read. It is
    /// listed all the same, so that the modules of the engine are those of the
    /// report, in the same order.
    pub(super) fn unread(name: &str, module_type: ModuleType) -> Self {
        Self {
            input: module_input(name, module_type),
            measured: Measured::default(),
        }
    }
}

/// What the front end measured of a module, beyond what the engine reads.
#[derive(Debug, Default)]
pub(super) struct Measured {
    /// Whether the front end read the module.
    pub(super) read: bool,
    /// The metrics of the procedures, in the order of the procedures of the
    /// module: what the parser's lines give, without the metrics that need the
    /// whole project.
    pub(super) procedures: Vec<Metrics>,
    /// The names that the module gives to the routines it declares with
    /// `Declare`.
    pub(super) declared: Vec<String>,
}

/// An empty module.
fn module_input(name: &str, module_type: ModuleType) -> ModuleInput {
    ModuleInput {
        name: name.to_owned(),
        module_type,
        with_events: NameSet::default(),
        controls: NameSet::default(),
        declarations: Vec::new(),
        procedures: Vec::new(),
        parse_recovered: false,
    }
}

/// Adapts the module `name` of type `module_type`, which the front end read as
/// `analysis`.
pub(super) fn adapt(name: &str, module_type: ModuleType, analysis: &Analysis) -> Adapted {
    let logical = &analysis.lines.logical;
    let parsed = &analysis.module;
    let outside = gaps(logical.len(), &parsed.procedures);

    // The constants come first: a statement of any procedure may use one.
    let mut consts = ConstTable::default();
    for line in outside.iter().flat_map(|gap| slice(logical, gap)) {
        consts.declare(line);
    }
    let mut converter = Converter {
        reader: StatementReader::new(&consts),
    };

    let mut input = module_input(name, module_type);
    let mut too_deep = false;
    for gap in &outside {
        let (code, deep) = converter.code_lines(slice(logical, gap));
        input.declarations.extend(code);
        too_deep |= deep;
    }
    let mut procedures = Vec::with_capacity(parsed.procedures.len());
    let mut metrics = Vec::with_capacity(parsed.procedures.len());
    for procedure in &parsed.procedures {
        let body = slice(logical, &procedure.lines);
        let measure = measure_procedure(
            procedure.start_line,
            procedure.end_line,
            procedure.parameter_count,
            body,
        );
        let (lines, deep) = converter.code_lines(body);
        too_deep |= deep;
        procedures.push(ProcedureInput {
            name: procedure.name.clone(),
            kind: procedure.kind,
            start_line: procedure.start_line,
            end_line: procedure.end_line,
            lines,
            predicates: ParserPredicates {
                nested_loop_compare: measure.nested_loop_compare,
                parse_recovered: procedure.is_recovered() || deep,
                ..ParserPredicates::default()
            },
        });
        metrics.push(measure.metrics);
    }
    input.procedures = procedures;
    input.parse_recovered = parsed.is_recovered() || too_deep;

    let with_events =
        parsed
            .declarations
            .iter()
            .filter_map(|declaration| match &declaration.kind {
                DeclarationKind::WithEvents { name } => Some(name.as_str()),
                _ => None,
            });
    input.with_events = with_events.collect();
    let declared = parsed
        .declarations
        .iter()
        .filter_map(|declaration| match &declaration.kind {
            DeclarationKind::Declare { name, .. } if !name.is_empty() => Some(name.clone()),
            _ => None,
        });

    Adapted {
        input,
        measured: Measured {
            read: true,
            procedures: metrics,
            declared: declared.collect(),
        },
    }
}

/// The lines of `range`. None when the range leaves the lines, which the parser
/// does not give.
fn slice<'a>(lines: &'a [LogicalLine], range: &Range<usize>) -> &'a [LogicalLine] {
    lines.get(range.clone()).unwrap_or_default()
}

/// The runs of logical lines that no procedure holds, in order. The procedures
/// do not overlap, but a hostile module is no reason to count on it.
fn gaps(count: usize, procedures: &[Procedure]) -> Vec<Range<usize>> {
    let mut gaps = Vec::new();
    let mut next = 0;
    for procedure in procedures {
        if procedure.lines.start > next {
            gaps.push(next..procedure.lines.start);
        }
        next = next.max(procedure.lines.end);
    }
    if count > next {
        gaps.push(next..count);
    }
    gaps
}

/// Reads the logical lines of a module into the lines of the engine.
struct Converter<'c> {
    reader: StatementReader<'c>,
}

impl Converter<'_> {
    /// The lines of the engine for `lines`, and whether one of the statements
    /// nests expressions deeper than the reader follows.
    fn code_lines(&mut self, lines: &[LogicalLine]) -> (Vec<CodeLine>, bool) {
        let mut too_deep = false;
        let code = (lines.iter())
            .map(|line| {
                let (code, deep) = self.code_line(line);
                too_deep |= deep;
                code
            })
            .collect();
        (code, too_deep)
    }

    /// The line of the engine for `line`, and whether its statement nests
    /// expressions too deep to be read.
    fn code_line(&mut self, line: &LogicalLine) -> (CodeLine, bool) {
        let comment = line.comment.as_ref().map(|comment| comment.text.clone());
        let raw = raw_text(&line.text, comment.as_deref());
        if line.kind != LogicalKind::Code {
            // A label or a comment alone holds no string and no expression.
            let code = CodeLine {
                line: line.line,
                raw,
                code: line.text.clone(),
                comment,
                ..CodeLine::default()
            };
            return (code, false);
        }
        let tokens = tokenize(&line.text);
        let facts = self.reader.read_tokens(&line.text, &tokens);
        let code = CodeLine {
            line: line.line,
            raw,
            code: without_strings(&line.text, &tokens),
            comment,
            strings: facts.strings,
            ..CodeLine::default()
        };
        (code, facts.too_deep)
    }
}

/// The statement `text` with its `comment` back after it, as the engine reads
/// `raw`. The parser cuts the comment off and does not keep what marked it, so
/// an apostrophe stands for it.
fn raw_text(text: &str, comment: Option<&str>) -> String {
    match comment {
        None => text.to_owned(),
        Some(comment) if text.is_empty() => format!("' {comment}"),
        Some(comment) => format!("{text} ' {comment}"),
    }
}

/// `text` with each string literal of `tokens` replaced by an empty one, which
/// the line keeps: `Open "" For Output` still reads `Open ... For`.
fn without_strings(text: &str, tokens: &[Token]) -> String {
    let mut code = String::with_capacity(text.len());
    let mut from = 0;
    for token in tokens
        .iter()
        .filter(|token| token.kind == TokenKind::String)
    {
        code.push_str(text.get(from..token.start).unwrap_or_default());
        code.push_str("\"\"");
        from = token.end;
    }
    code.push_str(text.get(from..).unwrap_or_default());
    code
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vba::{MAX_NESTING, parse, split_lines};

    fn analysis(text: &str) -> Analysis {
        let lines = split_lines(text).expect("a short module");
        let module = parse(&lines);
        Analysis { lines, module }
    }

    fn adapted(text: &str) -> Adapted {
        adapt("Module1", ModuleType::Standard, &analysis(text))
    }

    /// The lines of `lines` as (line, raw) pairs.
    fn raws(lines: &[CodeLine]) -> Vec<(u32, &str)> {
        lines
            .iter()
            .map(|line| (line.line, line.raw.as_str()))
            .collect()
    }

    #[test]
    fn the_lines_keep_the_physical_numbers_of_the_file() {
        let adapted = adapted(
            "VERSION 1.0 CLASS\nBEGIN\n  MultiUse = -1\nEND\nAttribute VB_Name = \"Helper\"\n\
             Option Explicit\n\nPublic Sub Run()\n    x = 1 + _\n        2\nEnd Sub\n",
        );

        let input = &adapted.input;
        assert_eq!(raws(&input.declarations), [(6, "Option Explicit")]);
        let run = &input.procedures[0];
        assert_eq!(
            (run.name.as_str(), run.start_line, run.end_line),
            ("Run", 8, 11)
        );
        // A statement over two physical lines has the number of the first.
        assert_eq!(
            raws(&run.lines),
            [(8, "Public Sub Run()"), (9, "x = 1 + 2"), (11, "End Sub")]
        );
    }

    #[test]
    fn every_line_outside_a_procedure_is_a_declaration() {
        let adapted = adapted(
            "Option Explicit\n' the first\nPrivate Const Name = \"x\"\nSet a = Nothing\n\
             Sub Run()\nEnd Sub\n'between\nDeclare Function GetTickCount Lib \"kernel32\" () As Long\n\
             Done:\nSub Late()\nEnd Sub\nx = 2\n",
        );

        let found = raws(&adapted.input.declarations);
        assert_eq!(
            found,
            [
                (1, "Option Explicit"),
                (2, "' the first"),
                (3, "Private Const Name = \"x\""),
                (4, "Set a = Nothing"),
                (7, "' between"),
                (
                    8,
                    "Declare Function GetTickCount Lib \"kernel32\" () As Long"
                ),
                (9, "Done"),
                (12, "x = 2"),
            ]
        );
        let procedures: Vec<_> = (adapted.input.procedures.iter())
            .map(|procedure| procedure.name.as_str())
            .collect();
        assert_eq!(procedures, ["Run", "Late"]);
    }

    #[test]
    fn a_line_has_its_code_its_strings_and_its_comment_apart() {
        let adapted = adapted(
            "Sub Run()\n    Open \"a b\" For Output As #1 ' log it\n    x = \"say \"\"hi\"\"\"\nEnd Sub\n",
        );

        let lines = &adapted.input.procedures[0].lines;
        let open = &lines[1];
        assert_eq!(open.raw, "Open \"a b\" For Output As #1 ' log it");
        assert_eq!(open.code, "Open \"\" For Output As #1");
        assert_eq!(open.comment.as_deref(), Some("log it"));
        assert_eq!(open.strings, ["a b"]);
        let said = &lines[2];
        assert_eq!(said.code, "x = \"\"");
        assert_eq!(said.strings, ["say \"hi\""]);
        assert_eq!(said.comment, None);
    }

    #[test]
    fn a_comment_alone_is_a_line_with_no_code() {
        let adapted = adapted("Sub Run()\n    ' Macro recorded\nEnd Sub\n");

        let note = &adapted.input.procedures[0].lines[1];
        assert_eq!((note.line, note.code.as_str()), (2, ""));
        assert_eq!(note.raw, "' Macro recorded");
        assert_eq!(note.comment.as_deref(), Some("Macro recorded"));
    }

    #[test]
    fn the_strings_are_folded_with_the_constants_of_the_module() {
        let adapted = adapted(
            "Sub Run()\n    Set o = CreateObject(Prog)\n    p = \"WScr\" & \"ipt.Sh\" & \"ell\"\n\
             End Sub\nPrivate Const Prog = \"Excel.\" & \"Application\"\n",
        );

        let lines = &adapted.input.procedures[0].lines;
        // The constant is declared after the procedure, and still folds.
        assert_eq!(lines[1].strings, ["Excel.Application"]);
        assert_eq!(lines[2].strings, ["WScript.Shell"]);
    }

    #[test]
    fn what_the_front_end_does_not_read_yet_is_left_empty() {
        let adapted =
            adapted("Sub Run()\n    Shell \"calc\"\n    x = Chr(65) & Chr(66)\nEnd Sub\n");

        let procedure = &adapted.input.procedures[0];
        assert_eq!(procedure.predicates, ParserPredicates::default());
        for line in &procedure.lines {
            assert!(
                line.members.is_empty() && line.chains.is_empty(),
                "{line:?}"
            );
            assert!(line.calls.is_empty() && line.progids.is_empty(), "{line:?}");
        }
    }

    #[test]
    fn the_with_events_variables_and_the_declared_routines_are_listed() {
        let adapted = adapted(
            "Private WithEvents App As Application\nDeclare Function GetTickCount Lib \"kernel32\" () As Long\n\
             Declare PtrSafe Sub Sleep Lib \"kernel32\" (ByVal ms As Long)\nSub Run()\nEnd Sub\n",
        );

        assert!(adapted.input.with_events.contains("app"));
        assert!(!adapted.input.with_events.contains("Application"));
        assert_eq!(adapted.measured.declared, ["GetTickCount", "Sleep"]);
    }

    #[test]
    fn the_type_and_the_name_of_the_module_are_those_given() {
        let adapted = adapt(
            "ThisWorkbook",
            ModuleType::Document,
            &analysis("Sub Run()\nEnd Sub\n"),
        );

        assert_eq!(adapted.input.name, "ThisWorkbook");
        assert_eq!(adapted.input.module_type, ModuleType::Document);
        assert!(!adapted.input.controls.contains("anything"));
        assert!(adapted.measured.read);
    }

    #[test]
    fn the_parser_recoveries_are_told_to_the_procedure_or_to_the_module() {
        // A procedure with no End, then one that is fine.
        let broken = adapted("Sub Broken()\n    x = 1\nSub Fine()\nEnd Sub\n").input;
        assert_eq!(
            (
                broken.procedures[0].predicates.parse_recovered,
                broken.procedures[1].predicates.parse_recovered
            ),
            (true, false)
        );
        assert!(broken.parse_recovered);

        // An End with nothing to close belongs to no procedure.
        let stray = adapted("Sub Fine()\nEnd Sub\nEnd Sub\n").input;
        assert!(!stray.procedures[0].predicates.parse_recovered);
        assert!(stray.parse_recovered);

        let clean = adapted("Sub Fine()\nEnd Sub\n").input;
        assert!(!clean.parse_recovered);
    }

    #[test]
    fn a_statement_too_deep_to_be_read_counts_as_one_that_did_not_parse() {
        let deep = format!(
            "x = {}1{}",
            "(".repeat(MAX_NESTING + 1),
            ")".repeat(MAX_NESTING + 1)
        );

        let inside = adapted(&format!("Sub Run()\n    {deep}\nEnd Sub\n")).input;
        assert!(inside.procedures[0].predicates.parse_recovered);
        assert!(inside.parse_recovered);
        let outside = adapted(&format!("{deep}\nSub Run()\nEnd Sub\n")).input;
        assert!(!outside.procedures[0].predicates.parse_recovered);
        assert!(outside.parse_recovered);
    }

    #[test]
    fn the_comparisons_in_nested_loops_are_those_of_the_procedure() {
        let adapted = adapted(
            "Sub Run()\n    For i = 1 To 9\n        For j = 1 To 9\n            If a(i) = b(j) Then n = n + 1\n\
             \x20       Next j\n    Next i\nEnd Sub\n",
        );

        let procedure = &adapted.input.procedures[0];
        assert_eq!(procedure.predicates.nested_loop_compare, [3]);
        let measure = &adapted.measured.procedures[0];
        assert_eq!(
            (measure.cyclomatic, measure.max_nesting),
            (Some(4), Some(2))
        );
    }

    #[test]
    fn a_module_that_was_not_read_is_listed_empty() {
        let unread = Adapted::unread("Sheet1", ModuleType::Document);

        assert_eq!(unread.input.name, "Sheet1");
        assert!(unread.input.procedures.is_empty() && unread.input.declarations.is_empty());
        assert!(!unread.measured.read);
    }
}
