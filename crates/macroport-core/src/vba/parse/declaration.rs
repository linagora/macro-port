// SPDX-License-Identifier: Apache-2.0
//! The declarations of a module: the statements outside any procedure that
//! declare or set something (SPEC M2 item 3).

use std::ops::Range;

use super::shape::{Directive, name_at};
use crate::vba::{Keyword, Token, TokenKind};

/// What an `Option` statement sets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptionKind {
    /// `Option Explicit`
    Explicit,
    /// `Option Base`
    Base,
    /// `Option Compare`
    Compare,
    /// `Option Private Module`
    PrivateModule,
    /// `Option VBASupport`
    VbaSupport,
    /// Any other word after `Option`, or none.
    Other,
}

/// What a declaration is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeclarationKind {
    /// An `Option` statement.
    Option(OptionKind),
    /// `Declare Function` or `Declare Sub`, with or without `PtrSafe` and a
    /// scope.
    Declare {
        /// The name the module gives the routine, not the one after `Alias`.
        /// Empty when the statement names none.
        name: String,
        /// `Function` rather than `Sub`.
        is_function: bool,
    },
    /// `Const`, alone or after a scope, with all the constants it declares.
    Const,
    /// `Dim`, `Public`, `Private` or `Global`, with all the variables it
    /// declares.
    Variable,
    /// A variable declared `WithEvents`.
    WithEvents {
        /// The name of the variable; of the first one when the statement
        /// declares several. Empty when the statement names none.
        name: String,
    },
    /// A `Type` with its members, up to its `End Type`.
    Type,
    /// An `Enum` with its members, up to its `End Enum`.
    Enum,
    /// `Implements`
    Implements,
    /// `Event`, alone or after a scope.
    Event,
    /// A conditional-compilation directive that stands outside any procedure.
    Directive(Directive),
}

/// A declaration outside any procedure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Declaration {
    /// What it is.
    pub kind: DeclarationKind,
    /// The physical line of its first word, from 1.
    pub line: u32,
    /// Its logical lines, as indices into [`SourceLines::logical`]: one, except
    /// for a `Type` or an `Enum`, which runs to its `End`, or to its last
    /// member when it has none.
    ///
    /// [`SourceLines::logical`]: crate::vba::SourceLines::logical
    pub lines: Range<usize>,
}

/// What the statement `tokens`, cut from `text`, declares, if it declares
/// something. The headers of procedures and the directives are told before.
pub(super) fn declaration_kind(tokens: &[Token], text: &str) -> Option<DeclarationKind> {
    let scopes = tokens
        .iter()
        .take_while(|token| is_scope(token.kind))
        .count();
    let word = tokens.get(scopes)?;
    let after = scopes.saturating_add(1);
    match word.kind {
        TokenKind::Keyword(Keyword::Option) if scopes == 0 => {
            Some(DeclarationKind::Option(option(tokens.get(after), text)))
        }
        TokenKind::Keyword(Keyword::Implements) if scopes == 0 => Some(DeclarationKind::Implements),
        TokenKind::Keyword(Keyword::Declare) => Some(declare_statement(
            tokens.get(after..).unwrap_or_default(),
            text,
        )),
        TokenKind::Keyword(Keyword::Const) => Some(DeclarationKind::Const),
        TokenKind::Keyword(Keyword::Type) => Some(DeclarationKind::Type),
        TokenKind::Keyword(Keyword::Enum) => Some(DeclarationKind::Enum),
        TokenKind::Keyword(Keyword::Event) => Some(DeclarationKind::Event),
        TokenKind::Keyword(Keyword::WithEvents) => Some(DeclarationKind::WithEvents {
            name: name_at(tokens, after, text),
        }),
        TokenKind::Identifier if scopes > 0 => Some(DeclarationKind::Variable),
        _ => None,
    }
}

/// The word that an `End` names to close the block that `kind` opens, if it
/// opens one: the members of a `Type` and of an `Enum` are no declarations.
pub(super) fn block_end(kind: &DeclarationKind) -> Option<Keyword> {
    match kind {
        DeclarationKind::Type => Some(Keyword::Type),
        DeclarationKind::Enum => Some(Keyword::Enum),
        _ => None,
    }
}

/// Whether the statement `tokens` reads `Word As ...`. A `Type` takes it for a
/// member whatever `Word` is, a keyword included: Excel exports `Type As Long`
/// as a member (corpus sample 13-06, line 105).
pub(super) fn reads_as_member(tokens: &[Token]) -> bool {
    tokens
        .get(1)
        .is_some_and(|token| token.kind == TokenKind::Keyword(Keyword::As))
}

/// The words that give a scope to a declaration, or open a variable one.
fn is_scope(kind: TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::Keyword(Keyword::Public | Keyword::Private | Keyword::Global | Keyword::Dim)
    )
}

/// What the `Option` statement sets, given the word that follows `Option`.
fn option(word: Option<&Token>, text: &str) -> OptionKind {
    const NAMED: [(&str, OptionKind); 4] = [
        ("explicit", OptionKind::Explicit),
        ("base", OptionKind::Base),
        ("compare", OptionKind::Compare),
        ("vbasupport", OptionKind::VbaSupport),
    ];
    let Some(word) = word else {
        return OptionKind::Other;
    };
    if word.kind == TokenKind::Keyword(Keyword::Private) {
        return OptionKind::PrivateModule;
    }
    NAMED
        .iter()
        .find(|(name, _)| word.is_name(text, name))
        .map_or(OptionKind::Other, |&(_, kind)| kind)
}

/// The `Declare` statement, given the tokens that follow the word `Declare`:
/// an optional `PtrSafe`, `Function` or `Sub`, and the name.
fn declare_statement(after: &[Token], text: &str) -> DeclarationKind {
    let ptr_safe = after
        .first()
        .is_some_and(|token| token.is_name(text, "ptrsafe"));
    let routine = usize::from(ptr_safe);
    let name = name_at(after, routine.saturating_add(1), text);
    let (name, is_function) = match after.get(routine).map(|token| token.kind) {
        Some(TokenKind::Keyword(Keyword::Function)) => (name, true),
        Some(TokenKind::Keyword(Keyword::Sub)) => (name, false),
        _ => (String::new(), false),
    };
    DeclarationKind::Declare { name, is_function }
}
