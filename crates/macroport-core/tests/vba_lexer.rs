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

#[test]
fn numbers_are_integers_floats_and_exponents() {
    check(&[
        ("42", "Number[42]"),
        ("007", "Number[007]"),
        ("3.14", "Number[3.14]"),
        ("5.", "Number[5.]"),
        (".5", "Number[.5]"),
        ("1E5", "Number[1E5]"),
        ("1.5e-3", "Number[1.5e-3]"),
        ("2D+10", "Number[2D+10]"),
        ("1d5", "Number[1d5]"),
    ]);
}

#[test]
fn an_exponent_needs_digits_and_a_number_may_touch_the_word_after_it() {
    check(&[
        ("1E", "Number[1] Identifier[E]"),
        ("1E+", "Number[1] Identifier[E] Plus[+]"),
        ("1Else", "Number[1] Keyword(Else)[Else]"),
        (
            "For i = 1To 3",
            "Keyword(For)[For] Identifier[i] Equals[=] Number[1] Keyword(To)[To] Number[3]",
        ),
    ]);
}

#[test]
fn a_number_takes_the_type_character_that_follows_it() {
    check(&[
        ("10%", "Number[10%]"),
        ("10&", "Number[10&]"),
        ("1.5!", "Number[1.5!]"),
        ("1.5#", "Number[1.5#]"),
        ("1.5@", "Number[1.5@]"),
        ("-1&", "Minus[-] Number[1&]"),
        (
            "x = 2^3",
            "Identifier[x] Equals[=] Number[2] Caret[^] Number[3]",
        ),
    ]);
}

#[test]
fn an_ampersand_is_a_type_character_unless_an_operand_follows_it() {
    check(&[
        ("i&", "Identifier[i&]"),
        (
            "Dim i&, j&",
            "Keyword(Dim)[Dim] Identifier[i&] Comma[,] Identifier[j&]",
        ),
        ("x& = 1&", "Identifier[x&] Equals[=] Number[1&]"),
        ("a&b", "Identifier[a] Ampersand[&] Identifier[b]"),
        (r#"a&"s""#, r#"Identifier[a] Ampersand[&] String["s"]"#),
        (
            "a&(b)",
            "Identifier[a] Ampersand[&] LeftParen[(] Identifier[b] RightParen[)]",
        ),
        ("1&2", "Number[1] Ampersand[&] Number[2]"),
        ("a & b", "Identifier[a] Ampersand[&] Identifier[b]"),
    ]);
}

#[test]
fn a_bang_reaches_a_member_unless_it_is_the_type_character_of_a_name() {
    check(&[
        ("rs!Name", "Identifier[rs] Bang[!] Identifier[Name]"),
        (
            "rs![Field Name]",
            "Identifier[rs] Bang[!] Identifier[[Field Name]]",
        ),
        (
            "rs.Fields!Name!Other",
            "Identifier[rs] Dot[.] Identifier[Fields] Bang[!] Identifier[Name] Bang[!] Identifier[Other]",
        ),
        ("!Name", "Bang[!] Identifier[Name]"),
        (
            "Foo(1)!Bar",
            "Identifier[Foo] LeftParen[(] Number[1] RightParen[)] Bang[!] Identifier[Bar]",
        ),
        ("x! = 1", "Identifier[x!] Equals[=] Number[1]"),
        ("x!", "Identifier[x!]"),
        (
            "f(x!)",
            "Identifier[f] LeftParen[(] Identifier[x!] RightParen[)]",
        ),
    ]);
}

#[test]
fn a_hash_around_digits_is_a_date() {
    for literal in [
        "#1/2/2000#",
        "#12/31/1999 11:59:59 PM#",
        "#2000-01-02#",
        "#Jan 1, 2000#",
        "#1:30 PM#",
        "#13:45:00#",
    ] {
        assert_eq!(show(literal), format!("Date[{literal}]"));
    }
    check(&[
        (
            "d = #1/2/2000# + 1",
            "Identifier[d] Equals[=] Date[#1/2/2000#] Plus[+] Number[1]",
        ),
        (
            "Foo(#1/2/2000#, #3/4/2000#)",
            "Identifier[Foo] LeftParen[(] Date[#1/2/2000#] Comma[,] Date[#3/4/2000#] RightParen[)]",
        ),
    ]);
}

#[test]
fn a_hash_before_a_file_number_or_a_directive_is_no_date() {
    check(&[
        (
            "Print #1, x",
            "Identifier[Print] Hash[#] Number[1] Comma[,] Identifier[x]",
        ),
        (
            "Print#1, x",
            "Identifier[Print] Hash[#] Number[1] Comma[,] Identifier[x]",
        ),
        (
            "Open f For Output As #1",
            "Identifier[Open] Identifier[f] Keyword(For)[For] Identifier[Output] Keyword(As)[As] Hash[#] Number[1]",
        ),
        (
            "Close #fileNumber",
            "Identifier[Close] Hash[#] Identifier[fileNumber]",
        ),
        (
            "Print#fn, x",
            "Identifier[Print] Hash[#] Identifier[fn] Comma[,] Identifier[x]",
        ),
        (
            "Print #1, #1/1/2000#",
            "Identifier[Print] Hash[#] Number[1] Comma[,] Date[#1/1/2000#]",
        ),
        (
            "#If DEBUG Then",
            "Hash[#] Keyword(If)[If] Identifier[DEBUG] Keyword(Then)[Then]",
        ),
        (
            "#Const A = 1",
            "Hash[#] Keyword(Const)[Const] Identifier[A] Equals[=] Number[1]",
        ),
        ("#Else", "Hash[#] Keyword(Else)[Else]"),
        ("#End If", "Hash[#] Keyword(End)[End] Keyword(If)[If]"),
    ]);
}

#[test]
fn a_hash_right_after_a_name_is_its_type_character() {
    check(&[
        ("x#", "Identifier[x#]"),
        (
            "x# = #1/2/2000#",
            "Identifier[x#] Equals[=] Date[#1/2/2000#]",
        ),
        (
            "Dim a#, b#",
            "Keyword(Dim)[Dim] Identifier[a#] Comma[,] Identifier[b#]",
        ),
        (
            "CDbl(x#)",
            "Identifier[CDbl] LeftParen[(] Identifier[x#] RightParen[)]",
        ),
    ]);
}

#[test]
fn a_hash_after_a_name_or_a_closing_bracket_opens_no_date() {
    check(&[
        (
            "x#1/2/2000#",
            "Identifier[x] Hash[#] Number[1] Slash[/] Number[2] Slash[/] Number[2000#]",
        ),
        (
            "f(1)#1/2/2000#",
            "Identifier[f] LeftParen[(] Number[1] RightParen[)] Hash[#] Number[1] Slash[/] Number[2] Slash[/] Number[2000#]",
        ),
        (
            "[a]#1/2/2000#",
            "Identifier[[a]] Hash[#] Number[1] Slash[/] Number[2] Slash[/] Number[2000#]",
        ),
        ("x #1/2/2000#", "Identifier[x] Date[#1/2/2000#]"),
    ]);
}

#[test]
fn a_hash_before_a_file_number_and_a_separator_opens_no_date() {
    check(&[
        (
            "Write #1, 1, 2#",
            "Identifier[Write] Hash[#] Number[1] Comma[,] Number[1] Comma[,] Number[2#]",
        ),
        (
            "Print #1, 5 - 3#",
            "Identifier[Print] Hash[#] Number[1] Comma[,] Number[5] Minus[-] Number[3#]",
        ),
        (
            "Print # 1; x",
            "Identifier[Print] Hash[#] Number[1] Semicolon[;] Identifier[x]",
        ),
        (
            "Print #fn ; x",
            "Identifier[Print] Hash[#] Identifier[fn] Semicolon[;] Identifier[x]",
        ),
        (
            "Write #1, #1/2/2000#, 5",
            "Identifier[Write] Hash[#] Number[1] Comma[,] Date[#1/2/2000#] Comma[,] Number[5]",
        ),
        // The shape of a file number wins over what could be a date.
        (
            "#Jan, 2000#",
            "Hash[#] Identifier[Jan] Comma[,] Number[2000#]",
        ),
    ]);
}

#[test]
fn a_dot_before_a_digit_starts_a_number_and_before_a_name_reaches_a_member() {
    check(&[
        ("x = .5", "Identifier[x] Equals[=] Number[.5]"),
        ("MsgBox .5", "Identifier[MsgBox] Number[.5]"),
        (
            "f(.5, .25)",
            "Identifier[f] LeftParen[(] Number[.5] Comma[,] Number[.25] RightParen[)]",
        ),
        (
            "Foo(1).Bar",
            "Identifier[Foo] LeftParen[(] Number[1] RightParen[)] Dot[.] Identifier[Bar]",
        ),
        (
            "If a Then .Name = 1",
            "Keyword(If)[If] Identifier[a] Keyword(Then)[Then] Dot[.] Identifier[Name] Equals[=] Number[1]",
        ),
        ("x.Y .5", "Identifier[x] Dot[.] Identifier[Y] Number[.5]"),
    ]);
}

#[test]
fn hexadecimal_and_octal_literals_are_numbers() {
    check(&[
        ("&HFF", "Number[&HFF]"),
        ("&hff", "Number[&hff]"),
        ("&H80070005", "Number[&H80070005]"),
        ("&H8000&", "Number[&H8000&]"),
        ("&HFF%", "Number[&HFF%]"),
        ("&O17", "Number[&O17]"),
        ("&o17&", "Number[&o17&]"),
        ("&17", "Number[&17]"),
        ("x = &H10", "Identifier[x] Equals[=] Number[&H10]"),
        ("-&H1", "Minus[-] Number[&H1]"),
        ("a And &HFF", "Identifier[a] Keyword(And)[And] Number[&HFF]"),
        (
            "Chr(&H41)",
            "Identifier[Chr] LeftParen[(] Number[&H41] RightParen[)]",
        ),
        (
            "Array(&H1, &H2)",
            "Identifier[Array] LeftParen[(] Number[&H1] Comma[,] Number[&H2] RightParen[)]",
        ),
        (
            "Const E_FAIL As Long = &H80004005",
            "Keyword(Const)[Const] Identifier[E_FAIL] Keyword(As)[As] Identifier[Long] Equals[=] Number[&H80004005]",
        ),
    ]);
}

#[test]
fn a_hexadecimal_or_octal_prefix_starts_a_number_after_a_name_too() {
    check(&[
        ("Sleep &H64", "Identifier[Sleep] Number[&H64]"),
        ("MsgBox &HFF", "Identifier[MsgBox] Number[&HFF]"),
        (
            "Debug.Print &HFF",
            "Identifier[Debug] Dot[.] Identifier[Print] Number[&HFF]",
        ),
        (
            "keybd_event &H12, 0, 0, 0",
            "Identifier[keybd_event] Number[&H12] Comma[,] Number[0] Comma[,] Number[0] Comma[,] Number[0]",
        ),
        ("a &H10", "Identifier[a] Number[&H10]"),
        (r#""x" &HFF"#, r#"String["x"] Number[&HFF]"#),
        (
            "f(1) &H1",
            "Identifier[f] LeftParen[(] Number[1] RightParen[)] Number[&H1]",
        ),
        ("a & &H10", "Identifier[a] Ampersand[&] Number[&H10]"),
        ("a And &O7", "Identifier[a] Keyword(And)[And] Number[&O7]"),
    ]);
}

#[test]
fn a_bare_octal_prefix_starts_a_number_only_where_an_operand_is_expected() {
    check(&[
        ("x = &17", "Identifier[x] Equals[=] Number[&17]"),
        ("(&7)", "LeftParen[(] Number[&7] RightParen[)]"),
        ("a &17", "Identifier[a] Ampersand[&] Number[17]"),
        (r#""x" &1"#, r#"String["x"] Ampersand[&] Number[1]"#),
        (
            "f(1) &7",
            "Identifier[f] LeftParen[(] Number[1] RightParen[)] Ampersand[&] Number[7]",
        ),
        ("a & &17", "Identifier[a] Ampersand[&] Number[&17]"),
    ]);
}

#[test]
fn an_ampersand_is_an_operator_when_the_digits_do_not_close_a_literal() {
    check(&[
        (
            "x = &Hex(5)",
            "Identifier[x] Equals[=] Ampersand[&] Identifier[Hex] LeftParen[(] Number[5] RightParen[)]",
        ),
        (
            "x = &H",
            "Identifier[x] Equals[=] Ampersand[&] Identifier[H]",
        ),
        (
            "x = &HG1",
            "Identifier[x] Equals[=] Ampersand[&] Identifier[HG1]",
        ),
        (
            "x = &O19",
            "Identifier[x] Equals[=] Ampersand[&] Identifier[O19]",
        ),
        (
            "x = &HFFg",
            "Identifier[x] Equals[=] Ampersand[&] Identifier[HFFg]",
        ),
        ("x = &8", "Identifier[x] Equals[=] Ampersand[&] Number[8]"),
        ("&", "Ampersand[&]"),
        ("&&", "Ampersand[&] Ampersand[&]"),
        ("&HFF&x", "Number[&HFF] Ampersand[&] Identifier[x]"),
    ]);
}
