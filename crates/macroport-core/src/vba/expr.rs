// SPDX-License-Identifier: Apache-2.0
//! The expression reader: it walks the tokens of a statement as expressions,
//! with the precedence of VBA, and lists the strings it finds.
//!
//! A string literal is listed as it is. A literal that is part of an
//! expression the closed set of [`super::fold`] folds is listed once, as the
//! value of the whole expression. Anything the set does not cover is left
//! unresolved: its operands are listed apart, and nothing is made of the
//! result.
//!
//! The reader never fails and never recurses deeper than [`MAX_DEPTH`]: a
//! statement that nests more is given up where it stands. Every token is
//! read once, and a folded string is built by appending to the left operand,
//! so the time is linear in the tokens whatever their shape.

use super::fold::{Folding, Value, parse_number};
use super::lexer::{Keyword, Token, TokenKind};

/// How deeply expressions may nest before the reader gives up on the rest of
/// the statement. Each level costs a few stack frames, so the bound keeps the
/// stack small whatever the text; code written by hand nests far less.
const MAX_DEPTH: usize = 64;

/// Binding power at which the operand of `Not` is read: the comparisons and
/// what binds tighter.
const NOT_POWER: u8 = 11;

/// Binding power at which the operand of a unary `-` or `+` is read: what
/// binds tighter than `*`, that is `^`.
const SIGN_POWER: u8 = 23;

/// The names that a function of the closed set may be qualified with:
/// `VBA.Chr(65)`, `VBA.Strings.Chr$(65)`.
const LIBRARIES: [&str; 3] = ["VBA", "Strings", "Interaction"];

/// What an operand is worth.
struct Node {
    /// What is known of its value.
    value: Value,
    /// Index of its first token, which gives a string its place in the
    /// order of the statement.
    start: usize,
}

impl Node {
    /// An operand whose value is not known.
    fn unknown(start: usize) -> Self {
        Self {
            value: Value::Unknown,
            start,
        }
    }
}

/// What a binary operator does with its operands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Operator {
    /// `&` and `+`: two strings become one.
    Join,
    /// Any other: its operands are read, and nothing is known of the result.
    Other,
}

/// The binding powers of the binary operator `kind` and what it does. The
/// left power is the one against which the loop of [`Reader::expression`]
/// compares; the right one is what the right operand is read at. They differ
/// by one: the operators group from the left.
fn binary(kind: TokenKind) -> Option<(u8, u8, Operator)> {
    let (power, operator) = match kind {
        TokenKind::Keyword(Keyword::Imp) => (1, Operator::Other),
        TokenKind::Keyword(Keyword::Eqv) => (3, Operator::Other),
        TokenKind::Keyword(Keyword::Xor) => (5, Operator::Other),
        TokenKind::Keyword(Keyword::Or) => (7, Operator::Other),
        TokenKind::Keyword(Keyword::And) => (9, Operator::Other),
        TokenKind::Equals
        | TokenKind::NotEqual
        | TokenKind::Less
        | TokenKind::Greater
        | TokenKind::LessEqual
        | TokenKind::GreaterEqual
        | TokenKind::Keyword(Keyword::Like | Keyword::Is) => (NOT_POWER, Operator::Other),
        TokenKind::Ampersand => (13, Operator::Join),
        TokenKind::Plus => (15, Operator::Join),
        TokenKind::Minus => (15, Operator::Other),
        TokenKind::Keyword(Keyword::Mod) => (17, Operator::Other),
        TokenKind::Backslash => (19, Operator::Other),
        TokenKind::Star | TokenKind::Slash => (21, Operator::Other),
        TokenKind::Caret => (SIGN_POWER, Operator::Other),
        _ => return None,
    };
    Some((power, power.saturating_add(1), operator))
}

/// Whether a token of `kind` can start an expression.
fn starts_operand(kind: TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::Identifier
            | TokenKind::String
            | TokenKind::Number
            | TokenKind::Date
            | TokenKind::Dot
            | TokenKind::Bang
            | TokenKind::LeftParen
            | TokenKind::Plus
            | TokenKind::Minus
            | TokenKind::Keyword(Keyword::Not)
    )
}

/// Reads the expressions of one statement.
pub(super) struct Reader<'a> {
    /// The text the tokens were cut from.
    text: &'a str,
    /// The tokens of the statement.
    tokens: &'a [Token],
    /// Index of the next token to read.
    pos: usize,
    /// How many expressions are open.
    depth: usize,
    /// The strings found, by the index of their first token.
    strings: Vec<(usize, String)>,
}

impl<'a> Reader<'a> {
    /// A reader of the statement `tokens`, which were cut from `text`.
    pub(super) fn new(text: &'a str, tokens: &'a [Token]) -> Self {
        Self {
            text,
            tokens,
            pos: 0,
            depth: 0,
            strings: Vec::new(),
        }
    }

    /// Reads the statement and gives the strings it holds, in source order.
    pub(super) fn read(mut self) -> Vec<String> {
        while let Some(kind) = self.kind() {
            if starts_operand(kind) {
                let node = self.expression(0);
                self.finish(node);
            } else {
                self.advance();
            }
        }
        self.strings.sort_by_key(|&(start, _)| start);
        self.strings.into_iter().map(|(_, text)| text).collect()
    }

    /// The kind of the next token.
    fn kind(&self) -> Option<TokenKind> {
        self.tokens.get(self.pos).map(|token| token.kind)
    }

    /// The name that the token at `index` stands for.
    fn name_at(&self, index: usize) -> &'a str {
        self.tokens
            .get(index)
            .map_or("", |token| token.name(self.text))
    }

    /// Moves past the next token.
    fn advance(&mut self) {
        self.pos = self.pos.saturating_add(1);
    }

    /// Gives up on the rest of the statement.
    fn give_up(&mut self) {
        self.pos = self.tokens.len();
    }

    /// Lists the string a node holds, if it holds one: its expression ends
    /// here, so nothing will be folded into it any more.
    fn finish(&mut self, node: Node) {
        self.finish_value(node.start, node.value);
    }

    /// Lists `value`, whose expression starts at the token `start`.
    fn finish_value(&mut self, start: usize, value: Value) {
        if let Value::Text(text) = value {
            self.strings.push((start, text));
        }
    }

    /// Lists the strings of all the nodes.
    fn finish_all(&mut self, nodes: Vec<Option<Node>>) {
        for node in nodes.into_iter().flatten() {
            self.finish(node);
        }
    }

    /// Reads an expression whose binary operators bind at least as tight as
    /// `min_power`.
    fn expression(&mut self, min_power: u8) -> Node {
        let start = self.pos;
        if self.depth >= MAX_DEPTH {
            self.give_up();
            return Node::unknown(start);
        }
        self.depth = self.depth.saturating_add(1);
        let mut left = self.operand();
        while let Some((power, next, operator)) = self.kind().and_then(binary) {
            if power < min_power {
                break;
            }
            self.advance();
            let right = self.expression(next);
            left = self.combine(operator, left, right);
        }
        self.depth = self.depth.saturating_sub(1);
        left
    }

    /// What the operands `left` and `right` of `operator` make together. Two
    /// strings joined are one, built by appending to the left one: the chain
    /// `a & b & c & ...` is read in linear time. Otherwise the operands that
    /// are known are listed, and the result is not.
    fn combine(&mut self, operator: Operator, left: Node, right: Node) -> Node {
        let start = left.start;
        match (operator, left.value, right.value) {
            (Operator::Join, Value::Text(mut joined), Value::Text(tail)) => {
                joined.push_str(&tail);
                Node {
                    value: Value::Text(joined),
                    start,
                }
            }
            (_, left_value, right_value) => {
                self.finish_value(start, left_value);
                self.finish_value(right.start, right_value);
                Node::unknown(start)
            }
        }
    }

    /// Reads one operand: a literal, a group, a name with what follows it, or
    /// an operator and its operand. Nothing is read, and the value is not
    /// known, when the next token starts none.
    fn operand(&mut self) -> Node {
        let start = self.pos;
        let Some(token) = self.tokens.get(self.pos).copied() else {
            return Node::unknown(start);
        };
        match token.kind {
            TokenKind::String => {
                self.advance();
                Node {
                    value: token
                        .string_value(self.text)
                        .map_or(Value::Unknown, Value::Text),
                    start,
                }
            }
            TokenKind::Number => {
                self.advance();
                let value =
                    parse_number(token.text(self.text)).map_or(Value::Unknown, Value::Number);
                Node { value, start }
            }
            TokenKind::Date => {
                self.advance();
                Node::unknown(start)
            }
            TokenKind::LeftParen => {
                self.advance();
                self.group(start)
            }
            TokenKind::Plus | TokenKind::Minus => {
                self.advance();
                self.unary(start, SIGN_POWER)
            }
            TokenKind::Keyword(Keyword::Not) => {
                self.advance();
                self.unary(start, NOT_POWER)
            }
            TokenKind::Identifier | TokenKind::Dot | TokenKind::Bang => self.primary(),
            _ => Node::unknown(start),
        }
    }

    /// Reads the operand of a unary operator, which the operator makes unknown.
    fn unary(&mut self, start: usize, power: u8) -> Node {
        let operand = self.expression(power);
        self.finish(operand);
        Node::unknown(start)
    }

    /// Reads what follows a `(` that opens a group: the expression it holds,
    /// whose value it keeps. A group of several expressions, which VBA only
    /// allows in an argument list, holds none.
    fn group(&mut self, start: usize) -> Node {
        let mut slots = self.arguments();
        if slots.len() == 1
            && let Some(Some(inner)) = slots.pop()
        {
            return Node { start, ..inner };
        }
        self.finish_all(slots);
        Node::unknown(start)
    }

    /// Reads a name and what follows it: `Foo`, `Foo(1, 2)`, `a.b(c).d`,
    /// `rs!Name`, or `.Font` in a `With`. Only one shape has a value: a call
    /// of the closed set, alone or qualified by library names. The arguments
    /// are expressions of their own.
    fn primary(&mut self) -> Node {
        let start = self.pos;
        let qualified = matches!(self.kind(), Some(TokenKind::Dot | TokenKind::Bang));
        if qualified {
            self.advance();
        }
        // Whether the elements so far are library names.
        let mut library = !qualified;
        while self.kind() == Some(TokenKind::Identifier) {
            let at = self.pos;
            self.advance();
            let arguments = if self.kind() == Some(TokenKind::LeftParen) {
                self.advance();
                Some(self.arguments())
            } else {
                None
            };
            if !matches!(self.kind(), Some(TokenKind::Dot | TokenKind::Bang)) {
                return self.last_element(start, at, arguments, library);
            }
            library = library && arguments.is_none() && self.is_library(at);
            self.finish_all(arguments.unwrap_or_default());
            self.advance();
        }
        Node::unknown(start)
    }

    /// Whether the token at `index` names a library.
    fn is_library(&self, index: usize) -> bool {
        let name = self.name_at(index);
        LIBRARIES
            .iter()
            .any(|library| library.eq_ignore_ascii_case(name))
    }

    /// What the last element of a name is worth: it is the token at `at`, has
    /// `arguments` if a `(` followed it, and `library` tells whether the
    /// elements before it, if there are any, are only library names.
    fn last_element(
        &mut self,
        start: usize,
        at: usize,
        arguments: Option<Vec<Option<Node>>>,
        library: bool,
    ) -> Node {
        let Some(arguments) = arguments else {
            return Node::unknown(start);
        };
        match Folding::named(self.name_at(at)).filter(|_| library) {
            Some(function) => self.fold(start, function, arguments),
            None => {
                self.finish_all(arguments);
                Node::unknown(start)
            }
        }
    }

    /// What `function` makes of its `arguments`: one expression whose value
    /// it folds, or nothing.
    fn fold(&mut self, start: usize, function: Folding, arguments: Vec<Option<Node>>) -> Node {
        if let [Some(argument)] = arguments.as_slice()
            && let Some(text) = function.apply(&argument.value)
        {
            return Node {
                value: Value::Text(text),
                start,
            };
        }
        self.finish_all(arguments);
        Node::unknown(start)
    }

    /// Reads the arguments of a call, or the content of a group, up to the
    /// `)` that closes them, which it reads too, or to the end of the
    /// statement. There is one slot per comma-separated argument, `None` when
    /// it holds no expression (`GetObject(, "X")`); a `()` has no slot.
    fn arguments(&mut self) -> Vec<Option<Node>> {
        let mut slots = Vec::new();
        if self.kind() == Some(TokenKind::RightParen) {
            self.advance();
            return slots;
        }
        loop {
            slots.push(self.slot());
            match self.kind() {
                Some(TokenKind::Comma) => self.advance(),
                Some(TokenKind::RightParen) => {
                    self.advance();
                    break;
                }
                _ => break,
            }
        }
        slots
    }

    /// Reads one argument, up to the `,` or `)` that ends it, and gives the
    /// first expression it holds. A token that starts none (`As`, `ByVal`) is
    /// skipped, and an expression after the first is listed apart.
    fn slot(&mut self) -> Option<Node> {
        let mut first = None;
        while let Some(kind) = self.kind() {
            if matches!(kind, TokenKind::Comma | TokenKind::RightParen) {
                break;
            }
            if starts_operand(kind) {
                let node = self.expression(0);
                if first.is_none() {
                    first = Some(node);
                } else {
                    self.finish(node);
                }
            } else {
                self.advance();
            }
        }
        first
    }
}
