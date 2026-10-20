// SPDX-License-Identifier: Apache-2.0
//! The lexer cuts the text of a logical line into tokens (SPEC M2 item 2). One
//! test per kind of token, then the words and the characters that mean
//! something else in another place. Each token shows as `Kind[text]`.

use macroport_core::vba::{split_lines, tokenize};

/// The tokens of `text`, as `Kind[text]` separated by spaces.
fn show(text: &str) -> String {
    tokenize(text)
        .iter()
        .map(|token| format!("{:?}[{}]", token.kind, token.text(text)))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Checks that each text gives the tokens next to it.
fn check(cases: &[(&str, &str)]) {
    for (text, expected) in cases {
        assert_eq!(show(text), *expected, "{text}");
    }
}

#[test]
fn identifiers_are_names_with_an_optional_type_character() {
    check(&[
        ("Foo", "Identifier[Foo]"),
        ("foo_bar2", "Identifier[foo_bar2]"),
        ("Génère", "Identifier[Génère]"),
        ("Chr$", "Identifier[Chr$]"),
        ("n%", "Identifier[n%]"),
        ("price@", "Identifier[price@]"),
        ("Foo.Bar", "Identifier[Foo] Dot[.] Identifier[Bar]"),
    ]);
}

#[test]
fn bracketed_names_are_identifiers_whatever_they_hold() {
    check(&[
        ("[Sheet 1]", "Identifier[[Sheet 1]]"),
        ("[_pcCount]", "Identifier[[_pcCount]]"),
        ("[]", "Identifier[[]]"),
        ("[Type]", "Identifier[[Type]]"),
        ("[a'b:c]", "Identifier[[a'b:c]]"),
        (
            "[A1:B2].Value",
            "Identifier[[A1:B2]] Dot[.] Identifier[Value]",
        ),
    ]);
}

#[test]
fn the_name_of_an_identifier_leaves_out_its_type_character_and_brackets() {
    let text = "Chr$ [Sheet 1] Foo n% If 42";
    let names: Vec<&str> = tokenize(text)
        .iter()
        .map(|token| token.name(text))
        .collect();
    assert_eq!(names, ["Chr", "Sheet 1", "Foo", "n", "If", "42"]);
}

#[test]
fn keywords_are_found_in_any_letter_case() {
    check(&[
        ("If", "Keyword(If)[If]"),
        ("if", "Keyword(If)[if]"),
        ("IF", "Keyword(If)[IF]"),
        ("eLsEiF", "Keyword(ElseIf)[eLsEiF]"),
        ("End Sub", "Keyword(End)[End] Keyword(Sub)[Sub]"),
        (
            "ReDim Preserve a(1)",
            "Keyword(ReDim)[ReDim] Keyword(Preserve)[Preserve] Identifier[a] LeftParen[(] Number[1] RightParen[)]",
        ),
        (
            "a And Not b Or c",
            "Identifier[a] Keyword(And)[And] Keyword(Not)[Not] Identifier[b] Keyword(Or)[Or] Identifier[c]",
        ),
    ]);
}

#[test]
fn the_words_that_rules_name_are_identifiers_and_not_keywords() {
    let words = [
        "Len", "CLng", "Mid", "Open", "Close", "Print", "Shell", "Kill", "Dir", "Environ", "Error",
        "Nothing", "True", "False", "Empty", "Null", "Me", "Date", "Time", "Debug", "Lib", "Alias",
        "PtrSafe", "Explicit", "String", "Integer", "Object",
    ];
    for word in words {
        assert_eq!(show(word), format!("Identifier[{word}]"));
    }
}

#[test]
fn a_keyword_is_a_name_after_a_dot_before_a_named_argument_and_in_brackets() {
    check(&[
        ("x.End", "Identifier[x] Dot[.] Identifier[End]"),
        (".Type = 1", "Dot[.] Identifier[Type] Equals[=] Number[1]"),
        ("rs!Select", "Identifier[rs] Bang[!] Identifier[Select]"),
        (
            "Foo(Type:=8)",
            "Identifier[Foo] LeftParen[(] Identifier[Type] ColonEquals[:=] Number[8] RightParen[)]",
        ),
        (
            "Foo(Optional := 1)",
            "Identifier[Foo] LeftParen[(] Identifier[Optional] ColonEquals[:=] Number[1] RightParen[)]",
        ),
        ("[If]", "Identifier[[If]]"),
        ("Type :", "Keyword(Type)[Type] Colon[:]"),
    ]);
}

#[test]
fn a_string_runs_to_its_closing_quote_and_a_doubled_quote_is_a_quote() {
    check(&[
        (r#""abc""#, r#"String["abc"]"#),
        (r#""""#, r#"String[""]"#),
        (r#""say ""hi"" now""#, r#"String["say ""hi"" now"]"#),
        (r#""""""""#, r#"String[""""""]"#),
        (r#""a" & "b""#, r#"String["a"] Ampersand[&] String["b"]"#),
        ("\"é\" 1", "String[\"é\"] Number[1]"),
    ]);
}

#[test]
fn nothing_inside_a_string_starts_a_token() {
    check(&[(
        r##""it's [x] #1/2/2000# a:b !c Rem""##,
        r##"String["it's [x] #1/2/2000# a:b !c Rem"]"##,
    )]);
}

#[test]
fn a_string_without_its_closing_quote_runs_to_the_end_of_the_text() {
    check(&[
        (r#"x = "abc"#, r#"Identifier[x] Equals[=] String["abc]"#),
        (r#""abc"""#, r#"String["abc""]"#),
        (r#"""#, r#"String["]"#),
    ]);
}

#[test]
fn the_value_of_a_string_has_its_quotes_off_and_doubled_quotes_read_as_one() {
    let cases = [
        (r#""abc""#, Some("abc")),
        (r#""""#, Some("")),
        (r#""a""b""#, Some(r#"a"b"#)),
        (r#""""""""#, Some(r#""""#)),
        (r#""é ' [x]""#, Some("é ' [x]")),
        ("abc", None),
        ("[abc]", None),
    ];
    for (text, value) in cases {
        let tokens = tokenize(text);
        let token = tokens.first().expect("a token");
        assert_eq!(token.string_value(text).as_deref(), value, "{text}");
    }
}

#[test]
fn a_comment_runs_to_the_end_of_the_text() {
    check(&[
        ("' hello", "Comment[' hello]"),
        (
            r#"x = 1 ' tail "q"#,
            r#"Identifier[x] Equals[=] Number[1] Comment[' tail "q]"#,
        ),
        ("Rem hello", "Comment[Rem hello]"),
        ("REM", "Comment[REM]"),
        (
            "x = 1 : Rem tail",
            "Identifier[x] Equals[=] Number[1] Colon[:] Comment[Rem tail]",
        ),
        (
            "If a Then Rem tail",
            "Keyword(If)[If] Identifier[a] Keyword(Then)[Then] Comment[Rem tail]",
        ),
        ("Else Rem tail", "Keyword(Else)[Else] Comment[Rem tail]"),
    ]);
}

#[test]
fn rem_is_a_name_anywhere_but_where_a_statement_starts() {
    check(&[
        ("Remark = 1", "Identifier[Remark] Equals[=] Number[1]"),
        ("Rem_1 = 1", "Identifier[Rem_1] Equals[=] Number[1]"),
        ("x.Rem", "Identifier[x] Dot[.] Identifier[Rem]"),
        ("x Rem y", "Identifier[x] Identifier[Rem] Identifier[y]"),
        ("[Rem]", "Identifier[[Rem]]"),
    ]);
}

#[test]
fn an_apostrophe_inside_a_string_or_brackets_is_no_comment() {
    check(&[(r#""a'b" [a'b]"#, r#"String["a'b"] Identifier[[a'b]]"#)]);
}

#[test]
fn punctuation_and_operators_are_one_token_each() {
    check(&[
        (",", "Comma[,]"),
        (";", "Semicolon[;]"),
        (":", "Colon[:]"),
        (":=", "ColonEquals[:=]"),
        ("(", "LeftParen[(]"),
        (")", "RightParen[)]"),
        (".", "Dot[.]"),
        ("+", "Plus[+]"),
        ("-", "Minus[-]"),
        ("*", "Star[*]"),
        ("/", "Slash[/]"),
        ("\\", "Backslash[\\]"),
        ("^", "Caret[^]"),
        ("&", "Ampersand[&]"),
        ("=", "Equals[=]"),
        (
            "a + b - c * d / e \\ f ^ g",
            "Identifier[a] Plus[+] Identifier[b] Minus[-] Identifier[c] Star[*] Identifier[d] Slash[/] Identifier[e] Backslash[\\] Identifier[f] Caret[^] Identifier[g]",
        ),
        ("a & b", "Identifier[a] Ampersand[&] Identifier[b]"),
        ("x: =b", "Identifier[x] Colon[:] Equals[=] Identifier[b]"),
    ]);
}

#[test]
fn comparison_operators_are_spelled_both_ways() {
    check(&[
        ("<>", "NotEqual[<>]"),
        ("><", "NotEqual[><]"),
        ("<", "Less[<]"),
        (">", "Greater[>]"),
        ("<=", "LessEqual[<=]"),
        ("=<", "LessEqual[=<]"),
        (">=", "GreaterEqual[>=]"),
        ("=>", "GreaterEqual[=>]"),
        ("a<>b", "Identifier[a] NotEqual[<>] Identifier[b]"),
        ("a<-1", "Identifier[a] Less[<] Minus[-] Number[1]"),
        ("a=-1", "Identifier[a] Equals[=] Minus[-] Number[1]"),
    ]);
}

#[test]
fn a_character_that_starts_no_token_is_unknown_and_lexing_goes_on() {
    check(&[
        ("?", "Unknown[?]"),
        (
            "x = ? y",
            "Identifier[x] Equals[=] Unknown[?] Identifier[y]",
        ),
        ("€", "Unknown[€]"),
        ("“x”", "Unknown[“] Identifier[x] Unknown[”]"),
        ("@", "Unknown[@]"),
        ("$", "Unknown[$]"),
        ("{}", "Unknown[{] Unknown[}]"),
    ]);
}

#[test]
fn a_bracket_that_no_bracket_closes_is_unknown() {
    check(&[
        ("[abc", "Unknown[[] Identifier[abc]"),
        ("[a [b", "Unknown[[] Identifier[a] Unknown[[] Identifier[b]"),
        ("[a [b]", "Identifier[[a [b]]"),
        ("[a] [b", "Identifier[[a]] Unknown[[] Identifier[b]"),
    ]);
}

#[test]
fn only_whitespace_lies_between_tokens_and_offsets_are_in_bytes() {
    // A multibyte letter, a tab, a no-break space and U+0019 (MS-VBAL
    // whitespace) between five tokens.
    let text = "é\t=\u{a0}1\u{19}+ 2";
    let spans: Vec<(usize, usize)> = tokenize(text)
        .iter()
        .map(|token| (token.start, token.end))
        .collect();
    assert_eq!(spans, [(0, 2), (3, 4), (6, 7), (8, 9), (10, 11)]);
    assert!(tokenize("").is_empty());
    assert!(tokenize(" \t\u{a0} ").is_empty());
}

#[test]
fn a_token_maps_back_to_the_physical_line_it_came_from() {
    let text = "Sub Foo()\n    x = 1 + _\n        2\nEnd Sub\n";
    let lines = split_lines(text).expect("lines");
    let line = lines
        .logical
        .iter()
        .find(|line| line.text.starts_with('x'))
        .expect("the assignment");
    let physical: Vec<u32> = tokenize(&line.text)
        .iter()
        .map(|token| line.line_at(token.start))
        .collect();
    assert_eq!(line.text, "x = 1 + 2");
    assert_eq!(physical, [2, 2, 2, 2, 3]);
}
