// SPDX-License-Identifier: Apache-2.0
//! What a logical line is, as far as the structure of a module goes: the
//! header of a procedure, the end of one, a conditional-compilation directive,
//! or any other statement.

use crate::report::{ProcedureKind, Visibility};
use crate::vba::{Keyword, Token, TokenKind};

/// What the header of a procedure says.
pub(super) struct Header {
    pub kind: ProcedureKind,
    pub visibility: Visibility,
    pub is_static: bool,
    /// The name without brackets or type character; empty when the header
    /// names nothing.
    pub name: String,
    /// How many parameters follow the name.
    pub parameter_count: usize,
    /// Byte offset of the `Sub`, `Function` or `Property` keyword in the text
    /// of the line.
    pub keyword_offset: usize,
}

/// The kind of procedure that an `End` names.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum EndOf {
    Sub,
    Function,
    Property,
}

impl EndOf {
    /// Whether an `End` of this kind closes a procedure of kind `kind`.
    pub fn closes(self, kind: ProcedureKind) -> bool {
        match self {
            Self::Sub => kind == ProcedureKind::Sub,
            Self::Function => kind == ProcedureKind::Function,
            Self::Property => matches!(
                kind,
                ProcedureKind::PropertyGet
                    | ProcedureKind::PropertyLet
                    | ProcedureKind::PropertySet
            ),
        }
    }
}

/// A conditional-compilation directive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Directive {
    /// `#If`
    If,
    /// `#ElseIf`
    ElseIf,
    /// `#Else`
    Else,
    /// `#End If`
    EndIf,
    /// `#Const`
    Const,
}

/// What a logical line of code is.
pub(super) enum Shape {
    Header(Header),
    End(EndOf),
    Directive(Directive),
    Statement,
}

/// What the statement `tokens`, cut from `text`, is. A directive is told by its
/// leading `#`, before anything else.
pub(super) fn shape(tokens: &[Token], text: &str) -> Shape {
    if let Some(directive) = directive(tokens) {
        Shape::Directive(directive)
    } else if let Some(header) = header(tokens, text) {
        Shape::Header(header)
    } else if let Some(end) = procedure_end(tokens) {
        Shape::End(end)
    } else {
        Shape::Statement
    }
}

/// `#If`, `#ElseIf`, `#Else`, `#End If` or `#Const`.
fn directive(tokens: &[Token]) -> Option<Directive> {
    let [hash, word, rest @ ..] = tokens else {
        return None;
    };
    if hash.kind != TokenKind::Hash {
        return None;
    }
    match word.kind {
        TokenKind::Keyword(Keyword::If) => Some(Directive::If),
        TokenKind::Keyword(Keyword::ElseIf) => Some(Directive::ElseIf),
        TokenKind::Keyword(Keyword::Else) => Some(Directive::Else),
        TokenKind::Keyword(Keyword::End)
            if rest
                .first()
                .is_some_and(|token| token.kind == TokenKind::Keyword(Keyword::If)) =>
        {
            Some(Directive::EndIf)
        }
        TokenKind::Keyword(Keyword::Const) => Some(Directive::Const),
        _ => None,
    }
}

/// The header of a procedure: its modifiers, `Sub`, `Function` or `Property`
/// with `Get`, `Let` or `Set`, and its name.
fn header(tokens: &[Token], text: &str) -> Option<Header> {
    let mut visibility = Visibility::Default;
    let mut is_static = false;
    let mut at = 0usize;
    while let Some(token) = tokens.get(at) {
        match token.kind {
            TokenKind::Keyword(Keyword::Public) => visibility = Visibility::Public,
            TokenKind::Keyword(Keyword::Private) => visibility = Visibility::Private,
            TokenKind::Keyword(Keyword::Friend) => visibility = Visibility::Friend,
            TokenKind::Keyword(Keyword::Static) => is_static = true,
            _ => break,
        }
        at = at.saturating_add(1);
    }
    let keyword = tokens.get(at)?;
    // The kind, and how many tokens the keyword takes: `Property Get` is two.
    let (kind, keyword_tokens) = match keyword.kind {
        TokenKind::Keyword(Keyword::Sub) => (ProcedureKind::Sub, 1usize),
        TokenKind::Keyword(Keyword::Function) => (ProcedureKind::Function, 1),
        TokenKind::Keyword(Keyword::Property) => match tokens.get(at.saturating_add(1))?.kind {
            TokenKind::Keyword(Keyword::Get) => (ProcedureKind::PropertyGet, 2),
            TokenKind::Keyword(Keyword::Let) => (ProcedureKind::PropertyLet, 2),
            TokenKind::Keyword(Keyword::Set) => (ProcedureKind::PropertySet, 2),
            _ => return None,
        },
        _ => return None,
    };
    let name_index = at.saturating_add(keyword_tokens);
    let name = name_at(tokens, name_index, text);
    let parameter_count = if name.is_empty() {
        0
    } else {
        tokens
            .get(name_index.saturating_add(1)..)
            .map_or(0, parameter_count)
    };
    Some(Header {
        kind,
        visibility,
        is_static,
        name,
        parameter_count,
        keyword_offset: keyword.start,
    })
}

/// How many parameters the tokens after the name of a procedure declare: the
/// non-empty items between the parentheses, cut at the commas that stand
/// outside any nested parenthesis, as in `ParamArray rest() As Variant`.
fn parameter_count(after_name: &[Token]) -> usize {
    let mut tokens = after_name.iter();
    if tokens.next().map(|token| token.kind) != Some(TokenKind::LeftParen) {
        return 0;
    }
    let (mut depth, mut count, mut item) = (1usize, 0usize, false);
    for token in tokens {
        match token.kind {
            TokenKind::LeftParen => {
                depth = depth.saturating_add(1);
                item = true;
            }
            TokenKind::RightParen => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    break;
                }
                item = true;
            }
            TokenKind::Comma if depth == 1 => {
                count = count.saturating_add(usize::from(item));
                item = false;
            }
            _ => item = true,
        }
    }
    count.saturating_add(usize::from(item))
}

/// `End Sub`, `End Function` or `End Property`.
fn procedure_end(tokens: &[Token]) -> Option<EndOf> {
    match end_word(tokens)? {
        Keyword::Sub => Some(EndOf::Sub),
        Keyword::Function => Some(EndOf::Function),
        Keyword::Property => Some(EndOf::Property),
        _ => None,
    }
}

/// The keyword that the statement `tokens` names after a leading `End`, as
/// `Sub` in `End Sub` or `Type` in `End Type`.
pub(super) fn end_word(tokens: &[Token]) -> Option<Keyword> {
    match tokens {
        [end, word, ..] if end.kind == TokenKind::Keyword(Keyword::End) => match word.kind {
            TokenKind::Keyword(word) => Some(word),
            _ => None,
        },
        _ => None,
    }
}

/// The name that the token at `at` spells, if it is a name; empty when there is
/// none.
pub(super) fn name_at(tokens: &[Token], at: usize, text: &str) -> String {
    tokens
        .get(at)
        .filter(|token| token.kind == TokenKind::Identifier)
        .map(|token| token.name(text).to_owned())
        .unwrap_or_default()
}
