// SPDX-License-Identifier: Apache-2.0
//! The ProgIDs that a statement passes to `CreateObject` and `GetObject` (SPEC
//! M2 items 3 and 4, SPEC section 7). The cases are statements of one logical
//! line.

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

/// Checks that each statement passes the ProgIDs next to it.
fn check_progids(cases: &[(&str, &[&str])]) {
    for (text, expected) in cases {
        assert_eq!(facts(text).progids, *expected, "{text}");
    }
}

#[test]
fn the_class_argument_of_createobject_is_a_progid() {
    check_progids(&[
        (
            r#"Set o = CreateObject("WScript.Shell")"#,
            &["WScript.Shell"],
        ),
        (r#"set o = createobject("A.B")"#, &["A.B"]),
        (r#"Set o = CreateObject ("A.B")"#, &["A.B"]),
        (r#"Call CreateObject("A.B")"#, &["A.B"]),
        (r#"Set o = CreateObject("A.B", "server")"#, &["A.B"]),
        // What is done with the object does not hide it.
        (r#"CreateObject("A.B").Run "calc""#, &["A.B"]),
        (r#"x = CreateObject("A.B").GetFile(p).Size"#, &["A.B"]),
        (r#"With CreateObject("A.B")"#, &["A.B"]),
        (r#"If a Then x = CreateObject("A.B") Else x = 1"#, &["A.B"]),
    ]);
}

#[test]
fn each_call_names_its_own_progid_in_the_order_of_the_calls() {
    check_progids(&[
        (
            r#"Set o = IIf(a, CreateObject("A.B"), CreateObject("C.D"))"#,
            &["A.B", "C.D"],
        ),
        (
            r#"Set a = CreateObject("A.B"): Set b = CreateObject("A.B")"#,
            &["A.B", "A.B"],
        ),
        // The call outside comes first, as its name does, whatever it is
        // given.
        (
            r#"Set o = CreateObject("A.B", CreateObject("C.D").Name)"#,
            &["A.B", "C.D"],
        ),
        (r#"x = CreateObject(CreateObject("A.B").Name)"#, &["A.B"]),
    ]);
}

#[test]
fn the_class_argument_is_a_progid_after_constant_folding() {
    let consts =
        consts_of("Private Const P1 = \"WScr\"\nConst P2$ = \"ipt.\", [P 3] = \"Shell\"\n");
    for text in [
        r#"Set o = CreateObject("WScr" & "ipt.Sh" & "ell")"#,
        r#"Set o = CreateObject("WScr" + "ipt.Sh" + "ell")"#,
        r#"Set o = CreateObject(Chr(87) & "Script" & Chr(46) & StrReverse("llehS"))"#,
        r#"Set o = CreateObject(StrReverse("llehS.tpircSW"))"#,
        r#"Set o = CreateObject(UCase(LCase("w")) & "Script." & UCase("s") & "hell")"#,
        r#"Set o = CreateObject(("WScript.Shell"))"#,
        "Set o = CreateObject(p1 & P2 & [P 3])",
        r#"Set o = VBA.CreateObject(p1 & "ipt." & "Shell")"#,
    ] {
        let found = facts_with(&consts, text);
        assert_eq!(found.progids, ["WScript.Shell"], "{text}");
        // The folded string is listed as a string as well.
        assert_eq!(found.strings, ["WScript.Shell"], "{text}");
    }
}

#[test]
fn the_class_argument_of_getobject_is_a_progid_and_its_path_never() {
    check_progids(&[
        (r#"Set x = GetObject(, "A.B")"#, &["A.B"]),
        (r#"Set x = GetObject("C:\a.xlsx", "A.B")"#, &["A.B"]),
        (r#"Set x = GetObject("", "A.B")"#, &["A.B"]),
        (r#"Set x = GetObject(, "A." & "B")"#, &["A.B"]),
        (r#"Set x = GetObject("a" & ".b", "A.B")"#, &["A.B"]),
        // The path, or the moniker, of the object is not a class.
        (r#"Set x = GetObject("C:\a.xlsx")"#, &[]),
        (r#"Set x = GetObject("winmgmts:\\.\root\cimv2")"#, &[]),
        (r#"Set x = GetObject("A.B")"#, &[]),
        ("Set x = GetObject(path, cls)", &[]),
        ("Set x = GetObject()", &[]),
    ]);
}

#[test]
fn an_argument_that_does_not_fold_names_no_progid() {
    check_progids(&[
        ("Set o = CreateObject(name)", &[]),
        (r#"Set o = CreateObject("Excel." & app)"#, &[]),
        (r#"Set o = CreateObject(prefix & "A.B")"#, &[]),
        (r#"Set o = CreateObject(Chr(n) & ".B")"#, &[]),
        (r#"Set o = CreateObject(Replace("A-B", "-", ""))"#, &[]),
        ("Set o = CreateObject(65)", &[]),
        ("Set o = CreateObject()", &[]),
        (r#"Set o = CreateObject(, "A.B")"#, &[]),
        ("Set o = CreateObject", &[]),
        // Without parentheses, the call is a statement that keeps nothing.
        (r#"CreateObject "A.B""#, &[]),
        ("Set x = GetObject(, cls)", &[]),
        // No call at all.
        (r#"MsgBox "hello" & vbCrLf & Chr(10)"#, &[]),
    ]);
    // What the call holds is still listed as strings.
    assert_eq!(
        facts(r#"Set o = CreateObject("Excel." & app)"#).strings,
        ["Excel."]
    );
}

#[test]
fn the_class_argument_is_found_by_its_name_wherever_it_stands() {
    check_progids(&[
        (r#"Set o = CreateObject(Class:="A.B")"#, &["A.B"]),
        (r#"Set o = CreateObject(class:="A" & ".B")"#, &["A.B"]),
        (
            r#"Set o = CreateObject(ServerName:="s", Class:="A.B")"#,
            &["A.B"],
        ),
        (r#"Set x = GetObject(Class:="A.B")"#, &["A.B"]),
        (
            r#"Set x = GetObject(Pathname:="p", Class:="A.B")"#,
            &["A.B"],
        ),
        (
            r#"Set x = GetObject(Class:="A.B", Pathname:="p")"#,
            &["A.B"],
        ),
        // An argument that is called otherwise is not the class, whatever
        // its position.
        (r#"Set o = CreateObject(ServerName:="A.B")"#, &[]),
        (r#"Set x = GetObject(Pathname:="A.B")"#, &[]),
        ("Set o = CreateObject(Class:=name)", &[]),
    ]);
}

#[test]
fn the_class_is_listed_whatever_its_shape() {
    // The consumers decide what a ProgID looks like, not the reader.
    check_progids(&[
        (r#"Set o = CreateObject("")"#, &[""]),
        (r#"Set o = CreateObject("not a progid")"#, &["not a progid"]),
        (r#"Set x = GetObject(, "C:\a.xlsx")"#, &["C:\\a.xlsx"]),
    ]);
}

#[test]
fn only_a_call_of_the_vba_library_names_a_progid() {
    check_progids(&[
        (r#"Set o = VBA.CreateObject("A.B")"#, &["A.B"]),
        (r#"Set o = vba.interaction.createobject("A.B")"#, &["A.B"]),
        (r#"Set o = Interaction.CreateObject("A.B")"#, &["A.B"]),
        (r#"Set o = VBA.Interaction.GetObject(, "A.B")"#, &["A.B"]),
        (r#"VBA.CreateObject("A.B").Run "x""#, &["A.B"]),
        // The result of the call may be indexed all the same.
        (r#"Set o = VBA.CreateObject("A.B")(1)"#, &["A.B"]),
        (r#"x = CreateObject("A.B")(1).Name"#, &["A.B"]),
        // The method of another object, which is not the function.
        (r#"Set o = obj.CreateObject("A.B")"#, &[]),
        (r#"Set o = Application.CreateObject("A.B")"#, &[]),
        (r#"Set o = .CreateObject("A.B")"#, &[]),
        (r#"Set o = Foo.VBA.CreateObject("A.B")"#, &[]),
        (r#"Set o = VBA.Foo.CreateObject("A.B")"#, &[]),
        (r#"Set o = VBA(1).CreateObject("A.B")"#, &[]),
        (r#"Set o = rs!CreateObject("A.B")"#, &[]),
    ]);
}

#[test]
fn a_statement_that_declares_something_names_no_progid() {
    check_progids(&[
        (r#"Const C = CreateObject("A.B")"#, &[]),
        (r#"Static o As Object, p = GetObject(, "A.B")"#, &[]),
    ]);
}

#[test]
fn a_statement_nested_beyond_the_limit_names_no_progid_in_the_part_read_flat() {
    let open = "(".repeat(MAX_NESTING);
    let close = ")".repeat(MAX_NESTING);
    let deep = facts(&format!(r#"x = {open}CreateObject("A.B"){close}"#));
    assert!(deep.too_deep);
    assert_eq!(deep.progids, Vec::<String>::new());
    // The literal is still there for the matchers of strings.
    assert_eq!(deep.strings, ["A.B"]);
    let shallow = facts(r#"x = (CreateObject("A.B"))"#);
    assert!(!shallow.too_deep);
    assert_eq!(shallow.progids, ["A.B"]);
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
fn many_calls_in_one_statement_are_read_in_linear_time() {
    // 250,000 tokens, under the cap of the lexer.
    let objects = format!("x = {}1", "CreateObject(\"a.b\") & ".repeat(50_000));
    let found = in_time(|| facts(&objects));
    assert_eq!(found.progids.len(), 50_000);
}
