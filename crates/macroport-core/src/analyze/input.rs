// SPDX-License-Identifier: Apache-2.0
//! The input of the rule engine, from what the front end makes of a module.
//!
//! [`adapt`] turns the lines and the procedures of a module into the model
//! that the engine reads ([`ModuleInput`]), with what measuring the procedures
//! showed ([`Analysis::measures`]). Seven conventions matter to the engine:
//!
//! - The line numbers are the physical lines of the file, never renumbered
//!   (SPEC section 5): the parser has kept them, so they are copied.
//! - Every logical line outside a procedure goes to the declarations of the
//!   module, the statements and the comments with the declarations proper
//!   (ADR-0004): the modules that have no procedure are scored on them.
//! - The strings of a statement are the ones that constant folding gives, read
//!   with the constants of the module, whichever procedure uses them.
//! - A chain that starts with a dot inside a `With` block continues the object
//!   of the block (`.Font` under `With Range("A1")` is `Range.Font`). The
//!   blocks are followed within a procedure, and within each run of lines
//!   between two procedures: a block never outlives the run that opens it.
//! - The ProgIDs of a statement are the classes that it passes to
//!   `CreateObject` and `GetObject`, folded like its strings. The dependencies
//!   of the procedures and of the file are counted from them (see
//!   [`metrics`](super::metrics)), and the trait `trivial` reads the former.
//! - A statement that shows a mark of obfuscation (SPEC section 7: three calls
//!   of `Chr` joined, or `StrReverse` of a literal) is cited for the predicate
//!   `chr_obfuscation` by its position among the lines of its procedure. The
//!   engine cites the statements of procedures only, so a mark outside any
//!   procedure is cited nowhere.
//! - `parse_recovered` holds for a procedure when the parser recovered in it,
//!   when a statement of it nests expressions deeper than the reader follows
//!   (it reads the rest of that statement flat, so the statement has no ProgID
//!   and no mark of obfuscation past that point), or when its blocks do not
//!   match (SPEC M2 item 6). It holds for the module when it holds for one of
//!   its procedures, or when the parser recovered outside any.
//!
//! What the front end does not read yet is left empty, not guessed: the
//! predicate `possibly_unused`.
//!
//! The facts of a statement weigh a few dozen times its text, and only the text
//! of the file bounds how many statements it holds. The engine keeps what it
//! reads of every module until the last one is read, so the input of a file has
//! a [`Room`] of its own. A statement is refused on what it may weigh, which its
//! tokens tell before its facts are read, and the module that would take a file
//! past the room is not read: it takes nothing of it, and the modules after it
//! may still fit.

use std::ops::Range;

use super::modules::Analysis;
use crate::engine::{
    CallSite, CodeLine, MemberUse, ModuleInput, NameSet, ParserPredicates, ProcedureInput,
};
use crate::report::{Metrics, Status};
use crate::rules::ModuleType;
use crate::vba::{
    self, ConstTable, DeclarationKind, LogicalKind, LogicalLine, Procedure, StatementReader, Token,
    TokenKind, VbaError, tokenize,
};

/// The most bytes that the input of the rule engine may take for the modules of
/// one file, as [`line_bytes`] counts them. A module of 8 MiB made of `a.a.a.`
/// has four million members, which took 220 MB to keep where a file may take
/// 256 MB in all (ACCEPTANCE A2): the lines of a file do not bound its facts,
/// so the facts have a limit of their own.
///
/// The text corpus takes 490 bytes a line, which makes room for 137,000 lines
/// of it, more than a file may hold (`MAX_FILE_LINES`); a line of 88
/// characters and thirteen names takes 1.5 KB and fills the room with 45,000.
/// The worst input measured peaks at 177 MB (release, Linux/glibc): a module of
/// 250,000 one-character lines, which the room refuses once it is full. The
/// measures and the alternatives are in ADR-0009.
const MAX_INPUT_BYTES: usize = 64 * 1024 * 1024;

/// What an allocation costs beyond the bytes it holds: its header, and the
/// rounding of its size.
const ALLOCATION: usize = 16;

/// What the rule engine may still read of the modules of a file, in the bytes
/// of input that [`line_bytes`] counts.
#[derive(Debug, Clone)]
pub(super) struct Room {
    left: usize,
}

impl Room {
    /// The room that the modules of one file share.
    pub(super) fn of_a_file() -> Self {
        Self {
            left: MAX_INPUT_BYTES,
        }
    }

    /// A room of `bytes`.
    #[cfg(test)]
    pub(super) fn of(bytes: usize) -> Self {
        Self { left: bytes }
    }

    /// Takes `bytes` of the room, or fails and takes none when it has less.
    fn take(&mut self, bytes: usize) -> Result<(), VbaError> {
        self.left = self.left.checked_sub(bytes).ok_or_else(too_much_code)?;
        Ok(())
    }

    /// Fails when the room has less than `bytes`, and takes none in any case.
    fn fits(&self, bytes: usize) -> Result<(), VbaError> {
        if bytes > self.left {
            return Err(too_much_code());
        }
        Ok(())
    }
}

/// The error of a module that does not fit in the room of its file.
fn too_much_code() -> VbaError {
    VbaError::new(
        Status::TooLarge,
        "vba.too_much_code",
        format!(
            "the statements of the modules of the file take more than {} MiB to read",
            MAX_INPUT_BYTES / (1024 * 1024)
        ),
    )
}

/// What the facts of a statement take at the most for each of its tokens, the
/// text apart: a name is at most a member, a call and an element of a chain,
/// which are three structs with an allocation each.
const FACTS_PER_TOKEN: usize =
    size_of::<MemberUse>() + size_of::<CallSite>() + size_of::<String>() + 3 * ALLOCATION;

/// How many times the text of a statement may be copied in its line: as its
/// raw text, as its code, and as the name of a member, of a call and of a chain.
const TEXT_COPIES: usize = 5;

/// The most that a copy of the object of a `With` block takes in a chain. The
/// reader keeps an object that weighs 4,096 at the most, where a name weighs
/// 32 more than its bytes: 124 names of one letter. A name takes 8 more bytes
/// here, [`line_bytes`] counting the 24 of its `String` and the 16 of its
/// allocation, which makes 5,088 bytes at the most.
const OBJECT_BYTES: usize = 5_120;

/// How many times the text of a statement may be copied in its line by the
/// copies of the object of a `With` block, besides [`TEXT_COPIES`]. The reader
/// lets a statement copy four times its text in weight (see
/// [`vba::Facts::chains`]), and a weight is a quarter less than the bytes it
/// stands for, at the most (see [`OBJECT_BYTES`]): so five, which is a
/// coincidence with [`TEXT_COPIES`], not the same bound.
const WITH_TEXT_COPIES: usize = 5;

/// What the copies of the object of a `With` block may add to a statement,
/// besides [`WITH_TEXT_COPIES`] times its text: what the statements before it
/// left unspent, which the reader caps at 16,384, and the 128 that it earns
/// whatever its text is, a quarter more for the bytes they stand for. 24 KiB
/// covers the 20,640.
const WITH_ALLOWANCE: usize = 24 * 1024;

/// The bytes that the object of a `With` block may add to the line of a
/// statement of `text` and `tokens`. The reader copies the object into the
/// chain that a dot or a bang starts, so a statement has as many copies as it
/// has dots and bangs at the most, within what the reader lets it copy.
fn with_object_bytes(text: &str, tokens: &[Token]) -> usize {
    let dots = (tokens.iter())
        .filter(|token| matches!(token.kind, TokenKind::Dot | TokenKind::Bang))
        .count();
    let allowed = (text.len().saturating_mul(WITH_TEXT_COPIES)).saturating_add(WITH_ALLOWANCE);
    dots.saturating_mul(OBJECT_BYTES).min(allowed)
}

/// What a statement of `text` and `tokens` may take at the most once it is read
/// as a line, known before its facts are: a line over what is left of the room
/// is refused without being read.
///
/// The strings that folding builds out of the constants of the module are not
/// in it, nor are the ProgIDs that copy them: the reader holds what it builds
/// to four times the text of the statement and 1 KiB, which a statement full of
/// references to a long constant can spend. Such a line may weigh more than its
/// ceiling by about that much twice over, which the room takes in full once the
/// line is read (see [`Room::take`]): the ceiling only spares reading what could
/// not fit.
fn ceiling(text: &str, tokens: &[Token]) -> usize {
    size_of::<CodeLine>()
        .saturating_add(text.len().saturating_mul(TEXT_COPIES))
        .saturating_add(tokens.len().saturating_mul(FACTS_PER_TOKEN))
        .saturating_add(with_object_bytes(text, tokens))
}

/// What `line` takes in memory, near enough: the line itself, its texts, and
/// for each fact the struct that holds it and the allocation of its name.
fn line_bytes(line: &CodeLine) -> usize {
    fn text(text: &str) -> usize {
        match text.len() {
            0 => 0,
            len => len.saturating_add(ALLOCATION),
        }
    }
    fn owned(text_of: &str) -> usize {
        size_of::<String>().saturating_add(text(text_of))
    }
    fn total(parts: impl Iterator<Item = usize>) -> usize {
        parts.fold(0, usize::saturating_add)
    }

    let texts = text(&line.raw)
        .saturating_add(text(&line.code))
        .saturating_add(line.comment.as_deref().map_or(0, text));
    let strings = total(line.strings.iter().map(|string| owned(string)));
    let members = total(
        (line.members.iter())
            .map(|member| size_of::<MemberUse>().saturating_add(text(&member.name))),
    );
    let calls = total(
        (line.calls.iter()).map(|call| size_of::<CallSite>().saturating_add(text(&call.name))),
    );
    let chains = total(line.chains.iter().map(|chain| {
        let elements = total(chain.iter().map(|element| owned(element)));
        size_of::<Vec<String>>()
            .saturating_add(ALLOCATION)
            .saturating_add(elements)
    }));
    let progids = total(line.progids.iter().map(|progid| owned(progid)));
    size_of::<CodeLine>()
        .saturating_add(texts)
        .saturating_add(strings)
        .saturating_add(members)
        .saturating_add(calls)
        .saturating_add(chains)
        .saturating_add(progids)
}

/// A module as the rule engine and the metrics read it.
pub(super) struct Adapted {
    /// What the rule engine reads.
    pub(super) input: ModuleInput,
    /// What the metrics of the project need of the module.
    pub(super) measured: Measured,
}

impl Adapted {
    /// A module that was not read, whatever the cause: no source, too many lines
    /// or procedures, or more code than the room of the file holds. It is listed
    /// all the same, so that the modules of the engine are those of the report,
    /// in the same order.
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
/// `analysis`, taking from `room` what its lines weigh. A module that does not
/// fit is not adapted, and takes nothing.
pub(super) fn adapt(
    name: &str,
    module_type: ModuleType,
    analysis: &Analysis,
    room: &mut Room,
) -> Result<Adapted, VbaError> {
    let mut left = room.clone();
    let adapted = read_module(name, module_type, analysis, &mut left)?;
    *room = left;
    Ok(adapted)
}

/// The module `name` as the engine reads it, its lines taken from `room`.
fn read_module(
    name: &str,
    module_type: ModuleType,
    analysis: &Analysis,
    room: &mut Room,
) -> Result<Adapted, VbaError> {
    let logical = &analysis.lines.logical;
    let parsed = &analysis.module;
    let outside = gaps(logical.len(), &parsed.procedures);

    // The constants come first: a statement of any procedure may use one.
    let mut consts = ConstTable::default();
    for line in outside.iter().flat_map(|gap| slice(logical, gap)) {
        consts.declare(line);
    }
    let mut converter = Converter {
        consts: &consts,
        room,
    };

    let mut input = module_input(name, module_type);
    // Whether a statement of the module is too deep to be read. The analysis
    // knows what the parser and the measure found, not that.
    let mut suspect = false;
    // A mark of obfuscation in a statement outside the procedures is dropped
    // with its run: the engine cites the statements of procedures only.
    for gap in &outside {
        let run = converter.code_lines(slice(logical, gap))?;
        input.declarations.extend(run.lines);
        suspect |= run.too_deep;
    }
    let mut procedures = Vec::with_capacity(parsed.procedures.len());
    let mut metrics = Vec::with_capacity(parsed.procedures.len());
    let found = parsed.procedures.iter().zip(&analysis.measures);
    for (index, (procedure, measure)) in found.enumerate() {
        let run = converter.code_lines(slice(logical, &procedure.lines))?;
        suspect |= run.too_deep;
        procedures.push(ProcedureInput {
            name: procedure.name.clone(),
            kind: procedure.kind,
            start_line: procedure.start_line,
            end_line: procedure.end_line,
            lines: run.lines,
            predicates: ParserPredicates {
                udf_candidate: vba::is_udf_candidate(module_type, procedure, &analysis.lines),
                chr_obfuscation: run.chr_obfuscation,
                nested_loop_compare: measure.nested_loop_compare.clone(),
                parse_recovered: analysis.procedure_recovered(index) || run.too_deep,
                ..ParserPredicates::default()
            },
        });
        metrics.push(measure.metrics.clone());
    }
    input.procedures = procedures;
    input.parse_recovered = analysis.is_recovered() || suspect;

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

    Ok(Adapted {
        input,
        measured: Measured {
            read: true,
            procedures: metrics,
            declared: declared.collect(),
        },
    })
}

/// The lines of `range`. No line when the range leaves the lines, which the
/// parser does not give.
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

/// Reads the logical lines of a module into the lines of the engine, taking
/// what each weighs from the room of the file.
struct Converter<'c, 'r> {
    /// The constants of the module, which every run of lines reads with.
    consts: &'c ConstTable,
    room: &'r mut Room,
}

/// A statement as the engine reads it, with what reading it showed besides the
/// facts that its line holds.
struct Statement {
    line: CodeLine,
    /// The statement nests expressions deeper than the reader follows.
    too_deep: bool,
    /// An expression of the statement shows a mark of obfuscation (SPEC section
    /// 7, `chr_obfuscation`).
    chr_obfuscation: bool,
}

/// The lines of the engine for a run of statements, with what reading them
/// showed.
struct Run {
    lines: Vec<CodeLine>,
    /// A statement of the run nests expressions deeper than the reader follows.
    too_deep: bool,
    /// The position among `lines` of each statement that shows a mark of
    /// obfuscation: what the predicate `chr_obfuscation` cites. A position takes
    /// a `usize`, which the room does not count: the line of its statement was
    /// counted, and weighs at least `size_of::<CodeLine>()`, many times that.
    chr_obfuscation: Vec<usize>,
}

impl Run {
    /// An empty run, with room for `capacity` lines.
    fn new(capacity: usize) -> Self {
        Self {
            lines: Vec::with_capacity(capacity),
            too_deep: false,
            chr_obfuscation: Vec::new(),
        }
    }

    /// Adds the next statement of the run. A mark is cited by the position that
    /// the statement takes among the lines.
    fn push(&mut self, statement: Statement) {
        self.too_deep |= statement.too_deep;
        if statement.chr_obfuscation {
            self.chr_obfuscation.push(self.lines.len());
        }
        self.lines.push(statement.line);
    }
}

impl Converter<'_, '_> {
    /// The lines of the engine for `lines`, and what reading their statements
    /// showed. Fails when a line does not fit in the room that is left.
    ///
    /// `lines` is a run: the lines between two procedures, or those of one
    /// procedure. The reader that follows its `With` blocks is its own, so
    /// that a block a run leaves open, which a procedure without an end does,
    /// is closed with the run and gives its object to no other.
    fn code_lines(&mut self, lines: &[LogicalLine]) -> Result<Run, VbaError> {
        let mut reader = StatementReader::new(self.consts);
        // The room limits what is kept, so room is not made for all the lines
        // at once: a module of a quarter of a million lines would take 50 MB
        // before the first of them was refused.
        let mut run = Run::new(lines.len().min(1 << 12));
        for line in lines {
            let statement = self.code_line(&mut reader, line)?;
            run.push(statement);
        }
        Ok(run)
    }

    /// The statement of the engine for `line`, which `reader` reads after the
    /// lines of its run.
    fn code_line(
        &mut self,
        reader: &mut StatementReader<'_>,
        line: &LogicalLine,
    ) -> Result<Statement, VbaError> {
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
            self.room.take(line_bytes(&code))?;
            return Ok(Statement {
                line: code,
                too_deep: false,
                chr_obfuscation: false,
            });
        }
        // A statement that cannot fit is refused on its tokens, before its
        // facts are read. The tokens are gone when the facts are: they weigh 24
        // bytes each, and a hostile statement has a million.
        let tokens = tokenize(&line.text);
        self.room.fits(ceiling(&line.text, &tokens))?;
        let code = without_strings(&line.text, &tokens);
        drop(tokens);
        let facts = reader.read(line);
        let converted = CodeLine {
            line: line.line,
            raw,
            code,
            comment,
            strings: facts.strings,
            members: facts.members.into_iter().map(member).collect(),
            calls: facts.calls.into_iter().map(call).collect(),
            chains: facts.chains,
            progids: facts.progids,
        };
        self.room.take(line_bytes(&converted))?;
        Ok(Statement {
            line: converted,
            too_deep: facts.too_deep,
            chr_obfuscation: facts.chr_obfuscation,
        })
    }
}

/// A member that the statement reader found, as the engine reads it.
fn member(found: vba::MemberUse) -> MemberUse {
    MemberUse {
        name: found.name,
        named_argument: found.named_argument,
    }
}

/// A call that the statement reader found, as the engine reads it.
fn call(found: vba::CallSite) -> CallSite {
    CallSite {
        name: found.name,
        after_dot: found.after_dot,
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
    use crate::vba::MAX_NESTING;

    fn analysis(text: &str) -> Analysis {
        Analysis::of(text).expect("a short module")
    }

    fn adapted(text: &str) -> Adapted {
        adapt(
            "Module1",
            ModuleType::Standard,
            &analysis(text),
            &mut Room::of_a_file(),
        )
        .expect("a module that fits")
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
    fn the_predicate_that_the_front_end_does_not_read_yet_is_left_unset() {
        // A private sub that nothing calls: not an entry point, so the call graph
        // will find it possibly unused. Two character codes are no mark of
        // obfuscation.
        let adapted = adapted("Private Sub Run()\n    x = Chr(65) & Chr(66)\nEnd Sub\n");

        let procedure = &adapted.input.procedures[0];
        assert_eq!(procedure.predicates, ParserPredicates::default());
    }

    #[test]
    fn the_statements_that_show_a_mark_of_obfuscation_are_cited_by_their_position() {
        // The positions count every line of the procedure, its declaration and
        // its `End` line too, and a `:` makes a statement of its own.
        let text = [
            "Sub Run()",
            "    x = 1",
            "    y = Chr(72) & Chr(105) & Chr(33)",
            "    z = StrReverse(\"abc\")",
            "    a = 1: b = Chr(1) & Chr(2) & Chr(3)",
            "    c = Chr(65) & Chr(66)",
            "End Sub",
            "",
        ]
        .join("\n");

        let adapted = adapted(&text);

        let procedure = &adapted.input.procedures[0];
        let cited = &procedure.predicates.chr_obfuscation;
        assert_eq!(cited, &[2, 3, 5]);
        let statements: Vec<_> = (cited.iter())
            .map(|&at| procedure.lines[at].raw.as_str())
            .collect();
        assert_eq!(
            statements,
            [
                "y = Chr(72) & Chr(105) & Chr(33)",
                "z = StrReverse(\"abc\")",
                "b = Chr(1) & Chr(2) & Chr(3)"
            ]
        );
    }

    #[test]
    fn a_mark_of_obfuscation_is_cited_for_its_own_procedure_only() {
        let text = [
            "Sub First()",
            "    x = 1",
            "End Sub",
            "Sub Second()",
            "    y = Chr(1) + Chr(2) + Chr(3)",
            "End Sub",
            "",
        ]
        .join("\n");

        let adapted = adapted(&text);

        let cited: Vec<_> = (adapted.input.procedures.iter())
            .map(|procedure| procedure.predicates.chr_obfuscation.clone())
            .collect();
        // The position is among the lines of the procedure, not of the module.
        assert_eq!(cited, [Vec::new(), vec![1]]);
    }

    #[test]
    fn a_mark_of_obfuscation_outside_any_procedure_is_given_to_no_procedure() {
        let adapted =
            adapted("Const Hidden = Chr(1) & Chr(2) & Chr(3)\nSub Run()\n    x = 1\nEnd Sub\n");

        // The statement is a declaration, and the predicate lists the statements
        // of the procedure. That nothing cites it is checked on the report.
        let procedure = &adapted.input.procedures[0];
        assert!(procedure.predicates.chr_obfuscation.is_empty());
        assert_eq!(
            raws(&adapted.input.declarations),
            [(1, "Const Hidden = Chr(1) & Chr(2) & Chr(3)")]
        );
    }

    #[test]
    fn a_statement_over_several_physical_lines_is_one_position_with_the_first_number() {
        let text = [
            "Sub Run()",
            "    x = 1",
            "    y = Chr(72) & _",
            "        Chr(105) & _",
            "        Chr(33)",
            "End Sub",
            "",
        ]
        .join("\n");

        let adapted = adapted(&text);

        let procedure = &adapted.input.procedures[0];
        assert_eq!(procedure.predicates.chr_obfuscation, [2]);
        assert_eq!(procedure.lines[2].line, 3);
    }

    #[test]
    fn a_statement_too_deep_to_be_read_keeps_the_mark_found_before_the_depth() {
        // The three codes are read before the reader gives up on the depth, in
        // the same statement.
        let deep = format!(
            "x = Chr(1) & Chr(2) & Chr(3) & {}1{}",
            "(".repeat(MAX_NESTING + 1),
            ")".repeat(MAX_NESTING + 1)
        );

        let adapted = adapted(&format!("Sub Run()\n    {deep}\nEnd Sub\n"));

        let procedure = &adapted.input.procedures[0];
        assert!(procedure.predicates.parse_recovered);
        assert_eq!(procedure.predicates.chr_obfuscation, [1]);
    }

    #[test]
    fn the_progids_of_a_statement_are_those_the_reader_finds() {
        let adapted = adapted(
            "Sub Run()\n    Set o = CreateObject(\"WScript.Shell\")\n    Set p = CreateObject(Prog)\n\
             \x20   Set q = GetObject(\"C:\\book.xlsx\", \"Word.\" & \"Application\")\n    Shell \"calc\"\n\
             End Sub\nPrivate Const Prog = \"WScr\" & \"ipt.\" & \"Shell\"\n\
             Set r = CreateObject(\"Excel.Application\")\n",
        );

        let lines = &adapted.input.procedures[0].lines;
        assert_eq!(lines[1].progids, ["WScript.Shell"]);
        // The constant is declared after the procedure, and still folds.
        assert_eq!(lines[2].progids, ["WScript.Shell"]);
        // The path of `GetObject` is no ProgID, and its class is folded.
        assert_eq!(lines[3].progids, ["Word.Application"]);
        assert!(lines[4].progids.is_empty());
        // A statement outside the procedures is read the same way.
        let outside = (adapted.input.declarations.iter()).flat_map(|line| &line.progids);
        assert_eq!(outside.collect::<Vec<_>>(), ["Excel.Application"]);
        // A ProgID is listed with the strings of its statement as well.
        assert!(lines[1].strings.contains(&"WScript.Shell".to_owned()));
    }

    #[test]
    fn a_function_with_none_of_the_effects_of_the_list_is_a_udf_candidate() {
        let text = "Function Twice(x)\n    Twice = x * 2\nEnd Function\n\
                    Function Paint(r)\n    r.Font.Bold = True\nEnd Function\n\
                    Private Function Hidden(x)\n    Hidden = x\nEnd Function\n\
                    Sub Run()\n    x = 1\nEnd Sub\n";

        let standard = adapted(text).input;

        let candidates: Vec<_> = (standard.procedures.iter())
            .map(|procedure| (procedure.name.as_str(), procedure.predicates.udf_candidate))
            .collect();
        // Paint assigns to a font, Hidden is private and Run is not a function.
        assert_eq!(
            candidates,
            [
                ("Twice", true),
                ("Paint", false),
                ("Hidden", false),
                ("Run", false)
            ]
        );
        // Only the functions of a standard module can be called from a sheet.
        let sheet = adapt(
            "Sheet1",
            ModuleType::Document,
            &analysis(text),
            &mut Room::of_a_file(),
        )
        .expect("a module that fits")
        .input;
        let flagged = (sheet.procedures.iter()).any(|procedure| procedure.predicates.udf_candidate);
        assert!(!flagged);
    }

    #[test]
    fn the_chains_of_a_statement_are_those_the_reader_finds() {
        let adapted = adapted(
            "Sub Run()\n    ws.Cells(1, 1).Value = 2\n    x.Interior _\n        .Color = 1\n\
             End Sub\n",
        );

        let lines = &adapted.input.procedures[0].lines;
        assert_eq!(lines[1].chains, [["ws", "Cells", "Value"]]);
        // A dot that opens a continued line goes on with the chain before it.
        assert_eq!(lines[2].chains, [["x", "Interior", "Color"]]);
        assert!(lines[0].chains.is_empty());
    }

    #[test]
    fn a_chain_that_starts_with_a_dot_continues_the_object_of_the_with_block() {
        let adapted = adapted(
            "Sub Run()\n    With Range(\"A1\")\n        .Font.Bold = True\n    End With\n    \
             .Font = 1\nEnd Sub\n",
        );

        let lines = &adapted.input.procedures[0].lines;
        assert_eq!(lines[2].chains, [["Range", "Font", "Bold"]]);
        // The block is closed, and a dot that follows it has no object.
        assert!(lines[4].chains.is_empty());
    }

    #[test]
    fn a_with_block_that_a_procedure_leaves_open_is_closed_with_the_procedure() {
        // The first procedure has no End Sub, and its With has no End With.
        let adapted = adapted(
            "Sub Broken()\n    With ws\n        .A.B = 1\nSub Fine()\n    .C.D = 2\nEnd Sub\n",
        );

        let procedures = &adapted.input.procedures;
        assert_eq!(procedures[0].lines[2].chains, [["ws", "A", "B"]]);
        assert_eq!(procedures[1].lines[1].chains, [["C", "D"]]);
    }

    #[test]
    fn a_with_statement_outside_the_procedures_opens_no_block_for_them() {
        let adapted = adapted("With ws\nSub Run()\n    .A.B = 1\nEnd Sub\n");

        assert_eq!(raws(&adapted.input.declarations), [(1, "With ws")]);
        assert_eq!(adapted.input.procedures[0].lines[1].chains, [["A", "B"]]);
    }

    #[test]
    fn the_members_and_the_calls_of_a_statement_are_those_the_reader_finds() {
        let adapted =
            adapted("Sub Run()\n    Shell \"calc\"\n    ws.Cells(1, 1).Value = 2\nEnd Sub\n");

        fn members(line: &CodeLine) -> Vec<&str> {
            (line.members.iter())
                .map(|member| member.name.as_str())
                .collect()
        }
        fn calls(line: &CodeLine) -> Vec<(&str, bool)> {
            (line.calls.iter())
                .map(|call| (call.name.as_str(), call.after_dot))
                .collect()
        }
        let lines = &adapted.input.procedures[0].lines;
        assert_eq!(members(&lines[1]), ["Shell"]);
        assert_eq!(calls(&lines[1]), [("Shell", false)]);
        assert_eq!(members(&lines[2]), ["ws", "Cells", "Value"]);
        assert_eq!(calls(&lines[2]), [("Cells", true)]);
        assert!(lines[2].members.iter().all(|member| !member.named_argument));
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
            &mut Room::of_a_file(),
        )
        .expect("a module that fits");

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
    fn blocks_that_do_not_match_count_as_a_procedure_that_did_not_parse() {
        // The If has no End If, and the Sub is closed all the same.
        let open =
            adapted("Sub Run()\n    If x Then\n        y = 1\nEnd Sub\nSub Fine()\nEnd Sub\n")
                .input;
        assert_eq!(
            (
                open.procedures[0].predicates.parse_recovered,
                open.procedures[1].predicates.parse_recovered
            ),
            (true, false)
        );
        assert!(open.parse_recovered);

        let matched =
            adapted("Sub Run()\n    If x Then\n        y = 1\n    End If\nEnd Sub\n").input;
        assert!(!matched.procedures[0].predicates.parse_recovered);
        assert!(!matched.parse_recovered);
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

    #[test]
    fn a_line_weighs_its_texts_and_its_facts() {
        const TEXT: usize = 2 + ALLOCATION;
        const OWNED: usize = size_of::<String>() + 1 + ALLOCATION;

        assert_eq!(line_bytes(&CodeLine::default()), size_of::<CodeLine>());

        let member = CodeLine {
            raw: "ws".to_owned(),
            code: "ws".to_owned(),
            members: vec![MemberUse {
                name: "ws".to_owned(),
                named_argument: false,
            }],
            ..CodeLine::default()
        };
        assert_eq!(
            line_bytes(&member),
            size_of::<CodeLine>() + 2 * TEXT + size_of::<MemberUse>() + TEXT
        );

        let chain = CodeLine {
            strings: vec!["x".to_owned()],
            chains: vec![vec!["a".to_owned(), "b".to_owned()]],
            progids: vec!["p".to_owned()],
            ..CodeLine::default()
        };
        assert_eq!(
            line_bytes(&chain),
            size_of::<CodeLine>() + 2 * OWNED + size_of::<Vec<String>>() + ALLOCATION + 2 * OWNED
        );
    }

    #[test]
    fn a_module_takes_from_the_room_what_its_lines_weigh() {
        const FULL: usize = 1 << 20;
        let text = "Option Explicit\nSub Run()\n    ws.Cells(1, 1).Value = 2\nEnd Sub\n";
        let mut room = Room::of(FULL);

        let adapted = adapt("Module1", ModuleType::Standard, &analysis(text), &mut room)
            .expect("a module that fits");

        let input = &adapted.input;
        let procedures = input
            .procedures
            .iter()
            .flat_map(|procedure| &procedure.lines);
        let weight: usize = input
            .declarations
            .iter()
            .chain(procedures)
            .map(line_bytes)
            .sum();
        assert!(weight > 0);
        assert_eq!(room.left.saturating_add(weight), FULL);
    }

    #[test]
    fn a_module_that_does_not_fit_in_the_room_is_refused_and_takes_none_of_it() {
        const FULL: usize = 1 << 20;
        // Comments only: a statement is refused on its ceiling, which is above
        // what its line weighs, and a comment is refused on its weight.
        let text = "' the first\n' the second\n' the third\n";
        let mut full = Room::of(FULL);
        adapt("Module1", ModuleType::Standard, &analysis(text), &mut full)
            .expect("a module that fits");
        let weight = FULL.saturating_sub(full.left);

        // The room it needs is enough, and one byte less is not.
        let mut exact = Room::of(weight);
        assert!(adapt("Module1", ModuleType::Standard, &analysis(text), &mut exact).is_ok());
        assert_eq!(exact.left, 0);
        let mut short = Room::of(weight.saturating_sub(1));
        let error = adapt("Module1", ModuleType::Standard, &analysis(text), &mut short)
            .err()
            .expect("a module that does not fit");
        assert_eq!(
            (error.status, error.code),
            (Status::TooLarge, "vba.too_much_code")
        );
        assert_eq!(short.left, weight.saturating_sub(1));
    }

    #[test]
    fn the_facts_of_a_statement_weigh_more_than_its_text() {
        // Ten thousand members take 0.9 MB where the text takes 20 kB.
        let text = format!("Sub Run()\n    x = {}a\nEnd Sub\n", "a.".repeat(10_000));

        let mut room = Room::of(100_000);
        let refused = adapt("Module1", ModuleType::Standard, &analysis(&text), &mut room);

        assert_eq!(
            refused.err().map(|error| error.code),
            Some("vba.too_much_code")
        );
        assert_eq!(room.left, 100_000);
        let mut roomy = Room::of(4_000_000);
        adapt(
            "Module1",
            ModuleType::Standard,
            &analysis(&text),
            &mut roomy,
        )
        .expect("a module that fits");
        let weight = 4_000_000 - roomy.left;
        assert!((800_000..1_200_000).contains(&weight), "{weight}");
    }

    #[test]
    fn a_statement_is_refused_when_it_may_not_fit_though_it_would() {
        // A thousand numbers have no fact: the line takes 8 kB. They are 2,001
        // tokens, which could have been names, so the line may take 292 kB.
        let numbers = vec!["1"; 1_000].join(" + ");
        let text = format!("Sub Run()\n    x = {numbers}\nEnd Sub\n");

        let mut room = Room::of(1 << 20);
        adapt("Module1", ModuleType::Standard, &analysis(&text), &mut room)
            .expect("a module that fits");
        let weight = (1 << 20) - room.left;
        assert!(weight < 20_000, "{weight}");

        let mut tight = Room::of(100_000);
        let refused = adapt(
            "Module1",
            ModuleType::Standard,
            &analysis(&text),
            &mut tight,
        );
        assert_eq!(
            refused.err().map(|error| error.code),
            Some("vba.too_much_code")
        );
    }

    #[test]
    fn a_statement_never_weighs_more_than_its_ceiling() {
        let many = |unit: &str| unit.repeat(500);
        let statements = [
            "x = 1".to_owned(),
            many("a."),
            many("a!"),
            many("Dir "),
            many("a("),
            many("f(a),"),
            many("\"a\","),
            many("[a b]."),
            many("ws.Cells(1, 1)."),
            many("a:="),
            "x = Chr(65) & Chr(66) & \"x\" & Prefix".to_owned(),
            "Dim a As Long, b As New Collection, c As String".to_owned(),
            "Set o = CreateObject(\"WScript.Shell\").Exec(cmd).StdOut.ReadAll".to_owned(),
            "With ws\n        .Cells(1, 1).Font.Bold = True\n    End With".to_owned(),
        ];
        for statement in statements {
            let text = format!("Const Prefix = \"p\"\nSub Run()\n    {statement}\nEnd Sub\n");
            let adapted = adapted(&text);
            let lines = &adapted.input.procedures[0].lines;
            for line in lines.iter().filter(|line| line.comment.is_none()) {
                let ceiling = ceiling(&line.raw, &tokenize(&line.raw));
                let weight = line_bytes(line);
                let start: String = line.raw.chars().take(40).collect();
                assert!(weight <= ceiling, "{weight} > {ceiling} for {start:?}");
            }
        }
    }

    #[test]
    fn the_copies_of_the_object_of_a_block_stay_within_the_ceiling_of_the_statement() {
        // The object weighs 3,960 in the reader's measure, nearly what a block
        // may keep. The statements before the last spend none of what they
        // earn, so the last has all that may be saved in front of it, and spends
        // it on copies of the object: the most that a short statement can weigh.
        let object = vec!["a"; 120].join(".");
        let saved = "        x = 1\n".repeat(150);
        let text = format!(
            "Sub Run()\n    With {object}\n{saved}        .b = .c + .d + .e + .f + .g\n    \
             End With\nEnd Sub\n"
        );

        let adapted = adapted(&text);

        let lines = &adapted.input.procedures[0].lines;
        let last = (lines.iter())
            .find(|line| line.raw.starts_with(".b"))
            .expect("the last statement");
        let copies = (last.chains.iter())
            .filter(|chain| chain.len() > 100)
            .count();
        assert!(copies > 0, "{} chains, none a copy", last.chains.len());
        let tokens = tokenize(&last.raw);
        let weight = line_bytes(last);
        // The copies take more than the text and the tokens of the statement
        // allow, and the ceiling has room for them.
        let without =
            size_of::<CodeLine>() + TEXT_COPIES * last.raw.len() + tokens.len() * FACTS_PER_TOKEN;
        assert!(weight > without, "{weight} <= {without}");
        let ceiling = ceiling(&last.raw, &tokens);
        assert!(weight <= ceiling, "{weight} > {ceiling}");
    }

    #[test]
    fn the_object_of_a_block_is_copied_at_a_dot_or_a_bang_only() {
        let added = |text: &str| with_object_bytes(text, &tokenize(text));

        assert_eq!(added("x = Chr(65) & \"x\""), 0);
        assert_eq!(added(".A = 1"), OBJECT_BYTES);
        assert_eq!(added("!A = 1"), OBJECT_BYTES);
        // A long text with a single dot, a long string say, is copied into once.
        let long = format!("x = \"{}\" & .A", "a".repeat(1 << 20));
        assert_eq!(added(&long), OBJECT_BYTES);
        // And a text full of dots is copied into as much as the reader allows.
        let many = ".A ".repeat(10_000);
        assert_eq!(added(&many), WITH_TEXT_COPIES * many.len() + WITH_ALLOWANCE);
    }
}
