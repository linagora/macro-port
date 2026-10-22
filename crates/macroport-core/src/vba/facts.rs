// SPDX-License-Identifier: Apache-2.0
//! The facts of a statement (SPEC M2 items 3 and 4): what the rule engine
//! reads of a line of code, in a form that needs no knowledge of VBA.
//!
//! [`StatementReader`] reads one statement at a time and keeps what it needs
//! from one to the next. Nothing in a statement is evaluated, constant folding
//! apart.

use super::expr::{Enclosing, Reader, WithObject, statement_start};
use super::fold::{Budget, ConstTable};
use super::lexer::{Keyword, Token, TokenKind, tokenize};
use super::{LogicalKind, LogicalLine};

/// How many `With` blocks a reader follows at once. Code written by hand
/// nests a few. A block beyond the limit has no known object, but it closes in
/// its turn.
pub const MAX_WITH_BLOCKS: usize = 64;

/// What each statement a reader reads adds to what its statements may copy of
/// the objects of `With` blocks, besides four times its text: room for the
/// object of a usual block, three names of eight letters (a name weighs its
/// bytes and 32).
const COPY_ALLOWANCE: usize = 128;

/// What the statements of a reader may leave unspent of that for the next one,
/// in weight: four of the largest objects a block keeps. A long statement
/// earns a lot, but it does not bank it for a later one.
const COPY_CARRY: usize = 16_384;

/// What a statement holds. The facts weigh a few dozen times the text of the
/// statement (a name is copied as a member, as a call and in a chain), and the
/// name of a constant stands for a longer string. A chain that continues the
/// object of a `With` block adds to that, within what the reader allows itself
/// (see [`Facts::chains`]). A caller that keeps the facts of many statements
/// bounds the total, as it bounds the lines themselves.
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
    /// A chain that starts with a dot (or a `!`) inside a `With` block
    /// continues the object of the block: `.Font` in `With Range("A1")` is
    /// `["Range", "Font"]`. The names after the dot are listed without the
    /// object, as the end of the real chain (and as no chain at all if only
    /// one is left), when the object is no chain of names (`With New
    /// Collection`) or weighs more than a block may keep (about 4 KiB, a name
    /// weighing its bytes and 32), when more than [`MAX_WITH_BLOCKS`] blocks
    /// are open, when the dot follows a `)` (`(a).b`), and when the reader has
    /// copied as much as it allows itself: a statement earns 128 and four
    /// times its text, in weight, and what the ones before left unspent counts
    /// up to 16 KiB. A long object is not copied into each statement of a long
    /// block, and which statements get it depends on the ones read before. A
    /// name alone is no chain, and neither is a type after `As` or `New`.
    pub chains: Vec<Vec<String>>,
    /// The ProgIDs that the statement passes to `CreateObject` and
    /// `GetObject`, in the order of the calls, one per call: the class
    /// argument as it folds (`CreateObject("WScr" & "ipt.Sh" & "ell")` gives
    /// `WScript.Shell`). The class is the argument called `Class`
    /// (`CreateObject(Class:="X.Y")`), else the first argument of
    /// `CreateObject` and the second of `GetObject([path], [class])`, whose
    /// path (a file, a moniker) is never listed; an argument called otherwise
    /// is never taken for the class by its position. An argument that does not
    /// fold gives none, and so does a call with no parentheses
    /// (`CreateObject "X.Y"`). Only the functions of the library of VBA count,
    /// alone or qualified by its names (`VBA.Interaction.CreateObject`), not
    /// the method of another object (`app.CreateObject`). The string is not
    /// checked for the shape of a ProgID, and it is listed among `strings` as
    /// well.
    pub progids: Vec<String>,
    /// An expression of the statement shows the marks of obfuscation that
    /// SPEC section 7 gives for `chr_obfuscation`: three or more calls of
    /// `Chr`, `ChrW` or `ChrB` joined by `&` or `+` (a call counts whether
    /// its argument folds or not, and a group or `UCase`, `LCase`,
    /// `StrReverse` around the join does not hide it), or `StrReverse` of a
    /// string literal or of literals joined. A call is the function of the
    /// library of VBA (`Chr`, `VBA.Chr`, `Strings.ChrW$`), not the method of
    /// another object (`obj.Chr`). The name of a constant is no literal. None
    /// is looked for in the part of the statement that `too_deep` reads flat,
    /// but one found before it stays.
    pub chr_obfuscation: bool,
    /// The statement nests expressions deeper than [`MAX_NESTING`]. The reader
    /// read the rest of it flat from there: its strings are listed as the
    /// source spells them, with none folded, and its names as members and
    /// calls, a name that stands alone counting as a call whatever it is (a
    /// label or a type name too), but no chain, no ProgID and no mark of
    /// obfuscation. A caller can count the statement among those that did not
    /// parse.
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

/// The `With` blocks that are open.
#[derive(Debug, Default)]
struct WithBlocks {
    /// The object of each block, the outermost first, up to
    /// [`MAX_WITH_BLOCKS`].
    objects: Vec<WithObject>,
    /// How many blocks lie beyond them: they have no object.
    beyond: usize,
}

impl WithBlocks {
    /// The object of the innermost block. None outside any block, and when
    /// that block lies beyond the limit; an object with no names when the
    /// block's is no chain of names that a block may keep.
    fn innermost(&self) -> Option<&WithObject> {
        match self.beyond {
            0 => self.objects.last(),
            _ => None,
        }
    }

    /// Follows the statement `tokens`, which was read with the result `object`
    /// (the object it opens a block on, if it is a chain of names). A `With`
    /// opens a block, an `End With` closes one (none if it closes none), and
    /// the end of a procedure closes all. A single-line `If` counts for the
    /// statement after its `Then` or its `Else`.
    fn follow(&mut self, tokens: &[Token], object: Option<WithObject>) {
        let kind_of = |index: usize| tokens.get(index).map(|token| token.kind);
        let head = statement_start(tokens);
        match (kind_of(head), kind_of(head.saturating_add(1))) {
            (Some(TokenKind::Keyword(Keyword::With)), _) => {
                let object = object.unwrap_or_default();
                if self.objects.len() < MAX_WITH_BLOCKS {
                    self.objects.push(object);
                } else {
                    self.beyond = self.beyond.saturating_add(1);
                }
            }
            (Some(TokenKind::Keyword(Keyword::End)), Some(TokenKind::Keyword(Keyword::With))) => {
                if self.beyond > 0 {
                    self.beyond = self.beyond.saturating_sub(1);
                } else {
                    self.objects.pop();
                }
            }
            (
                Some(TokenKind::Keyword(Keyword::End)),
                Some(TokenKind::Keyword(Keyword::Sub | Keyword::Function | Keyword::Property)),
            ) => {
                self.objects.clear();
                self.beyond = 0;
            }
            _ => {}
        }
    }
}

/// Reads the statements of a module one after the other, and follows the
/// `With` blocks they open and close. The end of a procedure (`End Sub`, `End
/// Function`, `End Property`) closes the blocks it left open, so one reader
/// can serve the procedures of a module in order.
#[derive(Debug)]
pub struct StatementReader<'c> {
    /// The constants of the module, which a name in an expression can stand for.
    consts: &'c ConstTable,
    /// The `With` blocks that the statements read so far left open.
    blocks: WithBlocks,
    /// What the next statement may copy of the object of a block into its
    /// chains, in weight: [`COPY_ALLOWANCE`] and four times its text, and what
    /// the ones before left unspent, [`COPY_CARRY`] at most. So no long object
    /// is copied into each statement of a long block, and no statement copies
    /// much more than its own text and the carry.
    copy_allowance: Budget,
}

impl<'c> StatementReader<'c> {
    /// A reader for the statements of a module whose constants are `consts`.
    pub fn new(consts: &'c ConstTable) -> Self {
        Self {
            consts,
            blocks: WithBlocks::default(),
            copy_allowance: Budget::default(),
        }
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
        self.read_statement(&line.text, &tokens, &continued)
    }

    /// The facts of the statement `tokens`, which were cut from `text`, a text
    /// of one physical line: every dot after a space starts a chain of its
    /// own. [`StatementReader::read`] knows the continued lines.
    pub fn read_tokens(&mut self, text: &str, tokens: &[Token]) -> Facts {
        self.read_statement(text, tokens, &[])
    }

    /// The facts of a statement, in the `With` block that the ones before it
    /// left open if any. A `With` statement opens a block, an `End With`
    /// closes one, and the end of a procedure closes all.
    fn read_statement(&mut self, text: &str, tokens: &[Token], continued: &[usize]) -> Facts {
        self.copy_allowance.keep_at_most(COPY_CARRY);
        self.copy_allowance.earn(COPY_ALLOWANCE, text);
        let enclosing = self.blocks.innermost().map(|object| Enclosing {
            object,
            allowance: &mut self.copy_allowance,
        });
        let reader = Reader::new(text, tokens, self.consts)
            .continued(continued)
            .within(enclosing);
        let (facts, object) = reader.read();
        self.blocks.follow(tokens, object);
        facts
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
