// SPDX-License-Identifier: Apache-2.0
//! The marks of obfuscation that the structural predicate `chr_obfuscation`
//! reads (SPEC M2 items 3 and 4, SPEC section 7), and the facts of random
//! statements. The cases are statements of one logical line.

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

/// The constants that the statements of `source` declare, in order.
fn consts_of(source: &str) -> ConstTable {
    let mut consts = ConstTable::default();
    for line in &split_lines(source).expect("lines").logical {
        consts.declare(line);
    }
    consts
}

/// Checks that each statement shows a mark of obfuscation.
fn check_marked(statements: &[&str]) {
    for text in statements {
        assert!(facts(text).chr_obfuscation, "{text}");
    }
}

/// Checks that no statement shows a mark of obfuscation.
fn check_unmarked(statements: &[&str]) {
    for text in statements {
        assert!(!facts(text).chr_obfuscation, "{text}");
    }
}

#[test]
fn three_calls_of_chr_joined_by_and_or_plus_are_a_mark() {
    check_marked(&[
        "x = Chr(104) & Chr(116) & Chr(116)",
        "x = ChrW(104) & ChrW(116) & ChrW(116)",
        "x = ChrB(104) & ChrB(116) & ChrB$(116)",
        "x = Chr$(1) & ChrW$(2) & ChrB$(3)",
        "x = Chr(1) + Chr(2) + Chr(3)",
        "x = Chr(1) & Chr(2) + Chr(3)",
        "x = Chr(1) & Chr(2) & Chr(3) & Chr(4)",
        "x = CHR(1) & chr(2) & cHr(3)",
        // Whether the calls fold or not, and whatever lies between them.
        "x = Chr(a) & Chr(b) & Chr(c)",
        "x = Chr(65.5) & Chr(-1) & Chr()",
        "x = Chr(1) & s & Chr(2) & t & Chr(3)",
        r#"x = Chr(1) & "a" & Chr(2) & Foo(1) & Chr(3)"#,
        "x = Chr(1) & obj.Name & Chr(2) & Chr(3)",
        // The calls of the library of VBA.
        "x = VBA.Chr(1) & VBA.Strings.ChrW$(2) & Strings.ChrB(3)",
        // Groups and the functions of the closed set around a join.
        "x = (Chr(1) & Chr(2)) & Chr(3)",
        "x = Chr(1) & (Chr(2) & Chr(3))",
        "x = ((Chr(1) & Chr(2) & Chr(3)))",
        "x = UCase(Chr(1) & Chr(2)) & Chr(3)",
        "x = Chr(1) & LCase(StrReverse(Chr(2) & Chr(3)))",
        // A `Not` reads the join as its operand.
        "x = Not Chr(1) & Chr(2) & Chr(3)",
        // Wherever the expression is.
        "Call Foo(Chr(1) & Chr(2) & Chr(3))",
        "Foo Chr(1) & Chr(2) & Chr(3), 1",
        "Const A = Chr(1) & Chr(2) & Chr(3)",
        "If Chr(1) & Chr(2) & Chr(3) = s Then x = 1",
        "x = Foo(Chr(1) & Chr(2) & Chr(3), 4).Bar",
        "Foo.Bar(Chr(1) & Chr(2) & Chr(3)) = 4",
    ]);
}

#[test]
fn text_with_fewer_than_three_calls_of_chr_is_no_mark() {
    check_unmarked(&[
        "x = Chr(1)",
        "x = Chr(1) & Chr(2)",
        "x = Chr(1) & Chr(2) ' & Chr(3)",
        "x = Chr & Chr & Chr",
        "x = Asc(a) & Asc(b) & Asc(c)",
        r#"x = "Chr(1) & Chr(2) & Chr(3)""#,
        "' x = Chr(1) & Chr(2) & Chr(3)",
    ]);
}

#[test]
fn three_calls_of_chr_that_are_not_all_joined_to_each_other_are_no_mark() {
    check_unmarked(&[
        // Another operator breaks the join.
        "x = Chr(1) & Chr(2) * Chr(3)",
        "x = Chr(1) & Chr(2) = Chr(3)",
        "x = Chr(1) & Chr(2) And Chr(3)",
        "x = Chr(1) & Chr(2) - Chr(3)",
        // The negation of a call is no call, so it counts for none.
        "x = -Chr(1) & Chr(2) & Chr(3)",
        // Calls in the arguments of another function, or apart.
        "x = Foo(Chr(1), Chr(2), Chr(3))",
        "x = Foo(Chr(1) & Chr(2), Chr(3))",
        "x = Chr(1) & Foo(Chr(2) & Chr(3))",
        "x = Chr(1) & Chr(2) & Foo(Chr(3))",
        "x = Chr(Chr(1) & Chr(2), Chr(3))",
        "x = Chr(1) & Chr(2): y = Chr(3)",
    ]);
}

#[test]
fn only_the_functions_of_the_library_of_vba_count_as_calls_of_chr() {
    check_unmarked(&[
        "x = obj.Chr(1) & obj.Chr(2) & obj.Chr(3)",
        "x = .Chr(1) & .Chr(2) & .Chr(3)",
        "x = Foo.VBA.Chr(1) & Foo.VBA.Chr(2) & Foo.VBA.Chr(3)",
        "x = rs!Chr(1) & rs!Chr(2) & rs!Chr(3)",
    ]);
}

#[test]
fn a_string_reversed_where_it_is_written_is_a_mark() {
    check_marked(&[
        r#"x = StrReverse("exe.dnammoc")"#,
        r#"x = strreverse$("exe")"#,
        r#"x = VBA.StrReverse("exe")"#,
        r#"x = StrReverse("a" & "b")"#,
        r#"x = StrReverse(("abc"))"#,
        r#"x = StrReverse("a" + "b" & "c")"#,
        r#"x = "a" & StrReverse("b")"#,
        r#"Foo StrReverse("abc"), 1"#,
        r#"Set o = CreateObject(StrReverse("llehS.tpircSW"))"#,
        r#"x = StrReverse(StrReverse("abc"))"#,
    ]);
}

#[test]
fn a_string_reversed_that_is_not_a_literal_is_no_mark() {
    check_unmarked(&[
        "x = StrReverse(s)",
        "x = StrReverse(obj.Name)",
        r#"x = StrReverse(Left("abc", 2))"#,
        r#"x = StrReverse("a" & s)"#,
        r#"x = StrReverse("a" & Chr(66))"#,
        r#"x = StrReverse(UCase("abc"))"#,
        "x = StrReverse(Chr(65))",
        r#"x = obj.StrReverse("abc")"#,
        r#"x = UCase("abc") & LCase("DEF")"#,
        r#"x = StrReverse("abc"#,
        r#"' x = StrReverse("abc")"#,
    ]);
    // A constant is a name, not a literal.
    let consts = consts_of("Const C = \"abc\"\n");
    assert!(!facts_with(&consts, "x = StrReverse(C)").chr_obfuscation);
}

#[test]
fn the_part_of_a_statement_read_flat_is_not_searched_for_marks() {
    let open = "(".repeat(MAX_NESTING);
    let close = ")".repeat(MAX_NESTING);
    let deep = facts(&format!("x = {open}Chr(1) & Chr(2) & Chr(3){close}"));
    assert!(deep.too_deep);
    assert!(!deep.chr_obfuscation);
    let shallow = facts("x = (Chr(1) & Chr(2) & Chr(3))");
    assert!(!shallow.too_deep);
    assert!(shallow.chr_obfuscation);
}

#[test]
fn a_mark_found_before_the_part_read_flat_stays() {
    let open = "(".repeat(MAX_NESTING);
    let close = ")".repeat(MAX_NESTING);
    let calls = facts(&format!("x = Chr(1) & Chr(2) & Chr(3) & {open}1{close}"));
    assert!(calls.too_deep);
    assert!(calls.chr_obfuscation);
    let reversed = facts(&format!(r#"x = StrReverse("a") & {open}1{close}"#));
    assert!(reversed.too_deep);
    assert!(reversed.chr_obfuscation);
}

/// What `work` gives, which it must give within [`LIMIT`].
fn in_time<T>(work: impl FnOnce() -> T) -> T {
    let started = Instant::now();
    let result = work();
    let elapsed = started.elapsed();
    assert!(elapsed < LIMIT, "took {elapsed:?}");
    result
}

#[test]
fn a_long_join_of_calls_is_read_in_linear_time() {
    // 500,000 and 560,000 tokens, under the cap of the lexer.
    let joined = format!("x = {}1", "Chr(1) & ".repeat(100_000));
    assert!(in_time(|| facts(&joined)).chr_obfuscation);
    let apart = format!("x = {}1", "Chr(1) & x * ".repeat(80_000));
    assert!(!in_time(|| facts(&apart)).chr_obfuscation);
}

/// A statement of `length` pieces of a vocabulary of calls, literals and
/// punctuation, picked by a generator that is the same on every run.
fn soup(state: &mut u64, length: usize) -> String {
    const WORDS: [&str; 34] = [
        "CreateObject(",
        "GetObject(",
        "Chr(",
        "ChrW$(",
        "ChrB(",
        "StrReverse(",
        "UCase(",
        "VBA.",
        "Interaction.",
        ".",
        "!",
        "\"a.b\"",
        "\"Wscript.Shell\"",
        "\"",
        "65",
        "&",
        "+",
        "*",
        "=",
        ",",
        ")",
        "(",
        "x",
        "Const",
        "With",
        "Dim",
        "Class:=",
        "Set",
        "Call",
        " ",
        " _\n",
        "'",
        ":",
        "Else",
    ];
    let mut text = String::new();
    for _ in 0..length {
        *state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        let pick = usize::try_from(*state >> 33)
            .ok()
            .and_then(|random| random.checked_rem(WORDS.len()))
            .unwrap_or(0);
        text.push_str(WORDS.get(pick).copied().unwrap_or(" "));
        text.push(' ');
    }
    text
}

#[test]
fn what_the_reader_names_is_among_its_strings_and_what_it_marks_is_in_the_text() {
    let consts = ConstTable::default();
    let mut state = 0x9e37_79b9_7f4a_7c15_u64;
    let mut named = 0usize;
    let mut marked = 0usize;
    for round in 0..20_000usize {
        let text = soup(&mut state, round % 40);
        let mut reader = StatementReader::new(&consts);
        let found = reader.read_tokens(&text, &tokenize(&text));
        for progid in &found.progids {
            assert!(found.strings.contains(progid), "{text:?}: {progid:?}");
        }
        named += found.progids.len();
        if found.chr_obfuscation {
            let lower = text.to_ascii_lowercase();
            let calls = lower.matches("chr").count();
            assert!(calls >= 3 || lower.contains("strreverse"), "{text:?}");
            marked += 1;
        }
    }
    // The vocabulary reaches both.
    assert!(named > 100, "{named}");
    assert!(marked > 100, "{marked}");
}
