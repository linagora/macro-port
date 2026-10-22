// SPDX-License-Identifier: Apache-2.0
//! Constant folding (SPEC M2 item 4): the strings of a statement, with the
//! expressions of the closed set replaced by their value, and nothing else
//! evaluated. The cases are statements of one logical line.

// clippy.toml keeps the clock out of the core, not out of its tests.
#![allow(clippy::disallowed_methods, clippy::disallowed_types)]
#![allow(clippy::expect_used)]

use std::time::{Duration, Instant};

use macroport_core::vba::{ConstTable, Facts, MAX_NESTING, StatementReader, split_lines, tokenize};

/// What reading a long statement may take: far above one pass, far below a
/// quadratic one.
const LIMIT: Duration = Duration::from_secs(5);

/// The facts of the statement `text`, with the module-level constants
/// `consts`.
fn facts_with(consts: &ConstTable, text: &str) -> Facts {
    StatementReader::new(consts).read_tokens(text, &tokenize(text))
}

/// The facts of the statement `text`, with no constant.
fn facts(text: &str) -> Facts {
    facts_with(&ConstTable::default(), text)
}

/// The strings of the statement `text`, with the constants `consts`.
fn strings_with(consts: &ConstTable, text: &str) -> Vec<String> {
    facts_with(consts, text).strings
}

/// The strings of the statement `text`, with no constant.
fn strings(text: &str) -> Vec<String> {
    facts(text).strings
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
        (r#"x = "a" ' "b""#, &["a"]),
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
fn chr_and_chrw_of_a_number_fold_to_a_character() {
    check(&[
        ("x = Chr(65)", &["A"]),
        (
            "x = Chr$(110) & Chr(111) & ChrW(116) & ChrW$(101)",
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
        // The case changes one character at a time: `ß` stays.
        (r#"x = UCase("straße")"#, &["STRAßE"]),
        // A call that is not closed folds as far as it reads.
        (r#"x = UCase("a" & "b""#, &["AB"]),
    ]);
}

#[test]
fn a_call_of_the_set_with_an_argument_outside_it_stays_unresolved() {
    check(&[
        ("x = Chr(n)", &[]),
        ("x = Chr(65 + 1)", &[]),
        ("x = Chr(65.5)", &[]),
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
    ]);
}

#[test]
fn the_bytes_of_a_code_page_and_chrb_stay_unresolved() {
    // Above 127, `Chr` is a character of the code page of the machine, and
    // `ChrB` a half character that the next byte completes.
    check(&[
        ("x = Chr(128)", &[]),
        ("x = Chr(255) & Chr(256)", &[]),
        ("x = ChrB(65)", &[]),
        ("x = ChrB(87) & ChrB(0) & ChrB$(83)", &[]),
    ]);
}

#[test]
fn only_a_library_name_may_qualify_a_function_of_the_set() {
    check(&[
        ("x = obj.Chr(65)", &[]),
        ("x = .Chr(65)", &[]),
        ("x = Foo.Chr(65)", &[]),
        ("x = Foo.VBA.Chr(65)", &[]),
        ("x = VBA.Foo.Chr(65)", &[]),
        ("x = VBA(1).Chr(65)", &[]),
        ("x = VBA!Chr(65)", &[]),
        ("x = Chr(65).Foo", &[]),
        (r#"x = Interaction.UCase("a")"#, &["a"]),
        // The module that holds `CreateObject` holds none of these functions.
        (r#"x = VBA.Interaction.UCase("a")"#, &["a"]),
    ]);
}

#[test]
fn what_a_string_function_builds_is_paid_for_out_of_the_text_of_its_statement() {
    // `UCase` nested sixty times around a literal of 100 KB: a few levels
    // copy it, the others are refused, and what was built is listed.
    let long = "a".repeat(100_000);
    let nested = format!("x = {}\"{long}\"{}", "UCase(".repeat(60), ")".repeat(60));
    assert_eq!(strings_in_time(&nested), ["A".repeat(100_000)]);
    // The literals themselves are never held back.
    let joined = format!("y = \"{long}\" & \"{long}\"");
    assert_eq!(strings_in_time(&joined), [format!("{long}{long}")]);
}

#[test]
fn a_module_level_const_folds_into_an_expression() {
    let consts =
        consts_of("Private Const P1 = \"WScr\"\nConst P2$ = \"ipt.\", [P 3] = \"Shell\"\n");
    for (name, expected) in [("p1", "WScr"), ("P2", "ipt."), ("P 3", "Shell")] {
        assert_eq!(consts.get(name), Some(expected), "{name}");
    }
    let progid = "Set o = CreateObject(p1 & P2 & [P 3])";
    assert_eq!(strings_with(&consts, progid), ["WScript.Shell"]);
    let mixed = r#"x = UCase(P1) & "-" & Chr(66)"#;
    assert_eq!(strings_with(&consts, mixed), ["WSCR-B"]);
}

#[test]
fn the_name_of_a_constant_stands_for_its_string_wherever_it_is_used() {
    let consts = consts_of("Const A = \"x\"\n");
    for (text, expected) in [
        ("y = A", &["x"][..]),
        ("Foo A, A", &["x", "x"]),
        ("y = A + 1", &["x"]),
        ("y = A & A", &["xx"]),
        (r#"y = A & "z""#, &["xz"]),
        ("y = UCase(A)", &["X"]),
        // A name is a constant only on its own: it is not one after a dot
        // (the member of another object), with arguments, or as a function.
        ("y = A.Name", &[]),
        ("y = A(1)", &[]),
        (r#"y = ws.A & "z""#, &["z"]),
        (r#"y = .A & "z""#, &["z"]),
        (r#"y = rs!A & "z""#, &["z"]),
        // `Chr` wants a number, and only strings are read from constants.
        ("y = Chr(A)", &["x"]),
    ] {
        assert_eq!(strings_with(&consts, text), expected, "{text}");
    }
    let number = consts_of("Const N = 65\n");
    assert_eq!(strings_with(&number, "y = Chr(N)"), Vec::<String>::new());
}

#[test]
fn a_const_statement_lists_its_strings_and_not_the_names_it_declares() {
    let consts = consts_of("Const A = \"x\"\nConst B = A & \"b\"\n");
    for (text, expected) in [
        ("Const A = \"x\"", &["x"][..]),
        (
            "Public Const B As String = A & \"b\", C = \"c\"",
            &["xb", "c"],
        ),
        ("#Const A = \"y\"", &["y"]),
        ("Const A", &[]),
    ] {
        assert_eq!(strings_with(&consts, text), expected, "{text}");
    }
}

#[test]
fn the_const_declarations_that_do_not_fold_to_a_string_have_no_value() {
    let consts = consts_of(
        "Const N = 10\nConst S As String = \"s\"\nConst X = Foo(1)\nConst Y = \"a\" & N\n\
         Public Const Z = \"z\" ' comment\nConst B = A\nConst A = \"a\"\nConst C = A & \"c\"\n\
         Dim D As String\nConst\nConst E\nConst F =\n#Const G = \"g\"\n\
         Const H = IIf(a, I = \"i\", \"h\"), J = \"j\"\n",
    );
    let found = [
        "N", "S", "X", "Y", "Z", "B", "A", "C", "D", "E", "F", "G", "H", "I", "J",
    ]
    .map(|name| consts.get(name));
    let expected = [
        None,
        Some("s"),
        None,
        None,
        Some("z"),
        // `A` is declared after `B`: a constant sees those before it.
        None,
        Some("a"),
        Some("ac"),
        None,
        None,
        None,
        None,
        // The comma inside the parentheses does not end the declarator.
        None,
        None,
        Some("j"),
    ];
    assert_eq!(found, expected);
}

#[test]
fn a_name_declared_twice_with_different_values_has_none() {
    let same = consts_of("Const A = \"v\"\nConst a = \"v\"\n");
    assert_eq!(same.get("A"), Some("v"));
    let differ = consts_of("Const A = \"first\"\nConst a = \"second\"\n");
    assert_eq!(differ.get("A"), None);
    let branches = consts_of(
        "#If Mac Then\nPrivate Const SEP = \"/\"\n#Else\nPrivate Const SEP = \"\\\"\n#End If\n",
    );
    assert_eq!(branches.get("SEP"), None);
}

#[test]
fn a_constant_too_long_for_the_statement_that_uses_it_stays_unresolved() {
    // A constant of a few kilobytes, repeated by a statement of a few bytes:
    // the budget of the statement is a small multiple of its own text.
    let big = "x".repeat(4_000);
    let consts = consts_of(&format!("Const A = \"{big}\"\n"));
    assert_eq!(consts.get("a").map(str::len), Some(4_000));
    for text in ["y = A", "y = A & A", "y = UCase(A)"] {
        assert_eq!(strings_with(&consts, text), Vec::<String>::new(), "{text}");
    }
    // What is left of the statement is listed as it would be anywhere else.
    assert_eq!(strings_with(&consts, "y = A & \"z\""), ["z"]);
}

#[test]
fn a_declaration_pays_for_all_its_declarators_out_of_one_budget() {
    let big = "x".repeat(3_000);
    let mut source = format!("Const A = \"{big}\"\nConst B0 = UCase(A)");
    for i in 1..100 {
        source.push_str(&format!(", B{i} = UCase(A)"));
    }
    let consts = consts_of(&source);
    // The first copies the constant twice, which the text of the statement
    // allows; the others have nothing left.
    assert_eq!(consts.get("B0").map(str::len), Some(3_000));
    assert_eq!(consts.get("B1"), None);
    assert_eq!(consts.get("B99"), None);
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
    assert!(longest.is_some_and(|len| len <= 2_048), "{longest:?}");
}

#[test]
fn an_expression_outside_the_closed_set_stays_unresolved() {
    // Each of these folds only what the closed set covers, and leaves the
    // other literals apart.
    check(&[
        (r#"x = "a" & y & "b""#, &["a", "b"]),
        (r#"x = "a" & Left("bcd", 2)"#, &["a", "bcd"]),
        (r#"x = Replace("abc", "b", "d")"#, &["abc", "b", "d"]),
        (r#"x = "a" & 1"#, &["a"]),
        (r#"x = "1" + 2"#, &["1"]),
        (r#"x = "a" & vbCrLf & "b""#, &["a", "b"]),
        (r#"x = Mid("abc", 2)"#, &["abc"]),
        (r#"x = "a" & Trim("b")"#, &["a", "b"]),
        (r#"x = "a" & obj.Name("c") & "b""#, &["a", "c", "b"]),
        (r#"x = "a" & ws.Cells(1, 1) & "b""#, &["a", "b"]),
        (r#"x = "a" & b!c & "d""#, &["a", "d"]),
        (r#"x = -"a" & "b""#, &["a", "b"]),
    ]);
}

#[test]
fn the_operators_group_as_in_vba() {
    check(&[
        // `+` binds tighter than `&`, and `*` tighter than both.
        (r#"x = "a" & "b" + y"#, &["a", "b"]),
        (r#"x = "a" + "b" & y"#, &["ab"]),
        (r#"x = "a" & "b" * 2"#, &["a", "b"]),
        // Comparisons bind looser than `&`, `Not` looser than a comparison.
        (r#"x = "a" & "b" Like "c" & "d""#, &["ab", "cd"]),
        (r#"x = "a" = "b""#, &["a", "b"]),
        (r#"x = Not "a" & "b""#, &["ab"]),
        // The operators group from the left: nothing joins to what comes
        // after an operand that is not known.
        (
            r#"x = s & "WScr" & "ipt.Sh" & "ell""#,
            &["WScr", "ipt.Sh", "ell"],
        ),
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
        ("& + ) ( , ,", &[]),
        ("x = (", &[]),
        ("x = (\"a\"", &["a"]),
        ("x = )\"a\"", &["a"]),
        (r#"f("a", , "b", ,)"#, &["a", "b"]),
        (r#"f(ByVal "a" As Long, "b")"#, &["a", "b"]),
    ]);
}

#[test]
fn a_line_that_is_not_code_holds_no_string() {
    let consts = ConstTable::default();
    let mut reader = StatementReader::new(&consts);
    let lines = split_lines("' x = \"a\"\nDone:\nx = \"b\"\n")
        .expect("lines")
        .logical;
    let read: Vec<_> = lines.iter().map(|line| reader.read(line).strings).collect();
    assert_eq!(read, [vec![], vec![], vec!["b".to_string()]]);
}

#[test]
fn nesting_beyond_the_limit_reads_the_rest_flat_and_flags_the_statement() {
    let nested = |groups: usize| {
        let (open, close) = ("(".repeat(groups), ")".repeat(groups));
        facts(&format!(r#"{open}"a" & "b"{close} & "c""#))
    };
    // The statement and the right operand of `&` each take a level.
    let fits = nested(MAX_NESTING.saturating_sub(2));
    assert_eq!(
        (fits.strings, fits.too_deep),
        (vec!["abc".to_string()], false)
    );
    let beyond = nested(MAX_NESTING.saturating_sub(1));
    assert_eq!(beyond.strings, ["a", "b", "c"]);
    assert!(beyond.too_deep);
}

#[test]
fn deep_nesting_of_every_shape_keeps_every_string_without_overflowing_the_stack() {
    let deep = |open: &str, close: &str| {
        let depth = 100_000;
        format!(
            r#"x = {}"a"{} & "b""#,
            open.repeat(depth),
            close.repeat(depth)
        )
    };
    let shapes = [
        deep("(", ")"),
        deep("-", ""),
        deep("Not ", ""),
        deep("f(", ")"),
        deep("a.b(", ")"),
        deep("UCase(", ")"),
    ];
    for (shape, text) in shapes.iter().enumerate() {
        let read = facts(text);
        assert!(read.too_deep, "shape {shape}");
        assert!(read.strings.contains(&"b".to_string()), "shape {shape}");
    }
}

/// The strings of the statement `text`, which must be read within [`LIMIT`].
fn strings_in_time(text: &str) -> Vec<String> {
    let started = Instant::now();
    let read = strings(text);
    let elapsed = started.elapsed();
    assert!(elapsed < LIMIT, "took {elapsed:?}");
    read
}

#[test]
fn a_long_statement_is_read_in_linear_time() {
    // 600,000 tokens, under the cap of the lexer, in two shapes: every join
    // folds, or none does.
    let folded = strings_in_time(&format!("x = {}\"a\"", "\"b\" & ".repeat(300_000)));
    assert_eq!(
        folded.iter().map(String::len).collect::<Vec<_>>(),
        [300_001]
    );
    let apart = strings_in_time(&format!("x = {}\"a\"", "y & \"b\" & ".repeat(100_000)));
    assert_eq!(apart.len(), 100_001);
}
