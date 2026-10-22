// SPDX-License-Identifier: Apache-2.0
//! The APIs that each procedure uses (SPEC M2 item 3, ADR-0011): the member
//! chains, the functions called by name and the ProgIDs of its statements, each
//! with how many times the procedure uses it.

use macroport_core::analyze::{Options, analyze_modules};
use macroport_core::report::{FileReport, ProcedureReport, Status};
use macroport_core::rules::{ModuleType, RuleSet, RulesSources};
use macroport_core::vba::MAX_NESTING;
use macroport_testkit::linear::assert_linear_time;

use crate::support::{checked, codes, module, options, procedures, report_of_modules, source};

/// The list of a procedure as `(name, count)` pairs, `None` if the report
/// leaves it out.
type Used = Option<Vec<(String, u32)>>;

fn pairs(procedure: &ProcedureReport) -> Used {
    let apis = procedure.apis.as_ref()?;
    Some(
        (apis.iter())
            .map(|api| (api.name.clone(), api.count))
            .collect(),
    )
}

/// A list from a literal.
fn used(entries: &[(&str, u32)]) -> Used {
    Some(
        (entries.iter())
            .map(|&(name, count)| (name.to_owned(), count))
            .collect(),
    )
}

/// The report of the standard module `text`.
fn report_of(text: &str) -> FileReport {
    report_of_modules(vec![source("Module1", ModuleType::Standard, text)])
}

/// The list of each procedure of `file`, in order.
fn lists(file: &FileReport) -> Vec<Used> {
    procedures(file).map(pairs).collect()
}

/// The messages of the errors of `file`, in order.
fn messages(file: &FileReport) -> Vec<&str> {
    let errors = file.errors.iter().flatten();
    errors.map(|error| error.message.as_str()).collect()
}

/// The text of a procedure called `name` whose statements are `lines`.
fn sub<S: AsRef<str>>(name: &str, lines: &[S]) -> String {
    let lines: Vec<_> = lines.iter().map(AsRef::as_ref).collect();
    format!("Sub {name}()\n{}\nEnd Sub\n", lines.join("\n"))
}

/// The list of the only procedure of a module whose statements are `lines`.
fn list_of<S: AsRef<str>>(lines: &[S]) -> Used {
    let mut lists = lists(&report_of(&sub("A", lines)));
    assert_eq!(lists.len(), 1);
    lists.remove(0)
}

/// `count` statements, each of which uses an API that no other uses.
fn distinct(count: usize) -> Vec<String> {
    (0..count)
        .map(|number| format!("    x.m{number}"))
        .collect()
}

#[test]
fn a_procedure_that_uses_no_api_has_an_empty_list_and_not_a_missing_one() {
    let file = report_of(&sub("A", &["x = 1"]));

    assert_eq!(module(&file).procedures[0].apis, Some(Vec::new()));
}

#[test]
fn the_member_chains_of_a_procedure_are_its_apis_with_the_with_blocks_resolved() {
    let list = list_of(&[
        "Workbooks.Open \"book.xlsx\"",
        "With Range(\"A1\")",
        "    .Font.Bold = True",
        "    .Interior.Color = 255",
        "End With",
        "Range(\"B1\").Interior.Color = 0",
    ]);

    let expected = [
        ("Range.Font.Bold", 1),
        ("Range.Interior.Color", 2),
        ("Workbooks.Open", 1),
    ];
    assert_eq!(list, used(&expected));
}

#[test]
fn names_that_differ_only_by_case_are_one_api_spelled_as_it_was_first_used() {
    let list = list_of(&[
        "Range(\"A1\").Value = 1",
        "range(\"A2\").value = 2",
        "RANGE(\"A3\").VALUE = 3",
    ]);

    assert_eq!(list, used(&[("Range.Value", 3)]));
}

#[test]
fn the_apis_are_in_the_order_of_their_names_with_the_case_folded() {
    let list = list_of(&[
        "Workbooks.Open \"a.xlsx\"",
        "application.Calculate",
        "Application.ScreenUpdating = False",
        "ws.Cells.Clear",
    ]);

    // By their bytes, `Application.ScreenUpdating` would come before
    // `Workbooks.Open`, and `application.Calculate` after it.
    let expected = [
        ("application.Calculate", 1),
        ("Application.ScreenUpdating", 1),
        ("Workbooks.Open", 1),
        ("ws.Cells.Clear", 1),
    ];
    assert_eq!(list, used(&expected));
}

#[test]
fn a_function_is_listed_only_if_a_rule_names_it_or_the_module_declares_it() {
    let declaring = "Private Declare Function GetTickCount Lib \"kernel32\" () As Long\n\
                     Sub Declared()\n    t = GetTickCount()\n    Shell \"calc\"\nEnd Sub\n";
    let other = sub(
        "Undeclared",
        &[
            "t = GetTickCount()",
            "Frobnicate t",
            "Helper t",
            "s = Trim(t)",
        ],
    );
    let file = report_of_modules(vec![
        source("Declaring", ModuleType::Standard, declaring),
        source("Other", ModuleType::Standard, &other),
    ]);

    // `Shell` and `Trim` are named by rules, and `GetTickCount` by the `Declare`
    // of the first module, which the second does not see. `t` and `s` are
    // variables, and nothing names `Frobnicate` or `Helper`.
    assert_eq!(
        lists(&file),
        [
            used(&[("GetTickCount", 1), ("Shell", 1)]),
            used(&[("Trim", 1)])
        ]
    );
}

#[test]
fn a_name_after_a_dot_is_a_chain_and_not_the_function_of_that_name() {
    let list = list_of(&["x = ws.Trim(y)", "x = Application.WorksheetFunction.Len(y)"]);

    // `Trim` and `Len` are functions that rules name, but these are members.
    let expected = [("Application.WorksheetFunction.Len", 1), ("ws.Trim", 1)];
    assert_eq!(list, used(&expected));
}

#[test]
fn a_function_that_the_vba_library_qualifies_is_listed_as_its_chain() {
    let list = list_of(&[
        "s = Replace(s, \"a\", \"b\")",
        "s = VBA.Strings.Replace(s, \"a\", \"b\")",
        "Interaction.Shell \"calc\"",
    ]);

    // The bare call and the qualified one are two spellings, listed as written.
    let expected = [
        ("Interaction.Shell", 1),
        ("Replace", 1),
        ("VBA.Strings.Replace", 1),
    ];
    assert_eq!(list, used(&expected));
}

#[test]
fn a_call_that_starts_a_chain_is_listed_as_the_chain_and_not_by_its_name() {
    // A family rule names `Intersect`, so a call of it that starts no chain is
    // listed by its name.
    let list = list_of(&[
        "x = Intersect(a, b).Address",
        "y = Intersect(a, b)",
        "z = Intersect(a, b).Count + Intersect(c, d)",
    ]);

    // Of the four calls, two start a chain and two stand on their own.
    let expected = [
        ("Intersect", 2),
        ("Intersect.Address", 1),
        ("Intersect.Count", 1),
    ];
    assert_eq!(list, used(&expected));
}

#[test]
fn a_progid_is_listed_with_the_function_that_creates_the_object() {
    let list = list_of(&[
        "Set d = CreateObject(\"Scripting.Dictionary\")",
        "Set w = GetObject(, \"Word.Application\")",
        "Set s = CreateObject(\"WScript\" & \".Shell\")",
        "Set t = createobject(\"scripting.dictionary\")",
    ]);

    let expected = [
        ("CreateObject:Scripting.Dictionary", 2),
        ("CreateObject:WScript.Shell", 1),
        ("GetObject:Word.Application", 1),
    ];
    assert_eq!(list, used(&expected));
}

#[test]
fn a_progid_whose_function_the_statement_does_not_show_has_a_neutral_label() {
    // The class of `CreateObject` is a variable, so the facts hold two calls
    // and one ProgID, and the call of the class cannot be told.
    let list = list_of(&["x = IIf(ok, CreateObject(name), GetObject(, \"Word.Application\"))"]);

    assert_eq!(list, used(&[("ProgID:Word.Application", 1)]));
}

#[test]
fn the_method_of_an_object_is_a_chain_and_gives_no_progid_to_the_call_beside_it() {
    let list = list_of(&["x = IIf(ok, app.CreateObject(\"A.B\"), GetObject(, \"C.D\"))"]);

    // The method is no function of the library, so the one ProgID that the
    // statement holds is that of `GetObject`, not a neutral one.
    let expected = [("app.CreateObject", 1), ("GetObject:C.D", 1)];
    assert_eq!(list, used(&expected));
}

#[test]
fn each_procedure_has_its_own_list_and_the_code_outside_them_belongs_to_none() {
    let text = format!(
        "Dim w As Object\nSet w = Workbooks.Open(\"a.xlsx\")\n{}{}{}",
        sub("A", &["Application.Calculate"]),
        sub("B", &["Application.Calculate", "Application.Calculate"]),
        sub::<&str>("C", &[]),
    );

    let file = report_of(&text);

    let calculate = |count| used(&[("Application.Calculate", count)]);
    assert_eq!(lists(&file), [calculate(1), calculate(2), used(&[])]);
}

#[test]
fn a_name_in_brackets_is_one_element_of_a_chain_whatever_text_it_holds() {
    let list = list_of(&["x = [a.b].c", "y = a.b.c", "z = [a b <i>].d"]);

    // The dot in the brackets is part of the name, so `[a.b].c` and `a.b.c` are
    // one entry, and the text of the file goes into the report as it is.
    assert_eq!(list, used(&[("a b <i>.d", 1), ("a.b.c", 2)]));
}

#[test]
fn a_declare_names_its_function_in_whichever_branch_of_a_conditional_compilation_holds_it() {
    let text = "#If Win64 Then\n#Else\n\
                Private Declare Function GetTickCount Lib \"kernel32\" () As Long\n\
                #End If\n\
                Sub A()\n    t = GetTickCount()\nEnd Sub\n";

    let file = report_of(text);

    assert_eq!(lists(&file), [used(&[("GetTickCount", 1)])]);
}

#[test]
fn a_with_block_inside_another_continues_the_chain_of_the_one_around_it() {
    let list = list_of(&[
        "With Range(\"A1\")",
        "    With .Font",
        "        .Bold = True",
        "    End With",
        "End With",
    ]);

    let expected = [("Range.Font", 1), ("Range.Font.Bold", 1)];
    assert_eq!(list, used(&expected));
}

#[test]
fn a_with_block_whose_object_is_no_name_gives_its_members_nothing_to_start_from() {
    let list = list_of(&["With New Collection", "    .Add 1", "End With"]);

    // `.Add` alone is no chain, and nothing says what it belongs to.
    assert_eq!(list, used(&[]));
}

#[test]
fn a_statement_nested_too_deep_gives_no_chain_but_its_functions_are_listed() {
    let levels = MAX_NESTING + 1;
    let (open, close) = ("(".repeat(levels), ")".repeat(levels));

    let file = report_of(&sub(
        "A",
        &[
            format!("x = {open}ws.Cells.Clear{close}"),
            format!("y = {open}Trim(ws.Cells.Clear){close}"),
            "Range(\"A1\").Value = 1".to_owned(),
        ],
    ));

    // The reader reads the rest of such a statement flat: no chain is found in
    // it, and `Trim` stands alone like a name that a rule names.
    assert_eq!(lists(&file), [used(&[("Range.Value", 1), ("Trim", 1)])]);
    assert_eq!(codes(&file), ["vba.statement.too_deep"]);
}

#[test]
fn a_procedure_may_use_a_thousand_names_and_one_that_uses_more_is_left_out_whole() {
    let thousand = sub("Thousand", &distinct(1_000));
    let more = sub("More", &distinct(1_001));
    let few = sub("Few", &["y.z"]);

    let file = report_of(&format!("{thousand}{more}{few}"));

    let counts: Vec<_> = lists(&file)
        .into_iter()
        .map(|list| list.map(|names| names.len()))
        .collect();
    assert_eq!(counts, [Some(1_000), None, Some(1)]);
    // The file says so, naming the module and where the procedure starts: the
    // first holds 1,000 statements, so the second starts at line 1003.
    assert_eq!(codes(&file), ["vba.apis.left_out"]);
    let expected = "module Module1: 1 procedure uses more APIs than a report lists";
    assert!(
        messages(&file)[0].starts_with(expected),
        "{:?}",
        messages(&file)
    );
    assert!(messages(&file)[0].contains("it starts at line 1003,"));
    assert_eq!(file.status, Status::Ok);
}

#[test]
fn the_lists_of_a_file_share_a_room_and_a_list_that_does_not_fit_is_left_out() {
    // The lists of a file take 4 MiB together, and an entry weighs about twice
    // its name (the name and its folded copy): of these names, one fits and two
    // do not. A list that does not fit takes nothing, so a later one is read.
    let long = "n".repeat(1_500_000);
    let using_long = |name: &str| sub(name, &[format!("x.{long}")]);
    let second = format!("{}{}", using_long("B"), sub("C", &["y.z"]));

    let file = report_of_modules(vec![
        source("First", ModuleType::Standard, &using_long("A")),
        source("Second", ModuleType::Standard, &second),
    ]);

    let counts: Vec<_> = lists(&file)
        .into_iter()
        .map(|list| list.map(|names| names.len()))
        .collect();
    assert_eq!(counts, [Some(1), None, Some(1)]);
    assert_eq!(codes(&file), ["vba.apis.left_out"]);
    let expected = "module Second: 1 procedure uses more APIs than a report lists";
    assert!(
        messages(&file)[0].starts_with(expected),
        "{:?}",
        messages(&file)
    );
}

#[test]
fn the_functions_that_a_list_names_are_those_of_the_rules_in_use() {
    let security = RulesSources::EMBEDDED
        .security
        .replace("pattern: [Shell]", "pattern: [Shell, MyHelper]");
    assert_ne!(
        security,
        RulesSources::EMBEDDED.security,
        "the rule is there"
    );
    let sources = RulesSources {
        security: &security,
        ..RulesSources::EMBEDDED
    };
    let widened = RuleSet::load(&sources).expect("the changed rules load");
    let text = sub("A", &["MyHelper 1", "Shell \"calc\""]);
    let modules = || vec![source("Module1", ModuleType::Standard, &text)];

    let shipped = report_of_modules(modules());
    let other = checked(analyze_modules(
        modules(),
        "Module1.bas",
        &Options::new(&widened),
    ));

    assert_eq!(lists(&shipped), [used(&[("Shell", 1)])]);
    assert_eq!(lists(&other), [used(&[("MyHelper", 1), ("Shell", 1)])]);
}

/// Analyses the module that `text` gives for a quarter of `size` statements,
/// then the one it gives for `size`, and fails if the time grows faster than the
/// size (`macroport_testkit::linear`).
fn analyse_in_linear_time(what: &str, size: usize, text: impl Fn(usize) -> String) {
    let modules = |statements| vec![source("Module1", ModuleType::Standard, &text(statements))];
    assert_linear_time(what, size, modules, |modules| {
        analyze_modules(modules.clone(), "Module1.bas", &options())
    });
}

#[test]
fn the_lists_of_a_module_are_built_in_time_linear_in_its_size() {
    // Every name is new. In one procedure the list is left out past a thousand
    // names, and the statements after that cost no more than the others.
    analyse_in_linear_time("one procedure", 40_000, |statements| {
        sub("A", &distinct(statements))
    });
    // In procedures of a hundred statements each, the lists stay and add up.
    analyse_in_linear_time("many procedures", 40_000, |statements| {
        (0..statements / 100)
            .map(|number| sub(&format!("P{number}"), &distinct(100)))
            .collect()
    });
}
