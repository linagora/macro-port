// SPDX-License-Identifier: Apache-2.0
//! The facts of a statement (SPEC M2 items 3 and 4): what the rule engine
//! reads of a line of code, in a form that needs no knowledge of VBA.
//!
//! [`StatementReader`] reads one statement at a time and keeps what it needs
//! from one to the next. Nothing in a statement is evaluated, constant folding
//! apart.

use super::expr::Reader;
use super::fold::ConstTable;
use super::lexer::{Token, TokenKind, tokenize};
use super::{LogicalKind, LogicalLine};

/// What a statement holds. The facts weigh a few dozen times the text of the
/// statement (a name is copied as a member, as a call and in a chain), and the
/// name of a constant stands for a longer string. A caller that keeps the
/// facts of many statements bounds the total, as it bounds the lines
/// themselves.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Facts {
    /// The strings of the statement, in source order, with the doubled quotes
    /// of a literal read as one. An expression of the closed set of constant
    /// folding (`"WScr" & "ipt.Sh" & "ell"`, `Chr(110) & "ote"`, `Prefix &
    /// "x"` with a `Const Prefix`) is listed once, as its value, and not by
    /// the literals it is made of. The name of a constant stands for its
    /// string wherever it is used, so a procedure that uses a constant of the
    /// module sees the string, unless the string is longer than the statement
    /// may copy. A string literal that is not closed is not listed.
    pub strings: Vec<String>,
    /// Every identifier of the statement used as a member (`.X`, `!X`) or as
    /// a standalone object name, in source order, a type name after `As` or
    /// `New` included. Not a keyword, a literal (`True`, `Nothing`), the label
    /// of a jump or a name in an `On` statement (`On Error GoTo Handler`). A
    /// name that is invoked is listed here and in `calls`.
    pub members: Vec<MemberUse>,
    /// The names invoked in the statement, in source order: `Shell(x)`,
    /// `Kill path`, `Call Helper`, and a name used without being assigned to,
    /// such as `Dir` in `x = Dir`. After a dot, only a name with arguments
    /// (`ws.Cells(1, 1)`): `ws.Name` reads a property. Not a name followed by
    /// `=` (`Dir = 1`: assigned to, or compared), by a `.` that touches it
    /// (`Kill.Count`), declared (`Dim Kill As Long`) or the name of a constant
    /// of the module that holds a string, and none in a statement that
    /// declares something (`Dim`, `Const`, `Sub`, `Declare`...).
    pub calls: Vec<CallSite>,
    /// The member chains of the statement, in the order of their first token:
    /// the names of `a.b(c).d` are `["a", "b", "d"]`. The arguments of the
    /// calls in a chain are left out, and the chains they hold are listed
    /// apart. `!` separates like `.`, and a name between brackets is one
    /// element (`[Sheet 1].Select`). A dot after a space does not continue the
    /// chain before it, unless it opens a continued line: `list.Add .Name`
    /// gives `["list", "Add"]` and, `.Name` being alone, no other chain.
    ///
    /// A chain that starts with a dot, inside a `With` block, has the names
    /// after the dot and no more: `.Font.Bold` gives `["Font", "Bold"]`,
    /// whatever the object of the block. A name alone is no chain, and neither
    /// is a type after `As` or `New`.
    pub chains: Vec<Vec<String>>,
    /// The statement nests expressions deeper than [`MAX_NESTING`]. The reader
    /// read the rest of it flat from there: its strings are listed as the
    /// source spells them, with none folded, and its names as members and
    /// calls, a name that stands alone counting as a call whatever it is (a
    /// label or a type name too), but no chain. A caller can count the
    /// statement among those that did not parse.
    ///
    /// [`MAX_NESTING`]: super::MAX_NESTING
    pub too_deep: bool,
}

/// An identifier used as a member or as a standalone object name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberUse {
    /// The identifier, as the source spells it but for its brackets
    /// (`[Sheet 1]`) and its type-declaration character (`Left$`).
    pub name: String,
    /// The identifier names an argument, as `Pattern` in `Pattern:=xlSolid`.
    pub named_argument: bool,
}

/// A name invoked as a procedure, a function or a statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallSite {
    /// The name, spelled as for [`MemberUse::name`].
    pub name: String,
    /// The name follows a `.` or a `!`, as `Cells` in `ws.Cells(1, 1)`, not
    /// `Range` in `Range("A1").Select`.
    pub after_dot: bool,
}

/// Reads the statements of a module one after the other.
#[derive(Debug)]
pub struct StatementReader<'c> {
    /// The constants of the module, which a name in an expression can stand for.
    consts: &'c ConstTable,
}

impl<'c> StatementReader<'c> {
    /// A reader for the statements of a module whose constants are `consts`.
    pub fn new(consts: &'c ConstTable) -> Self {
        Self { consts }
    }

    /// The facts of a logical line, whose continued lines are known: a dot
    /// that opens one continues the chain before it. A line that is not code
    /// (a label, a comment) holds none.
    pub fn read(&mut self, line: &LogicalLine) -> Facts {
        if line.kind != LogicalKind::Code {
            return Facts::default();
        }
        let tokens = tokenize(&line.text);
        let continued = continued_dots(line, &tokens);
        Reader::new(&line.text, &tokens, self.consts)
            .continued(&continued)
            .read()
    }

    /// The facts of the statement `tokens`, which were cut from `text`, a text
    /// of one physical line: every dot after a space starts a chain of its
    /// own. [`StatementReader::read`] knows the continued lines.
    pub fn read_tokens(&mut self, text: &str, tokens: &[Token]) -> Facts {
        Reader::new(text, tokens, self.consts).read()
    }
}

/// The indices of the dots and bangs of `tokens`, which were cut from the text
/// of `line`, that open a physical line of it: a continuation lies between
/// each and the token before.
fn continued_dots(line: &LogicalLine, tokens: &[Token]) -> Vec<usize> {
    let opens_line = |index: usize, token: &Token| {
        matches!(token.kind, TokenKind::Dot | TokenKind::Bang)
            && index
                .checked_sub(1)
                .and_then(|before| tokens.get(before))
                .is_some_and(|before| line.line_at(before.start) != line.line_at(token.start))
    };
    let opening = tokens
        .iter()
        .enumerate()
        .filter(|&(index, token)| opens_line(index, token));
    opening.map(|(index, _)| index).collect()
}
