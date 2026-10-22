// SPDX-License-Identifier: Apache-2.0
//! The input of the rule engine: a VBA project as the matchers of SPEC
//! section 7 read it.
//!
//! The engine owns this model. It is plain data, with no reference to the
//! lexer or the parser: `analyze` (#43) converts the parser's output into it,
//! so each field below says what the parser must put there. Tests build it by
//! hand.
//!
//! Conventions that hold for the whole model:
//!
//! - A `line` is a 1-based physical line of the module source as the file
//!   presents it, never renumbered (SPEC section 5): export headers,
//!   `Attribute` lines and the corpus header block count. Evidence quotes it.
//! - Names keep the spelling of the source. The engine compares them
//!   case-insensitively and ignores a type-declaration character at their end
//!   (`Chr$` is `Chr`), so the parser need not normalise them. A bracketed name
//!   (`[Sheet 1]`) is given without its brackets.
//! - Everything here comes from a hostile file. The engine only reads it, and
//!   must not panic whatever it holds.

use std::collections::BTreeSet;

use super::names::comparable_key;
use crate::report::ProcedureKind;
use crate::rules::ModuleType;

/// A VBA project: its references and its modules.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectInput {
    /// Display names of the references, such as `Microsoft Outlook 16.0 Object
    /// Library`, without paths (SPEC M1 item 7). Read by `reference`. Empty for
    /// a text module.
    pub references: Vec<String>,
    /// The modules, in the order of the report. The index of a module in this
    /// list is the `module` of the hits found in it.
    pub modules: Vec<ModuleInput>,
}

/// A module of the project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleInput {
    /// The module name. Read by `module_type`, whose hits over a module or a
    /// project quote it.
    pub name: String,
    /// Read by `module_type`, `auto_exec` and `event_handler`.
    pub module_type: ModuleType,
    /// Names of the `WithEvents` variables declared in the module, such as
    /// `App` for `Private WithEvents App As Application`. Read by
    /// `event_handler` in a class or a document module, as SPEC section 7 says.
    pub with_events: NameSet,
    /// Names of the controls of a form (`CommandButton1`). Empty for any other
    /// module, and for a form whose controls are not known: as specified
    /// today nothing reads them (M1 reads no form designer, and #38 skips the
    /// `Begin` blocks of a text `.frm`), so a handler named after a control is
    /// not recognised as an event handler until something fills this. Read by
    /// `event_handler`.
    pub controls: NameSet,
    /// The logical lines outside any procedure, in source order: `Option`,
    /// `Declare`, `Const`, variables, `Type` and `Enum` blocks, `#If`, and
    /// the comments between procedures. Module-scope signals read them too.
    pub declarations: Vec<CodeLine>,
    /// The procedures, in source order.
    pub procedures: Vec<ProcedureInput>,
    /// `parse_recovered` holds for the module as a whole: the parser stepped
    /// over something in it, in a procedure or outside any, as it does for a
    /// stray `End Sub` that no procedure holds (`ParsedModule::is_recovered`).
    /// Read by `parse_recovered` over a module or a project. Over one
    /// procedure, only its own `ParserPredicates` count.
    pub parse_recovered: bool,
}

/// A set of names that finds a name as the matchers compare names: whatever
/// its case, and without the type-declaration character that may end it. The
/// parser puts the names in with the spelling of the source (collect them).
///
/// The set keeps each name in the form in which names are compared, so that a
/// lookup does not read the other names: a procedure is looked up in it, and a
/// hostile module may declare thousands of names and hold thousands of
/// procedures.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NameSet(BTreeSet<String>);

impl NameSet {
    /// Whether the set holds no name.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Whether `name` is one of the names of the set. A name that is empty
    /// once its type character is gone is none of them.
    pub fn contains(&self, name: &str) -> bool {
        // Most modules declare none of these names: build no key for them.
        !self.0.is_empty() && self.contains_key(&comparable_key(name))
    }

    /// Whether `key`, a name already in the form in which names are compared,
    /// is one of the names of the set. The set holds no empty name, so an empty
    /// key is none of them.
    pub(super) fn contains_key(&self, key: &str) -> bool {
        self.0.contains(key)
    }
}

impl<S: AsRef<str>> FromIterator<S> for NameSet {
    fn from_iter<I: IntoIterator<Item = S>>(names: I) -> Self {
        let keys = names.into_iter().map(|name| comparable_key(name.as_ref()));
        Self(keys.filter(|key| !key.is_empty()).collect())
    }
}

/// A `Sub`, `Function` or `Property` procedure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcedureInput {
    /// The name in the declaration. Read by `procedure_name`, `auto_exec` and
    /// `event_handler`.
    pub name: String,
    /// How the procedure is declared. The matchers do not read it: it is
    /// carried for the callers that describe the procedure.
    pub kind: ProcedureKind,
    /// The physical line of the `Sub`, `Function` or `Property` keyword: the
    /// line that the hits about the whole procedure cite.
    pub start_line: u32,
    /// The physical line of the matching `End`, or of the last line of the
    /// procedure when the parser recovered without finding one. Callers check
    /// with it that a hit is inside the procedure (A4).
    pub end_line: u32,
    /// The logical lines from the declaration to the `End` line, both
    /// included, in source order.
    pub lines: Vec<CodeLine>,
    /// What the parser found about the procedure as a whole.
    pub predicates: ParserPredicates,
}

/// One logical code line, the unit of the line-based matchers (SPEC section
/// 7): a statement, after joining `_` continuations and splitting `:`
/// separators (SPEC M2 item 1). The comment is kept apart in `comment` and
/// left out of `code`. A comment-only line is a logical line with an empty
/// `code`.
///
/// Every item below belongs to the statement it was found in: with
/// `a = 1: b = Shell(c)`, the call `Shell` goes to the second line. The hits
/// of the item quote this line.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CodeLine {
    /// The first physical line of the statement. Statements split from one
    /// physical line share its number.
    pub line: u32,
    /// The statement as written, comment and string literals intact, its
    /// continuation markers removed and its physical lines joined by one
    /// space. The comment comes back after an apostrophe, whatever marked it:
    /// the parser does not keep whether `Rem` did. Read by `regex` with `text:
    /// raw`, and quoted by the hits found in the statement.
    pub raw: String,
    /// `raw` for `regex` with `text: code`: each string literal replaced by
    /// `""` and the comment removed. The empty literal stays, so that
    /// `Open "" For Output` still matches `Open\s+.+\s+For`.
    pub code: String,
    /// The text after the comment marker (`'` or `Rem`), as written, if the
    /// statement ends with a comment. Read by `comment`.
    pub comment: Option<String>,
    /// The contents of the string literals, with `""` read as `"`. A literal
    /// that is part of a foldable expression (SPEC M2 item 4) is replaced by
    /// the folded value, once for the whole expression: `"WScr" & "ipt.Sh" &
    /// "ell"` gives `WScript.Shell`. Without folding, each literal is listed.
    /// Read by `string` and by `long_hex_or_base64_literal`, which both see
    /// the folded value.
    pub strings: Vec<String>,
    /// Every identifier used as a member (`.X`, `!X`) or as a standalone
    /// object name, in source order, including a type name after `As` or `New`
    /// (the rules were calibrated that way: `office.mail-members` lists
    /// `MailItem`). Not keywords. A name that is invoked is listed here and in
    /// `calls`. Read by `member`.
    pub members: Vec<MemberUse>,
    /// The member chains of the statement after `With` resolution (nested
    /// `With` included), each as its elements in order, call arguments
    /// dropped: the statement `.Interior.Color = 1` in `With
    /// Worksheets(1).Range("A1")` gives `["Worksheets", "Range", "Interior",
    /// "Color"]`. A `!` access is an element like a `.` access. An element is
    /// a whole name, so a bracketed name that holds a dot stays one element.
    /// Chains found in call arguments are listed as chains of their own. A
    /// lone name is not a chain. Read by `chain`.
    pub chains: Vec<Vec<String>>,
    /// The names invoked in the statement: `Shell(x)`, `Kill path`, `Call
    /// Helper`. A name that is assigned to (`Dir = 1`), followed by `.`, or
    /// declared (`Dim Kill As Long`) is not a call (the calibration prototype
    /// left those out). Read by `call`.
    pub calls: Vec<CallSite>,
    /// The ProgID passed to `CreateObject` or `GetObject`, after constant
    /// folding, once per call. A call whose argument does not fold has none.
    /// Read by `progid`, which ignores a string that is not shaped like a
    /// ProgID, such as the file name that `GetObject` takes.
    pub progids: Vec<String>,
}

/// An identifier used as a member or as a standalone object name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberUse {
    /// The identifier.
    pub name: String,
    /// The identifier names an argument, as `Pattern` in `Pattern:=xlSolid`.
    /// The `member` matcher never matches it (SPEC section 7): the parser may
    /// flag such a name or leave it out.
    pub named_argument: bool,
}

/// A name invoked as a procedure, a function or a statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallSite {
    /// The name.
    pub name: String,
    /// The name follows `.` or `!`, as `Replace` in `Range.Replace What:="a"`.
    /// The `call` matcher never matches it (SPEC section 7): the parser may
    /// flag such a call or leave it out.
    pub after_dot: bool,
}

/// What the parser found about a procedure as a whole, for the structural
/// predicates that need the parse structure. The engine computes
/// `auto_exec`, `event_handler` and `long_hex_or_base64_literal` itself.
///
/// A predicate that holds once for the procedure is a `bool`, and its hit
/// cites the declaration. A predicate that holds at statements gives the
/// position of each in `ProcedureInput::lines`, which tells apart the
/// statements that share a physical line, as in `a = 1: b = Chr(1) & Chr(2) &
/// Chr(3)`. A position outside `lines` cites nothing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParserPredicates {
    /// `udf_candidate` holds.
    pub udf_candidate: bool,
    /// `chr_obfuscation`: the position of each statement that holds an
    /// obfuscated expression.
    pub chr_obfuscation: Vec<usize>,
    /// `nested_loop_compare`: the position of each comparison made in a loop
    /// nested in another one. The predicate gives one hit for the procedure, at
    /// the last of them, as in the prototype that calibrated the weights of the
    /// rules: a hit for each comparison would count up to
    /// `max_hits_per_procedure` times.
    pub nested_loop_compare: Vec<usize>,
    /// `possibly_unused` holds: not an entry point and never referenced.
    pub possibly_unused: bool,
    /// `parse_recovered` holds: the parser recovered from unbalanced blocks
    /// or an unparseable statement in the procedure.
    pub parse_recovered: bool,
}

#[cfg(test)]
mod tests {
    use super::NameSet;

    #[test]
    fn a_name_set_finds_a_name_whatever_its_case_and_type_character() {
        let names: NameSet = ["App", "Text_Box$", "Étiquette"].into_iter().collect();
        for found in ["app", "APP%", "TEXT_BOX", "Text_Box$", "ÉTIQUETTE"] {
            assert!(names.contains(found), "{found}");
        }
        for missing in ["Apps", "Text", "Box", "Text_Box_"] {
            assert!(!names.contains(missing), "{missing}");
        }
        assert!(!NameSet::default().contains("App"));
    }

    #[test]
    fn a_name_set_never_holds_nor_finds_an_empty_name() {
        let names: NameSet = ["", "$", "App"].into_iter().collect();
        assert_eq!(names, NameSet::from_iter(["App"]));
        assert!(!names.contains(""));
        assert!(!names.contains("$"));
    }
}
