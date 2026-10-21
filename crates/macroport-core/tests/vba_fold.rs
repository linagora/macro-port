// SPDX-License-Identifier: Apache-2.0
//! Constant folding (SPEC M2 item 4): the strings of a statement, with the
//! expressions of the closed set replaced by their value, and nothing else
//! evaluated. The cases are statements of one logical line.
#![allow(clippy::expect_used)]

use macroport_core::vba::{ConstTable, StatementReader, split_lines, tokenize};

/// The strings of the statement `text`, with the module-level constants
/// `consts`.
fn strings_with(consts: &ConstTable, text: &str) -> Vec<String> {
    StatementReader::new(consts)
        .read_tokens(text, &tokenize(text))
        .strings
}

/// The strings of the statement `text`, with no constant.
fn strings(text: &str) -> Vec<String> {
    strings_with(&ConstTable::default(), text)
}

/// The constants that the statements of `source` declare, in order.
fn consts_of(source: &str) -> ConstTable {
    let mut consts = ConstTable::default();
    for line in &split_lines(source).expect("lines").logical {
        consts.declare(line);
    }
    consts
}

/// Checks that each statement gives the strings next to it.
fn check(cases: &[(&str, &[&str])]) {
    for (text, expected) in cases {
        assert_eq!(strings(text), *expected, "{text}");
    }
}

#[test]
fn a_string_literal_is_listed_with_its_doubled_quotes_read_as_one() {
    check(&[
        (r#"x = "abc""#, &["abc"]),
        (r#"x = "say ""hi"" now""#, &[r#"say "hi" now"#]),
        (r#"x = """#, &[""]),
        (r#"MsgBox "a", vbOKOnly, "b""#, &["a", "b"]),
        ("x = 1 + 2", &[]),
    ]);
}

#[test]
fn literals_joined_with_and_or_plus_are_folded_into_one_string() {
    check(&[
        (r#"x = "WScr" & "ipt.Sh" & "ell""#, &["WScript.Shell"]),
        (r#"x = "WScr" + "ipt.Sh" + "ell""#, &["WScript.Shell"]),
        (r#"x = "a" & "b" + "c""#, &["abc"]),
        (r#"x = ("a" & "b") & ("c" & "d")"#, &["abcd"]),
        (r#"x = (("a"))"#, &["a"]),
        (r#"CreateObject("a" & "b"), "c""#, &["ab", "c"]),
    ]);
}

#[test]
fn chr_chrw_and_chrb_of_a_number_fold_to_a_character() {
    check(&[
        ("x = Chr(65)", &["A"]),
        (
            "x = Chr$(110) & Chr(111) & ChrW(116) & ChrB(101)",
            &["note"],
        ),
        ("x = ChrW(8364)", &["\u{20ac}"]),
        ("x = Chr(&H41) & Chr(&O102) & Chr(67%)", &["ABC"]),
        (
            r#"x = Chr(110) & Chr(111) & "te" & "pad" & Chr(46) & StrReverse("exe")"#,
            &["notepad.exe"],
        ),
        ("x = VBA.Chr(65) & VBA.Strings.Chr$(66)", &["AB"]),
        ("x = Strings.Chr(65) & vba.chr$(66)", &["AB"]),
        ("x = chr(0)", &["\u{0}"]),
        ("x = Chr((65))", &["A"]),
    ]);
}

#[test]
fn strreverse_lcase_and_ucase_fold_the_value_they_are_given() {
    check(&[
        (r#"x = StrReverse("exe")"#, &["exe"]),
        (r#"x = StrReverse("lleHS.tpircSW")"#, &["WScript.SHell"]),
        (r#"x = UCase("abc") & LCase$("DEF")"#, &["ABCdef"]),
        (r#"x = StrReverse(UCase("ab" & "c"))"#, &["CBA"]),
        (r#"x = LCase(StrReverse("AB") & Chr(67))"#, &["bac"]),
    ]);
}

#[test]
fn a_call_of_the_set_with_an_argument_outside_it_stays_unresolved() {
    check(&[
        ("x = Chr(n)", &[]),
        ("x = Chr(65 + 1)", &[]),
        ("x = Chr(65.5)", &[]),
        ("x = Chr(256)", &[]),
        ("x = Chr(-5)", &[]),
        ("x = ChrW(65536)", &[]),
        ("x = ChrW(&HD800)", &[]),
        ("x = Chr(99999999999)", &[]),
        ("x = Chr(65, 66)", &[]),
        ("x = Chr()", &[]),
        ("x = Chr(\"A\")", &["A"]),
        ("x = Chr(Asc(\"A\") + 1)", &["A"]),
        ("x = StrReverse(y)", &[]),
        ("x = StrReverse(65)", &[]),
        (r#"x = UCase(y) & "b""#, &["b"]),
        (r#"x = UCase("a", "b")"#, &["a", "b"]),
        (r#"x = Chr(65) & Chr(y) & Chr(67)"#, &["A", "C"]),
        (r#"x = obj.Chr(65)"#, &[]),
        (r#"x = .Chr(65)"#, &[]),
        (r#"x = Foo.Chr(65)"#, &[]),
        (r#"x = Foo.VBA.Chr(65)"#, &[]),
        (r#"x = VBA.Foo.Chr(65)"#, &[]),
        (r#"x = VBA(1).Chr(65)"#, &[]),
        (r#"x = Chr(65).Foo"#, &[]),
    ]);
}

#[test]
fn a_module_level_const_folds_into_an_expression() {
    let consts =
        consts_of("Private Const P1 = \"WScr\"\nConst P2$ = \"ipt.\", [P 3] = \"Shell\"\n");
    assert_eq!(consts.get("p1"), Some("WScr"));
    assert_eq!(consts.get("P2"), Some("ipt."));
    assert_eq!(consts.get("P 3"), Some("Shell"));
    assert_eq!(
        strings_with(&consts, "Set o = CreateObject(p1 & P2 & [P 3])"),
        ["WScript.Shell"]
    );
    assert_eq!(
        strings_with(&consts, r#"x = UCase(P1) & "-" & Chr(66)"#),
        ["WSCR-B"]
    );
}

#[test]
fn a_lone_const_adds_no_string_but_a_const_in_an_expression_does() {
    let consts = consts_of("Const A = \"x\"\n");
    for lone in ["y = A", "y = (A)", "Foo A, A", "y = A + 1", "y = A.Name"] {
        assert_eq!(strings_with(&consts, lone), Vec::<String>::new(), "{lone}");
    }
    for (text, expected) in [
        ("y = A & A", "xx"),
        (r#"y = A & "z""#, "xz"),
        ("y = UCase(A)", "X"),
    ] {
        assert_eq!(strings_with(&consts, text), [expected], "{text}");
    }
}

#[test]
fn the_const_declarations_that_do_not_fold_to_a_string_are_not_recorded() {
    let consts = consts_of(
        "Const N = 10\nConst S As String = \"s\"\nConst X = Foo(1)\nConst Y = \"a\" & N\n\
         Public Const Z = \"z\" ' comment\nConst B = A\nConst A = \"a\"\nConst C = A & \"c\"\n\
         Dim D As String\nConst\nConst E\nConst F =\n#Const G = \"g\"\n",
    );
    let found: Vec<_> = ["N", "S", "X", "Y", "Z", "B", "A", "C", "D", "E", "F", "G"]
        .into_iter()
        .map(|name| consts.get(name))
        .collect();
    assert_eq!(
        found,
        [
            None,
            Some("s"),
            None,
            None,
            Some("z"),
            None,
            Some("a"),
            Some("ac"),
            None,
            None,
            None,
            None,
        ]
    );
    assert_eq!(consts.len(), 4);
}

#[test]
fn a_const_declared_twice_keeps_its_first_value() {
    let consts = consts_of("Const A = \"first\"\nConst a = \"second\"\n");
    assert_eq!(consts.get("A"), Some("first"));
}

#[test]
fn a_const_declared_with_others_in_one_statement_sees_the_ones_before_it() {
    let consts = consts_of("Const A = \"a\", B = A & \"b\", C = B & A\n");
    assert_eq!(consts.get("C"), Some("aba"));
}

#[test]
fn an_expression_outside_the_closed_set_stays_unresolved() {
    // Each of these leaves every literal apart, and folds nothing around them.
    check(&[
        (r#"x = "a" & y & "b""#, &["a", "b"]),
        (r#"x = "a" & Left("bcd", 2)"#, &["a", "bcd"]),
        (r#"x = Replace("abc", "b", "d")"#, &["abc", "b", "d"]),
        (r#"x = "a" & 1"#, &["a"]),
        (r#"x = "1" + 2"#, &["1"]),
        (r#"x = "a" & vbCrLf & "b""#, &["a", "b"]),
        (r#"x = Mid("abc", 2)"#, &["abc"]),
        (r#"x = String(3, "a")"#, &["a"]),
        (r#"x = "a" & Trim("b")"#, &["a", "b"]),
        (r#"x = "a" & CStr(5) & "b""#, &["a", "b"]),
        (r#"x = "a" & obj.Name & "b""#, &["a", "b"]),
        (r#"x = "a" & obj.Name("c") & "b""#, &["a", "c", "b"]),
        (r#"x = "a" & ws.Cells(1, 1) & "b""#, &["a", "b"]),
        (r#"x = "a" & b!c & "d""#, &["a", "d"]),
        (r#"x = "a" = "b""#, &["a", "b"]),
        (r#"x = "a" & "b" Like "c" & "d""#, &["ab", "cd"]),
        (r#"x = Not "a" & "b""#, &["ab"]),
        (r#"x = -"a" & "b""#, &["a", "b"]),
    ]);
}

#[test]
fn a_string_that_is_not_closed_is_no_literal() {
    check(&[
        (r#"x = "abc"#, &[]),
        (r#"x = "a" & "bc"#, &["a"]),
        (r#"x = "abc"""#, &[]),
    ]);
}

#[test]
fn what_is_not_a_string_expression_is_left_alone() {
    check(&[
        ("x = #1/2/2000#", &[]),
        ("x = 10 & 20", &[]),
        ("If a = b Then c = d", &[]),
        ("For i = 1 To 10 Step 2", &[]),
        ("", &[]),
        ("'", &[]),
        ("& + ) ( , ,", &[]),
        ("x = (", &[]),
        ("x = (\"a\"", &["a"]),
        ("x = )\"a\"", &["a"]),
        (r#"f("a", , "b", ,)"#, &["a", "b"]),
        (r#"f(ByVal "a" As Long, "b")"#, &["a", "b"]),
    ]);
}

#[test]
fn nesting_beyond_the_depth_limit_gives_up_without_overflowing_the_stack() {
    let deep = |open: &str, close: &str, inner: &str| {
        format!(
            "x = {}{inner}{}",
            open.repeat(100_000),
            close.repeat(100_000)
        )
    };
    for text in [
        deep("(", ")", r#""a""#),
        deep("-", "", "1"),
        deep("Not ", "", "a"),
        deep("f(", ")", r#""a""#),
        deep("a.b(", ")", r#""a""#),
        deep("UCase(", ")", r#""a""#),
    ] {
        // Only that it returns: the strings that were read before giving up
        // are not specified.
        let _ = strings(&text);
    }
    // A reasonable nesting still folds.
    let ok = format!("x = {}\"a\"{}", "(".repeat(20), ")".repeat(20));
    assert_eq!(strings(&ok), ["a"]);
    let ok = format!("x = {}\"a\"{}", "UCase(".repeat(20), ")".repeat(20));
    assert_eq!(strings(&ok), ["A"]);
}

#[test]
fn a_folded_value_that_would_outgrow_its_statement_stays_unresolved() {
    // A constant of a few kilobytes, doubled by a statement of a few bytes:
    // the budget of the statement is a small multiple of its own text.
    let big = "x".repeat(4_000);
    let consts = consts_of(&format!("Const A = \"{big}\"\n"));
    assert_eq!(consts.get("a").map(str::len), Some(4_000));
    for text in ["y = A & A", "y = UCase(A)"] {
        assert_eq!(strings_with(&consts, text), Vec::<String>::new(), "{text}");
    }
    // What is left of the statement is listed as it would be anywhere else.
    assert_eq!(strings_with(&consts, "y = A & \"z\""), ["z"]);
    // The literals of the statement itself are never held back.
    let long = "z".repeat(100_000);
    let text = format!("y = \"{long}\" & \"{long}\"");
    assert_eq!(strings(&text), [format!("{long}{long}")]);
}

#[test]
fn constants_cannot_double_their_way_to_a_huge_value() {
    let mut source = String::from("Const C0 = \"0123456789abcdef\"\n");
    for i in 1..=60 {
        source.push_str(&format!("Const C{i} = C{} & C{}\n", i - 1, i - 1));
    }
    let consts = consts_of(&source);
    let longest = (0..=60)
        .filter_map(|i| consts.get(&format!("C{i}")))
        .map(str::len)
        .max();
    assert!(longest.is_some_and(|len| len < 1_000), "{longest:?}");
}

#[test]
fn a_statement_with_many_operands_is_read_without_quadratic_work() {
    // 600,000 tokens, under the cap of the lexer.
    let text = format!("x = {}\"a\"", "\"b\" & ".repeat(300_000));
    let folded = strings(&text);
    assert_eq!(folded.len(), 1);
    assert_eq!(folded.first().map(String::len), Some(300_001));
}
