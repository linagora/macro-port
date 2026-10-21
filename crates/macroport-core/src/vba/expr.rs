// SPDX-License-Identifier: Apache-2.0
//! The expression reader: it walks the tokens of a statement as expressions,
//! with the precedence of VBA, and lists the strings, the members, the calls
//! and the chains it finds.
//!
//! A string literal is listed as it is. A literal that is part of an
//! expression the closed set of [`super::fold`] folds is listed once, as the
//! value of the whole expression. Anything the set does not cover is left
//! unresolved: its operands are listed apart, and nothing is made of the
//! result.
//!
//! Every identifier is a member, but the words for a value (`True`), the
//! label of a jump and what an `On` statement holds. It is a call when it is
//! invoked with arguments, or when it stands alone as an operand without being
//! assigned to: the reader does not know which names are variables, so it
//! takes those that the syntax does not rule out, like the calibration
//! prototype.
//!
//! The names joined by dots make a chain, the arguments of the calls in it
//! left out. A dot continues the chain when it touches the name before it or
//! opens a continued line (`a.b`, `a _` then `.b`); after a space it starts an
//! operand of its own, a member of the `With` object (`list.Add .Name`). A
//! chain that starts with a dot (or a bang) inside a `With` block continues the
//! object of the block, which the caller hands to the reader.
//!
//! The reader never fails and never recurses deeper than [`MAX_NESTING`]:
//! where a statement nests more, the rest is read flat, its strings and names
//! listed as they are, and the statement is flagged. Every token is read once
//! and a folded string is built by appending to the left operand, so the time
//! is linear in the tokens, but for the final sorts of the facts. What a
//! string function or a constant makes out of a string is paid for out of a
//! [`Budget`] that the text of the statement sets, and what a chain copies of
//! the object of a block out of an allowance that the text read so far sets,
//! so that the memory and the time are linear too.

use super::facts::{CallSite, Facts, MemberUse};
use super::fold::{Budget, ConstTable, Folding, Value, parse_number};
use super::lexer::{Keyword, Token, TokenKind};

/// How many expressions may be open at once, in a group inside a group and so
/// on, before the reader reads the rest of the statement flat. Each level
/// costs a few stack frames, so the bound keeps the stack small whatever the
/// text; code written by hand nests far less.
pub const MAX_NESTING: usize = 64;

/// Binding power at which the operand of `Not` is read: the comparisons and
/// what binds tighter.
const NOT_POWER: u8 = 11;

/// Binding power at which the operand of a unary `-` or `+` is read: what
/// binds tighter than `*`, that is `^`.
const SIGN_POWER: u8 = 23;

/// The names that a function of the closed set may be qualified with, each
/// followed by a `.`: `VBA.Chr(65)`, `VBA.Strings.Chr$(65)`.
const LIBRARIES: [&str; 2] = ["VBA", "Strings"];

/// The words that stand for a value, which the lexer reads as identifiers: they
/// are no member and no call.
const LITERALS: [&str; 5] = ["True", "False", "Nothing", "Null", "Empty"];

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

/// Whether a token of `kind` can start an expression. A type name, which `As`
/// and `New` introduce, counts as one.
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
            | TokenKind::Keyword(Keyword::Not | Keyword::New | Keyword::As)
    )
}

/// Whether the statement `tokens` declares something: a variable, a constant,
/// a procedure, a type. It holds names and constant expressions only, so the
/// names in it are no calls. `ReDim` declares nothing: its bounds are
/// ordinary expressions.
fn declares(tokens: &[Token]) -> bool {
    match tokens {
        [first, second, ..] if first.kind == TokenKind::Identifier => {
            // The element of a `Type` block: `Name As String`.
            second.kind == TokenKind::Keyword(Keyword::As)
        }
        [first, ..] => matches!(
            first.kind,
            TokenKind::Keyword(
                Keyword::Dim
                    | Keyword::Const
                    | Keyword::Static
                    | Keyword::Private
                    | Keyword::Public
                    | Keyword::Friend
                    | Keyword::Global
                    | Keyword::Declare
                    | Keyword::Sub
                    | Keyword::Function
                    | Keyword::Property
                    | Keyword::Type
                    | Keyword::Enum
                    | Keyword::Event
                    | Keyword::Implements
                    | Keyword::Option
            )
        ),
        [] => false,
    }
}

/// Where the last statement of the logical line `tokens` starts: at the first
/// token, or after the last `Then` or `Else` of a single-line `If` (`If a Then
/// With b`).
pub(super) fn statement_start(tokens: &[Token]) -> usize {
    let separator = tokens.iter().rposition(|token| {
        matches!(
            token.kind,
            TokenKind::Keyword(Keyword::Then | Keyword::Else)
        )
    });
    separator.map_or(0, |index| index.saturating_add(1))
}

/// Where the object of the `With` block that the statement `tokens` opens
/// would start, if it opens one.
fn block_object_start(tokens: &[Token]) -> Option<usize> {
    let head = statement_start(tokens);
    let opens = tokens
        .get(head)
        .is_some_and(|token| token.kind == TokenKind::Keyword(Keyword::With));
    opens.then(|| head.saturating_add(1))
}

/// The facts `found`, each with the index of the token it was found at, in
/// the order of the source.
fn in_order<T>(mut found: Vec<(usize, T)>) -> Vec<T> {
    found.sort_by_key(|&(at, _)| at);
    found.into_iter().map(|(_, fact)| fact).collect()
}

/// The member chain being read: `a.b(c).d`.
struct Chain {
    /// Index of its first token.
    start: usize,
    /// Whether a dot lies before the element about to be read: a leading `.`
    /// (inside a `With`) or the elements read already.
    qualified: bool,
    /// Whether the elements read so far are only library names (`VBA.`,
    /// `VBA.Strings.`), so that the next may be a function of the closed set.
    library: bool,
    /// The names read so far, the object of the `With` block in front of them
    /// if a dot started the chain.
    elements: Vec<String>,
    /// How many of the first elements are that object.
    inherited: usize,
}

/// What a name weighs in memory beyond its bytes: the header of its `String`
/// (24 bytes) and the share of the allocator. A copy of a name of one letter
/// takes thirty to forty bytes, not one.
const NAME_OVERHEAD: usize = 32;

/// The most that the object of a `With` block may weigh for the reader to keep
/// it: a few dozen names. Code written by hand has a handful. A block whose
/// object weighs more has none, so that the blocks that are open (at most
/// [`MAX_WITH_BLOCKS`](super::MAX_WITH_BLOCKS)) hold a few hundred kilobytes
/// together, whatever the text.
const MAX_OBJECT_WEIGHT: usize = 4096;

/// The object of a `With` block: the chain of names that follows the `With`,
/// which a dot inside the block continues.
#[derive(Debug, Default)]
pub(super) struct WithObject {
    /// The names, `["a", "b"]` for `With a.b`. Empty when the object is no
    /// chain of names or weighs too much.
    elements: Vec<String>,
    /// What it weighs: the bytes of the names, and [`NAME_OVERHEAD`] for each.
    weight: usize,
}

impl WithObject {
    /// The object whose chain is `elements`. None when it weighs more than
    /// [`MAX_OBJECT_WEIGHT`], without copying it.
    fn new(elements: &[String]) -> Option<Self> {
        let mut weight = 0usize;
        for name in elements {
            weight = weight
                .saturating_add(name.len())
                .saturating_add(NAME_OVERHEAD);
            if weight > MAX_OBJECT_WEIGHT {
                return None;
            }
        }
        Some(Self {
            elements: elements.to_vec(),
            weight,
        })
    }

    /// A copy of the names, paid for out of `allowance`. Empty when the
    /// allowance falls short.
    fn copy(&self, allowance: &mut Budget) -> Vec<String> {
        if allowance.take(self.weight) {
            self.elements.clone()
        } else {
            Vec::new()
        }
    }
}

/// The `With` block that a statement lies in.
pub(super) struct Enclosing<'a> {
    /// The object of the block, which a dot continues.
    pub(super) object: &'a WithObject,
    /// What the statements of the reader may still copy of it.
    pub(super) allowance: &'a mut Budget,
}

/// Reads the expressions of one statement.
pub(super) struct Reader<'a> {
    /// The text the tokens were cut from.
    text: &'a str,
    /// The tokens of the statement.
    tokens: &'a [Token],
    /// The constants of the module.
    consts: &'a ConstTable,
    /// Index of the next token to read.
    pos: usize,
    /// How many expressions are open.
    depth: usize,
    /// What is left to build out of strings.
    budget: Budget,
    /// Whether the statement declares something, so that no name in it is
    /// called.
    declaration: bool,
    /// Whether the rest of the statement was read flat, for it nests too deep.
    too_deep: bool,
    /// The strings found, by the index of their first token.
    strings: Vec<(usize, String)>,
    /// The members found, by the index of their token.
    members: Vec<(usize, MemberUse)>,
    /// The calls found, by the index of their token.
    calls: Vec<(usize, CallSite)>,
    /// The chains found, by the index of their first token.
    chains: Vec<(usize, Vec<String>)>,
    /// The innermost `With` block that the statement lies in, if the caller
    /// follows one. Its object has no names when it is unknown.
    enclosing: Option<Enclosing<'a>>,
    /// The indices of the dots and bangs that open a line continued from the
    /// one before, in order: they continue the chain before them, as a dot
    /// after a space does not.
    continued: &'a [usize],
    /// Where the object of the `With` block that the statement opens would
    /// start, if it opens one.
    object_start: Option<usize>,
    /// The object that the statement opens a block on, if it is a chain of
    /// names and nothing else.
    opened: Option<WithObject>,
}

impl<'a> Reader<'a> {
    /// A reader of the statement `tokens`, which were cut from `text`, outside
    /// any `With` block, with no continued line.
    pub(super) fn new(text: &'a str, tokens: &'a [Token], consts: &'a ConstTable) -> Self {
        Self {
            text,
            tokens,
            consts,
            pos: 0,
            depth: 0,
            budget: Budget::for_text(text),
            declaration: declares(tokens),
            too_deep: false,
            strings: Vec::new(),
            members: Vec::new(),
            calls: Vec::new(),
            chains: Vec::new(),
            enclosing: None,
            continued: &[],
            object_start: block_object_start(tokens),
            opened: None,
        }
    }

    /// The same reader, inside the `With` block `enclosing`, if the caller
    /// follows one. The object of a block has no names when it is unknown.
    pub(super) fn within(mut self, enclosing: Option<Enclosing<'a>>) -> Self {
        self.enclosing = enclosing;
        self
    }

    /// The same reader, for a statement whose tokens at the indices
    /// `continued` are dots or bangs that open a continued line.
    pub(super) fn continued(mut self, continued: &'a [usize]) -> Self {
        self.continued = continued;
        self
    }

    /// Reads the statement and gives the facts it holds, in source order, and
    /// the object it opens a `With` block on, if it opens one on a chain of
    /// names that a block may keep.
    pub(super) fn read(mut self) -> (Facts, Option<WithObject>) {
        // Whether the statement is a `Const` one, and whether the next name
        // is one it declares: that name is not a use of the constant.
        let mut const_statement = false;
        let mut declaring = false;
        while let Some(kind) = self.kind() {
            if self.named_argument() {
                continue;
            }
            match kind {
                TokenKind::Keyword(Keyword::Const) => {
                    (const_statement, declaring) = (true, true);
                    self.advance();
                }
                TokenKind::Comma if const_statement => {
                    declaring = true;
                    self.advance();
                }
                TokenKind::Identifier if declaring => {
                    declaring = false;
                    self.member(self.pos, false);
                    self.advance();
                }
                _ if starts_operand(kind) => {
                    let node = self.expression_here();
                    self.finish(node);
                }
                TokenKind::Keyword(Keyword::On) => self.skip_on(),
                TokenKind::Keyword(Keyword::GoTo | Keyword::GoSub | Keyword::Resume) => {
                    self.advance();
                    self.skip_label();
                }
                _ => self.advance(),
            }
        }
        let facts = Facts {
            strings: in_order(self.strings),
            members: in_order(self.members),
            calls: in_order(self.calls),
            chains: in_order(self.chains),
            too_deep: self.too_deep,
        };
        (facts, self.opened)
    }

    /// Reads a text that is nothing but one expression, and gives its value
    /// when it is a string. A comment may follow it. What the expression
    /// builds is paid for out of `budget`, which the declarators of a
    /// statement share.
    fn value(mut self, budget: &mut Budget) -> Option<String> {
        self.budget = *budget;
        let node = self.expression(0);
        *budget = self.budget;
        match node.value {
            Value::Text(text) if self.at_end() => Some(text),
            _ => None,
        }
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

    /// Moves past an `On Error ...` or `On n GoTo ...` statement, which holds
    /// no name worth a fact: up to the `Else` that a single-line `If` may put
    /// after it.
    fn skip_on(&mut self) {
        while let Some(kind) = self.kind() {
            if kind == TokenKind::Keyword(Keyword::Else) {
                break;
            }
            self.advance();
        }
    }

    /// Reads the rest of the statement without its structure: the strings it
    /// holds, one by one, as the source spells them, and its names, which are
    /// members, and calls by the rule of [`Reader::bare_call`] (labels and
    /// type names included, which the structure would tell apart). Nesting
    /// deeper than the reader goes hides nothing that way.
    fn read_flat(&mut self) {
        self.too_deep = true;
        let tokens = self.tokens;
        let kind_of = |index: usize| tokens.get(index).map(|token| token.kind);
        for (at, token) in tokens.iter().enumerate().skip(self.pos) {
            let next = kind_of(at.saturating_add(1));
            match token.kind {
                TokenKind::String => {
                    if let Some(text) = token.string_value(self.text) {
                        self.strings.push((at, text));
                    }
                }
                TokenKind::Identifier => {
                    self.member(at, next == Some(TokenKind::ColonEquals));
                    let before = at.checked_sub(1).and_then(kind_of);
                    let after_dot = matches!(before, Some(TokenKind::Dot | TokenKind::Bang));
                    if next == Some(TokenKind::LeftParen) {
                        self.call(at, after_dot);
                    } else if !after_dot && !matches!(next, Some(TokenKind::Dot | TokenKind::Bang))
                    {
                        self.bare_call(at, next);
                    }
                }
                _ => {}
            }
        }
        self.pos = tokens.len();
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

    /// Reads the expression that a token accepted by [`starts_operand`] opens.
    /// If it read nothing all the same, it moves past that token: the loops of
    /// the reader cannot stand still, whatever the two come to disagree on.
    fn expression_here(&mut self) -> Node {
        let before = self.pos;
        let node = self.expression(0);
        if self.pos == before {
            self.advance();
        }
        node
    }

    /// Reads an expression whose binary operators bind at least as tight as
    /// `min_power`.
    fn expression(&mut self, min_power: u8) -> Node {
        let start = self.pos;
        if self.depth >= MAX_NESTING {
            self.read_flat();
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
            TokenKind::Keyword(Keyword::New | Keyword::As) => {
                self.advance();
                self.type_name();
                Node::unknown(start)
            }
            TokenKind::Identifier | TokenKind::Dot | TokenKind::Bang => self.primary(),
            _ => Node::unknown(start),
        }
    }

    /// Reads the type that `As` or `New` introduces, as in `As New
    /// Scripting.Dictionary`. Its names are members, and none is called.
    fn type_name(&mut self) {
        if self.kind() == Some(TokenKind::Keyword(Keyword::New)) {
            self.advance();
        }
        while self.kind() == Some(TokenKind::Identifier) {
            let at = self.pos;
            self.advance();
            self.member(at, false);
            if self.kind() != Some(TokenKind::Dot) {
                break;
            }
            self.advance();
        }
    }

    /// Moves past the label that a `GoTo`, a `GoSub` or a `Resume` names, if
    /// there is one.
    fn skip_label(&mut self) {
        if matches!(self.kind(), Some(TokenKind::Identifier | TokenKind::Number)) {
            self.advance();
        }
    }

    /// Reads `Name:=` if that is what comes next: the name is a named
    /// argument, and the value follows it as an expression of its own. Tells
    /// whether it was.
    fn named_argument(&mut self) -> bool {
        let at = self.pos;
        let named = self
            .tokens
            .get(at)
            .zip(self.tokens.get(at.saturating_add(1)))
            .is_some_and(|(name, equals)| {
                name.kind == TokenKind::Identifier && equals.kind == TokenKind::ColonEquals
            });
        if named {
            self.member(at, true);
            self.pos = at.saturating_add(2);
        }
        named
    }

    /// The name that the token at `index` stands for, if it is one: not empty
    /// and not a word for a value.
    fn name(&self, index: usize) -> Option<&'a str> {
        let name = self.name_at(index);
        let literal = LITERALS
            .iter()
            .any(|literal| literal.eq_ignore_ascii_case(name));
        (!name.is_empty() && !literal).then_some(name)
    }

    /// Lists the name at `at` as a member.
    fn member(&mut self, at: usize, named_argument: bool) {
        if let Some(name) = self.name(at) {
            let name = name.to_owned();
            self.members.push((
                at,
                MemberUse {
                    name,
                    named_argument,
                },
            ));
        }
    }

    /// Lists the name at `at` as a call, unless the statement declares it.
    fn call(&mut self, at: usize, after_dot: bool) {
        if self.declaration {
            return;
        }
        if let Some(name) = self.name(at) {
            self.calls.push((
                at,
                CallSite {
                    name: name.to_owned(),
                    after_dot,
                },
            ));
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
    /// `rs!Name`, or `.Font` in a `With`. Only two shapes have a value: a call
    /// of the closed set, alone or qualified by library names, and the name
    /// of a constant. The arguments are expressions of their own, and the names
    /// make a chain.
    fn primary(&mut self) -> Node {
        let start = self.pos;
        let qualified = matches!(self.kind(), Some(TokenKind::Dot | TokenKind::Bang));
        let elements = if qualified {
            self.object_of_block(start)
        } else {
            Vec::new()
        };
        if qualified {
            self.advance();
        }
        let mut chain = Chain {
            start,
            qualified,
            library: !qualified,
            inherited: elements.len(),
            elements,
        };
        while self.kind() == Some(TokenKind::Identifier) {
            let at = self.pos;
            self.advance();
            chain.elements.push(self.name_at(at).to_owned());
            let arguments = self.argument_lists(&mut chain);
            if !self.continues_operand() {
                let node = self.last_element(&chain, at, arguments);
                self.end_chain(chain);
                return node;
            }
            self.inner_element(&mut chain, at, arguments);
            self.advance();
            chain.qualified = true;
        }
        self.end_chain(chain);
        Node::unknown(start)
    }

    /// Reads the argument list that follows a name, if there is one, and the
    /// ones after it, which index what the call gives (`Cells(1)(2)`). The call
    /// is then none of the closed set, and those lists hold expressions of
    /// their own.
    fn argument_lists(&mut self, chain: &mut Chain) -> Option<Vec<Option<Node>>> {
        if self.kind() != Some(TokenKind::LeftParen) {
            return None;
        }
        self.advance();
        let arguments = self.arguments();
        while self.kind() == Some(TokenKind::LeftParen) {
            self.advance();
            chain.library = false;
            let indexes = self.arguments();
            self.finish_all(indexes);
        }
        Some(arguments)
    }

    /// Whether the next token is a dot or a bang that continues the operand
    /// just read. A dot after a space starts an operand of its own: `list.Add
    /// .Name` passes the member `Name` of the `With` object to `Add`, as the
    /// corpus does. A continued line puts a space there all the same
    /// (`x.Interior _` then `.Color`): the caller says which dots open one.
    fn continues_operand(&self) -> bool {
        let at = self.pos;
        let Some(dot) = self
            .tokens
            .get(at)
            .filter(|token| matches!(token.kind, TokenKind::Dot | TokenKind::Bang))
        else {
            return false;
        };
        let touches = self
            .token_before(at)
            .is_some_and(|before| before.end == dot.start);
        touches || self.continued.binary_search(&at).is_ok()
    }

    /// The token before the one at `at`.
    fn token_before(&self, at: usize) -> Option<&'a Token> {
        at.checked_sub(1).and_then(|before| self.tokens.get(before))
    }

    /// The names that a chain starting with a dot (or a bang) at the token
    /// `at` continues: the object of the `With` block that the statement lies
    /// in. Empty when a `)` lies before the dot, which continues what the
    /// parenthesis closed, when the block has no known object, and when the
    /// allowance for copying it falls short.
    fn object_of_block(&mut self, at: usize) -> Vec<String> {
        let after_group = self
            .token_before(at)
            .is_some_and(|token| token.kind == TokenKind::RightParen);
        match &mut self.enclosing {
            Some(enclosing) if !after_group => enclosing.object.copy(enclosing.allowance),
            _ => Vec::new(),
        }
    }

    /// Ends `chain`. It is listed if it has an element of its own besides the
    /// first. It is the object of the block that the statement opens if it is
    /// the whole of the expression after the `With`.
    fn end_chain(&mut self, chain: Chain) {
        if self.object_start == Some(chain.start) && self.at_end() {
            self.opened = WithObject::new(&chain.elements);
        }
        if chain.elements.len() > chain.inherited.max(1) {
            self.chains.push((chain.start, chain.elements));
        }
    }

    /// Whether nothing but a comment is left of the statement.
    fn at_end(&self) -> bool {
        self.tokens
            .get(self.pos..)
            .is_some_and(|rest| rest.iter().all(|token| token.kind == TokenKind::Comment))
    }

    /// Whether the token at `index` names a library.
    fn is_library(&self, index: usize) -> bool {
        let name = self.name_at(index);
        LIBRARIES
            .iter()
            .any(|library| library.eq_ignore_ascii_case(name))
    }

    /// An element of `chain` that a `.` or a `!` follows, which is the token
    /// at `at` and has `arguments` if a `(` followed it. It is a member, and a
    /// call if it has arguments. A library name is followed by a `.`.
    fn inner_element(
        &mut self,
        chain: &mut Chain,
        at: usize,
        arguments: Option<Vec<Option<Node>>>,
    ) {
        self.member(at, false);
        chain.library = chain.library
            && self.kind() == Some(TokenKind::Dot)
            && arguments.is_none()
            && self.is_library(at);
        if let Some(arguments) = arguments {
            self.call(at, chain.qualified);
            self.finish_all(arguments);
        }
    }

    /// What the last element of `chain` is worth: it is the token at `at` and
    /// has `arguments` if a `(` followed it. It is a member. With arguments it
    /// is a call, of the closed set of folding perhaps; without, it may be a
    /// constant, or a call all the same.
    fn last_element(
        &mut self,
        chain: &Chain,
        at: usize,
        arguments: Option<Vec<Option<Node>>>,
    ) -> Node {
        self.member(at, false);
        let start = chain.start;
        let Some(arguments) = arguments else {
            // A qualified name belongs to another object, not to the module:
            // a property that is read or set, or a method without arguments.
            // No call is listed for it.
            if chain.qualified {
                return Node::unknown(start);
            }
            return match self.constant(start, at) {
                Some(node) => node,
                None => {
                    self.bare_call(at, self.kind());
                    Node::unknown(start)
                }
            };
        };
        self.call(at, chain.qualified);
        match Folding::named(self.name_at(at)).filter(|_| chain.library) {
            Some(function) => self.fold(start, function, arguments),
            None => {
                self.finish_all(arguments);
                Node::unknown(start)
            }
        }
    }

    /// Lists the name at `at`, which stands alone with no dot before it and
    /// no argument list after it, as a call, unless the token `next` that
    /// follows it makes it a variable: it is assigned to or compared with `=`
    /// (`Dir = 1`), or it is the variable of a `For Each`. The reader cannot
    /// tell a function from a variable otherwise.
    fn bare_call(&mut self, at: usize, next: Option<TokenKind>) {
        if !matches!(
            next,
            Some(TokenKind::Equals | TokenKind::ColonEquals | TokenKind::Keyword(Keyword::In))
        ) {
            self.call(at, false);
        }
    }

    /// The constant of the module that the name at `at` stands for, if it is
    /// one with a string. The node has the string if it fits in the budget,
    /// and none otherwise.
    fn constant(&mut self, start: usize, at: usize) -> Option<Node> {
        let value = self.consts.get(self.name_at(at))?;
        if !self.budget.take(value.len()) {
            return Some(Node::unknown(start));
        }
        Some(Node {
            value: Value::Text(value.to_owned()),
            start,
        })
    }

    /// What `function` makes of its `arguments`: one expression whose value
    /// it folds, or nothing.
    fn fold(&mut self, start: usize, function: Folding, arguments: Vec<Option<Node>>) -> Node {
        if let [Some(argument)] = arguments.as_slice()
            && let Some(text) = function.apply(&argument.value, &mut self.budget)
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
    /// first expression it holds. A token that starts none (`ByVal`) is
    /// skipped, and an expression after the first is listed apart. The name of
    /// a named argument is not part of the expression.
    fn slot(&mut self) -> Option<Node> {
        let mut first = None;
        while let Some(kind) = self.kind() {
            if matches!(kind, TokenKind::Comma | TokenKind::RightParen) {
                break;
            }
            if self.named_argument() {
                continue;
            }
            if starts_operand(kind) {
                let node = self.expression_here();
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

/// Records in `table` the constants that the statement `tokens`, which were
/// cut from `text`, declares: `Const A = "a", B = A & "b"`, with a scope
/// before the `Const` if it has one. A declarator can use the ones before it.
/// What they build is paid for out of one budget, the statement's, whatever
/// their number. A name whose value is not a known string is recorded all the
/// same, as one that has none.
pub(super) fn declare_consts(text: &str, tokens: &[Token], table: &mut ConstTable) {
    let scope = tokens.first().is_some_and(|token| {
        matches!(
            token.kind,
            TokenKind::Keyword(
                Keyword::Public | Keyword::Private | Keyword::Global | Keyword::Friend
            )
        )
    });
    let rest = tokens.get(usize::from(scope)..).unwrap_or_default();
    let Some((first, declarators)) = rest.split_first() else {
        return;
    };
    if first.kind != TokenKind::Keyword(Keyword::Const) {
        return;
    }
    let mut budget = Budget::for_text(text);
    for declarator in split_declarators(declarators) {
        let Some((name, after)) = declarator.split_first() else {
            continue;
        };
        // The type of the constant, if it is given, lies before the `=`.
        let Some(equals) = after
            .iter()
            .position(|token| token.kind == TokenKind::Equals)
        else {
            continue;
        };
        if name.kind != TokenKind::Identifier {
            continue;
        }
        let expression = after.get(equals.saturating_add(1)..).unwrap_or_default();
        let value = Reader::new(text, expression, table).value(&mut budget);
        table.insert(name.name(text), value);
    }
}

/// The declarators of a `Const` statement: what lies between the commas that
/// are not inside parentheses.
fn split_declarators(tokens: &[Token]) -> Vec<&[Token]> {
    let mut declarators = Vec::new();
    let mut depth = 0usize;
    let mut from = 0usize;
    for (index, token) in tokens.iter().enumerate() {
        match token.kind {
            TokenKind::LeftParen => depth = depth.saturating_add(1),
            TokenKind::RightParen => depth = depth.saturating_sub(1),
            TokenKind::Comma if depth == 0 => {
                declarators.push(tokens.get(from..index).unwrap_or_default());
                from = index.saturating_add(1);
            }
            _ => {}
        }
    }
    declarators.push(tokens.get(from..).unwrap_or_default());
    declarators
}
