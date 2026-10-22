// SPDX-License-Identifier: Apache-2.0
//! The procedures that each procedure calls (SPEC M2 item 5, ADR-0012): the
//! call graph of the project, fed by the statements of every module of the file,
//! answers once all of them are read.

use macroport_core::analyze::analyze;
use macroport_core::report::{FileReport, ProcedureReport};
use macroport_core::rules::ModuleType;
use macroport_core::vba::MAX_LINES;

use crate::support::{
    analyse_in_linear_time, checked, codes, options, procedures, report_of_modules, source,
};

/// The calls of a procedure as names, `None` if the report leaves the list out.
type Called<'a> = Option<Vec<&'a str>>;

/// The report of the modules `(name, type, text)`, in that order.
fn report_of(modules: &[(&str, ModuleType, &str)]) -> FileReport {
    let modules = (modules.iter())
        .map(|&(name, module_type, text)| source(name, module_type, text))
        .collect();
    report_of_modules(modules)
}

/// The names that a procedure calls, if its report lists them.
fn called(procedure: &ProcedureReport) -> Called<'_> {
    let names = procedure.calls.as_ref();
    names.map(|names| names.iter().map(String::as_str).collect())
}

/// Each procedure of `file` in order, with the names it calls.
fn calls(file: &FileReport) -> Vec<(&str, Called<'_>)> {
    procedures(file)
        .map(|procedure| (procedure.name.as_str(), called(procedure)))
        .collect()
}

/// A list of names from a literal.
fn some<'a>(names: &[&'a str]) -> Called<'a> {
    Some(names.to_vec())
}

/// The message of the only error of `file`.
fn only_message(file: &FileReport) -> &str {
    let errors = file.errors.as_deref().unwrap_or_default();
    assert_eq!(errors.len(), 1, "{errors:?}");
    errors.first().map_or("", |error| error.message.as_str())
}

/// A module that calls `Helper` twice, spelled two ways, and `Twice` once.
const CALLER: &str = "\
Sub Start()
    helper 1
    y = TWICE(2)
    Call Helper
End Sub
";

/// A module that declares what `CALLER` calls.
const CALLED: &str = "\
Public Sub Helper(n As Long)
End Sub

Public Function Twice(n As Long) As Long
End Function
";

/// `count` procedures called `P0` and on, each with no statement.
fn declared(count: usize) -> String {
    (0..count)
        .map(|number| format!("Sub P{number}()\nEnd Sub\n"))
        .collect()
}

/// The text of a procedure called `name` that calls the first `count` of them.
fn calling(name: &str, count: usize) -> String {
    let statements: Vec<_> = (0..count).map(|number| format!("    P{number}")).collect();
    format!("Sub {name}()\n{}\nEnd Sub\n", statements.join("\n"))
}

#[test]
fn a_procedure_lists_the_procedures_it_calls_by_the_names_the_project_declares_them_with() {
    let file = report_of(&[
        ("Caller", ModuleType::Standard, CALLER),
        ("Called", ModuleType::Standard, CALLED),
    ]);

    // Each once and in byte order, though the code spells `Helper` two ways and
    // calls it twice, and the module that declares `Twice` comes after.
    let expected = [
        ("Start", some(&["Helper", "Twice"])),
        ("Helper", some(&[])),
        ("Twice", some(&[])),
    ];
    assert_eq!(calls(&file), expected);
    assert_eq!(codes(&file), Vec::<&str>::new());
}

#[test]
fn the_order_of_the_modules_changes_the_order_of_the_procedures_and_nothing_else() {
    let file = report_of(&[
        ("Called", ModuleType::Standard, CALLED),
        ("Caller", ModuleType::Standard, CALLER),
    ]);

    let expected = [
        ("Helper", some(&[])),
        ("Twice", some(&[])),
        ("Start", some(&["Helper", "Twice"])),
    ];
    assert_eq!(calls(&file), expected);
}

#[test]
fn a_procedure_that_calls_nothing_has_an_empty_list_and_not_a_missing_one() {
    let file = report_of(&[(
        "Module1",
        ModuleType::Standard,
        "Sub A()\n    x = 1\nEnd Sub\n",
    )]);

    assert_eq!(calls(&file), [("A", some(&[]))]);
}

#[test]
fn a_procedure_that_calls_itself_calls_nothing_by_that_but_a_pair_that_call_each_other_do() {
    let own = "Sub A(n As Long)\n    If n > 0 Then A n - 1\nEnd Sub\n";
    let pair = "Sub B()\n    C\nEnd Sub\nSub C()\n    B\nEnd Sub\n";

    let file = report_of(&[
        ("Own", ModuleType::Standard, own),
        ("Pair", ModuleType::Standard, pair),
    ]);

    let expected = [("A", some(&[])), ("B", some(&["C"])), ("C", some(&["B"]))];
    assert_eq!(calls(&file), expected);
}

#[test]
fn a_statement_outside_the_procedures_is_the_call_of_none() {
    let text = "Dim x As Long\nx = Helper\nSub A()\nEnd Sub\nSub Helper()\nEnd Sub\n";

    let file = report_of(&[("Module1", ModuleType::Standard, text)]);

    assert_eq!(calls(&file), [("A", some(&[])), ("Helper", some(&[]))]);
}

#[test]
fn a_bare_name_reaches_its_module_and_the_public_procedures_of_the_standard_modules() {
    let first = "\
Sub Start()
    Mine
    Hidden
    Shown
    Fetch
End Sub

Sub Dotted()
    c.Fetch
End Sub

Private Sub Mine()
End Sub
";
    let second = "Private Sub Hidden()\nEnd Sub\nPublic Sub Shown()\nEnd Sub\n";
    let class = "Public Sub Fetch()\nEnd Sub\n";

    let file = report_of(&[
        ("Module1", ModuleType::Standard, first),
        ("Module2", ModuleType::Standard, second),
        ("Class1", ModuleType::Class, class),
    ]);

    // `Hidden` is private to another module and `Fetch` is not in a standard
    // module: a bare name reaches neither. After a dot, a name can be any
    // procedure of that name.
    let all = calls(&file);
    assert_eq!(all.first(), Some(&("Start", some(&["Mine", "Shown"]))));
    assert_eq!(all.get(1), Some(&("Dotted", some(&["Fetch"]))));
}

#[test]
fn a_name_that_a_statement_gives_to_run_is_a_call() {
    let first = "Sub Start()\n    Application.Run \"Module2.Helper\"\nEnd Sub\n";
    let second = "Public Sub Helper()\nEnd Sub\n";

    let file = report_of(&[
        ("Module1", ModuleType::Standard, first),
        ("Module2", ModuleType::Standard, second),
    ]);

    assert_eq!(
        calls(&file),
        [("Start", some(&["Helper"])), ("Helper", some(&[]))]
    );
}

#[test]
fn a_text_file_is_a_project_of_its_own_and_lists_only_the_calls_within_it() {
    let text = "Sub Start()\n    Finish\n    Elsewhere\nEnd Sub\nPrivate Sub Finish()\nEnd Sub\n";

    let file = checked(analyze(text.as_bytes(), "Module1.bas", &options()));

    // Another file may declare `Elsewhere`, and this one cannot tell.
    assert_eq!(
        calls(&file),
        [("Start", some(&["Finish"])), ("Finish", some(&[]))]
    );
}

#[test]
fn a_name_is_not_told_from_a_parameter_or_from_the_member_of_an_object() {
    let first = "Sub Start(Helper As Long)\n    x = Helper\n    y = fso.BuildPath(a, b)\nEnd Sub\n";
    let second = "Public Sub Helper()\nEnd Sub\nPublic Sub BuildPath()\nEnd Sub\n";

    let file = report_of(&[
        ("Module1", ModuleType::Standard, first),
        ("Module2", ModuleType::Standard, second),
    ]);

    // The graph reads names and not types: `Helper` is a parameter here and
    // `fso` is no module, and each name is taken for a call of the procedure
    // that has it, which errs on the side of a procedure being used.
    let all = calls(&file);
    assert_eq!(
        all.first(),
        Some(&("Start", some(&["BuildPath", "Helper"])))
    );
}

#[test]
fn a_module_that_cannot_be_read_leaves_the_calls_of_the_whole_file_unknown() {
    let big = "a\n".repeat(MAX_LINES as usize + 1);
    let caller = "Sub A()\n    B\nEnd Sub\n";
    let called = "Public Sub B()\nEnd Sub\n";

    let file = report_of(&[
        ("First", ModuleType::Standard, caller),
        ("Big", ModuleType::Standard, &big),
        ("Last", ModuleType::Standard, called),
    ]);

    // `Big` may declare any procedure and call any: a list of the others would
    // not be whole, so none is given. They are read all the same.
    assert_eq!(codes(&file), ["vba.too_many_lines"]);
    assert_eq!(calls(&file), [("A", None), ("B", None)]);
    let complexity = procedures(&file).map(|procedure| procedure.metrics.cyclomatic);
    assert_eq!(complexity.collect::<Vec<_>>(), [Some(1), Some(1)]);
}

#[test]
fn a_procedure_may_call_a_thousand_procedures_and_one_that_calls_more_has_no_list() {
    let text = format!(
        "{}{}{}",
        declared(1_001),
        calling("Thousand", 1_000),
        calling("More", 1_001)
    );

    let file = report_of(&[("Module1", ModuleType::Standard, &text)]);

    let all = calls(&file);
    let length_of = |name: &str| {
        let found = all.iter().find(|(procedure, _)| *procedure == name);
        let (_, names) = found.expect("a procedure of that name");
        names.as_ref().map(Vec::len)
    };
    assert_eq!(length_of("Thousand"), Some(1_000));
    assert_eq!(length_of("More"), None);
    // The others are read, and the file tells of the one list that did not fit.
    assert_eq!((length_of("P0"), length_of("P1000")), (Some(0), Some(0)));
    assert_eq!(codes(&file), ["vba.calls.left_out"]);
    // The graph is whole all the same: a list too long to keep is no reason to
    // leave `possibly_unused` out, and the error does not say that it is.
    assert!(procedures(&file).all(|procedure| procedure.possibly_unused.is_some()));
    assert!(!only_message(&file).contains("possibly_unused"));
    let line = (procedures(&file).find(|procedure| procedure.name == "More"))
        .map(|procedure| procedure.start_line)
        .expect("a procedure called More");
    let expected =
        format!("1 procedure has no list of calls (it starts at line {line} of module Module1):");
    assert!(
        only_message(&file).starts_with(&expected),
        "{}",
        only_message(&file)
    );
}

#[test]
fn names_past_the_bound_of_the_graph_leave_every_list_out_and_the_file_tells_of_it() {
    // 5,600 distinct names of 200 characters weigh more than the 1 MiB of names
    // that the graph keeps.
    let names: Vec<_> = (0..5_600)
        .map(|n| format!("    {}{n:04}", "x".repeat(196)))
        .collect();
    let big = format!("Sub A()\n{}\nEnd Sub\n", names.join("\n"));
    let small = "Sub B()\n    A\nEnd Sub\n";

    let file = report_of(&[
        ("Big", ModuleType::Standard, &big),
        ("Small", ModuleType::Standard, small),
    ]);

    // Both modules are read and their procedures measured, and no list is known.
    assert_eq!(calls(&file), [("A", None), ("B", None)]);
    let complexity = procedures(&file).map(|procedure| procedure.metrics.cyclomatic);
    assert_eq!(complexity.collect::<Vec<_>>(), [Some(1), Some(1)]);
    assert_eq!(codes(&file), ["vba.calls.left_out"]);
    let expected = "2 procedures have no list of calls (the first starts at line 1 of module Big):";
    assert!(
        only_message(&file).starts_with(expected),
        "{}",
        only_message(&file)
    );
    // Nor does the graph tell which procedures nothing calls: the names it did
    // not keep may be calls. The error says so, and the entry points are known.
    assert!(procedures(&file).all(|procedure| procedure.possibly_unused.is_none()));
    assert!(procedures(&file).all(|procedure| procedure.is_entry_point == Some(true)));
    let too = "; `possibly_unused` is left out of them too, since";
    assert!(only_message(&file).contains(too), "{}", only_message(&file));
}

#[test]
fn the_calls_of_a_module_are_found_in_time_linear_in_its_size() {
    // Every name is new, and no procedure has it.
    analyse_in_linear_time("one procedure, new names", 40_000, |statements| {
        let lines: Vec<_> = (0..statements).map(|n| format!("    x{n}")).collect();
        format!("Sub A()\n{}\nEnd Sub\n", lines.join("\n"))
    });
    // Each of many procedures calls the next.
    analyse_in_linear_time("a chain of procedures", 20_000, |count| {
        (0..count)
            .map(|n| format!("Sub P{n}()\n    P{}\nEnd Sub\n", n + 1))
            .collect()
    });
    // Each of many procedures calls the same hundred, whose lists add up.
    analyse_in_linear_time("many lists of a hundred", 40_000, |statements| {
        let callers = (0..statements / 100).map(|n| calling(&format!("Q{n}"), 100));
        declared(100) + &callers.collect::<String>()
    });
}
