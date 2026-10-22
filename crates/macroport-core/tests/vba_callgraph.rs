// SPDX-License-Identifier: Apache-2.0
//! The call graph and the entry points (SPEC M2 item 5). Each case is a small
//! project: its modules are parsed, and the graph is fed the way the front end
//! feeds it, a statement at a time. The cases name what the graph says as
//! `module:name`, the module being the position in the project.

// clippy.toml excuses `expect` in the `#[test]` functions only, not in helpers.
#![allow(clippy::expect_used)]

mod vba_callgraph_common;

use macroport_core::rules::ModuleType::{self, Class, Document, Form, Standard};
use macroport_core::vba::{CallGraph, ProcedureLinks};
use vba_callgraph_common::Module;

/// The graph of the modules of a project, for each procedure: its name and what
/// the graph says of it.
struct Project(Vec<Vec<(String, ProcedureLinks)>>);

impl Project {
    /// The links of the procedure called `name` of the module `module`: the
    /// first one if the module has several.
    fn links(&self, module: usize, name: &str) -> &ProcedureLinks {
        let procedures = self.0.get(module).expect("a module");
        let found = procedures.iter().find(|(found, _)| found == name);
        assert!(found.is_some(), "no {name} in module {module}");
        &found.expect("the procedure").1
    }

    /// The procedures that are possibly unused, as `module:name`.
    fn unused(&self) -> Vec<String> {
        self.select(|links| links.possibly_unused == Some(true))
    }

    /// The procedures that are entry points, as `module:name`.
    fn entries(&self) -> Vec<String> {
        self.select(|links| links.is_entry_point)
    }

    /// The procedures that the predicate holds for, as `module:name`.
    fn select(&self, predicate: impl Fn(&ProcedureLinks) -> bool) -> Vec<String> {
        let modules = self.0.iter().enumerate();
        let procedures = modules.flat_map(|(module, list)| {
            list.iter()
                .filter(|(_, links)| predicate(links))
                .map(move |(name, _)| format!("{module}:{name}"))
        });
        procedures.collect()
    }

    /// The procedures that the procedure `name` of the module `module` calls.
    fn calls(&self, module: usize, name: &str) -> Vec<&str> {
        let calls = self.links(module, name).calls.as_ref();
        let calls = calls.expect("the calls of a graph read in full");
        calls.iter().map(String::as_str).collect()
    }
}

/// Adds a module to the graph and reads it a procedure at a time. `started`
/// names the procedures that the host starts. Gives the names of the
/// procedures of the module.
fn add(
    graph: &mut CallGraph,
    module_type: ModuleType,
    text: &str,
    started: &[&str],
) -> Vec<String> {
    let module = Module::read(module_type, text);
    let flags: Vec<bool> = (module.procedures.iter())
        .map(|procedure| started.contains(&procedure.name.as_str()))
        .collect();
    module.add_to(graph, &flags);
    let names = module
        .procedures
        .into_iter()
        .map(|procedure| procedure.name);
    names.collect()
}

/// The graph of the project made of `modules`, where the host starts the
/// procedures named in `started`.
fn project_with(modules: &[(ModuleType, &str)], started: &[&str]) -> Project {
    let mut graph = CallGraph::new();
    let names: Vec<Vec<String>> = (modules.iter())
        .map(|&(module_type, text)| add(&mut graph, module_type, text, started))
        .collect();
    let links = graph.resolve();
    assert_eq!(links.len(), names.len());
    let modules = names.into_iter().zip(links).map(|(names, links)| {
        assert_eq!(links.len(), names.len());
        names.into_iter().zip(links).collect()
    });
    Project(modules.collect())
}

/// The graph of the project made of `modules`, where the host starts nothing.
fn project(modules: &[(ModuleType, &str)]) -> Project {
    project_with(modules, &[])
}

/// The graph of a project of one standard module.
fn standard(text: &str) -> Project {
    project(&[(Standard, text)])
}

#[test]
fn a_public_procedure_of_a_standard_module_without_parameters_is_an_entry_point() {
    // A button or a shortcut may run it. No visibility keyword means public.
    let entries = standard(
        "Sub Plain()\nEnd Sub\n\
         Public Sub Listed()\nEnd Sub\n\
         Private Sub Hidden()\nEnd Sub\n\
         Friend Sub Friendly()\nEnd Sub\n\
         Sub WithParameter(ByVal x As Long)\nEnd Sub\n\
         Sub WithOptional(Optional ByVal x As Long)\nEnd Sub\n",
    )
    .entries();
    assert_eq!(entries, ["0:Plain", "0:Listed"]);
}

#[test]
fn a_public_function_of_a_standard_module_is_an_entry_point_whatever_its_parameters() {
    // A worksheet formula may call it.
    let entries = standard(
        "Function Formula(ByVal x As Long) As Long\nEnd Function\n\
         Public Function Plain() As Long\nEnd Function\n\
         Private Function Hidden(ByVal x As Long) As Long\nEnd Function\n",
    )
    .entries();
    assert_eq!(entries, ["0:Formula", "0:Plain"]);
}

#[test]
fn the_property_procedures_of_a_standard_module_are_entry_points_together() {
    // The SPEC says public procedures without parameters: the `Get` is one. The
    // `Let` takes a parameter, but it is the same property, which other modules
    // reach by its name: a name is an entry point as soon as one of its
    // procedures is.
    let project = standard(
        "Public Property Get Reading() As Long\nEnd Property\n\
         Public Property Let Reading(ByVal v As Long)\nEnd Property\n\
         Public Property Let Writing(ByVal v As Long)\nEnd Property\n",
    );
    assert_eq!(project.entries(), ["0:Reading", "0:Reading"]);
    assert_eq!(project.unused(), ["0:Writing"]);
}

#[test]
fn a_group_is_started_by_the_host_when_one_of_its_procedures_is() {
    let text = "Public Property Get Handler() As Long\nEnd Property\n\
                Public Property Let Handler(ByVal v As Long)\nEnd Property\n\
                Public Sub Idle()\nEnd Sub\n";
    let mut graph = CallGraph::new();
    Module::read(Class, text).add_to(&mut graph, &[false, true, false]);
    let links = graph.resolve();
    let entries: Vec<bool> = links[0].iter().map(|links| links.is_entry_point).collect();
    assert_eq!(entries, [true, true, false]);
}

#[test]
fn the_procedures_of_the_other_modules_are_entry_points_only_when_the_host_starts_them() {
    for module_type in [Class, Document, Form] {
        let text = "Public Sub Plain()\nEnd Sub\nPublic Function Formula() As Long\nEnd Function\n";
        assert_eq!(project(&[(module_type, text)]).entries(), [] as [&str; 0]);
    }
}

#[test]
fn a_procedure_that_the_host_starts_is_an_entry_point_and_is_not_unused() {
    let text = "Private Sub Workbook_Open()\nEnd Sub\nPrivate Sub Idle()\nEnd Sub\n";
    let project = project_with(&[(Document, text)], &["Workbook_Open"]);
    assert_eq!(project.entries(), ["0:Workbook_Open"]);
    assert_eq!(project.unused(), ["0:Idle"]);
}

#[test]
fn a_procedure_that_the_host_starts_is_not_unused_and_what_it_calls_is_used() {
    let text =
        "Private Sub Workbook_Open()\n    Prepare\nEnd Sub\nPrivate Sub Prepare()\nEnd Sub\n";
    let project = project_with(&[(Document, text)], &["Workbook_Open"]);
    assert_eq!(project.unused(), [] as [&str; 0]);
    assert_eq!(project.calls(0, "Workbook_Open"), ["Prepare"]);
}

#[test]
fn a_flag_that_is_missing_means_that_the_host_does_not_start_the_procedure() {
    let text = "Private Sub First()\nEnd Sub\nPrivate Sub Second()\nEnd Sub\n";
    let mut graph = CallGraph::new();
    graph.add_module(Document, &Module::read(Document, text).procedures, &[true]);
    let links = graph.resolve();
    let entries: Vec<bool> = links[0].iter().map(|links| links.is_entry_point).collect();
    assert_eq!(entries, [true, false]);
}

#[test]
fn the_events_of_a_class_are_entry_points() {
    let text = "Private Sub Class_Initialize()\nEnd Sub\n\
                Private Sub CLASS_TERMINATE()\nEnd Sub\n\
                Private Sub Class_Other()\nEnd Sub\n";
    assert_eq!(
        project(&[(Class, text)]).entries(),
        ["0:Class_Initialize", "0:CLASS_TERMINATE"]
    );
    // They are events of classes only.
    for module_type in [Document, Form] {
        assert_eq!(project(&[(module_type, text)]).entries(), [] as [&str; 0]);
    }
    assert_eq!(project(&[(Standard, text)]).entries(), [] as [&str; 0]);
}

#[test]
fn a_procedure_that_no_other_refers_to_is_possibly_unused() {
    let project = standard(
        "Sub Main()\n    Used\n    Call AlsoUsed(1)\n    x = Counted(2)\nEnd Sub\n\
         Private Sub Used()\nEnd Sub\n\
         Private Sub AlsoUsed(ByVal n As Long)\nEnd Sub\n\
         Private Function Counted(ByVal n As Long) As Long\nEnd Function\n\
         Private Sub Unused()\nEnd Sub\n\
         Private Sub UnusedWithParameter(ByVal n As Long)\nEnd Sub\n",
    );
    assert_eq!(project.unused(), ["0:Unused", "0:UnusedWithParameter"]);
    assert_eq!(project.entries(), ["0:Main"]);
    assert_eq!(project.calls(0, "Main"), ["AlsoUsed", "Counted", "Used"]);
}

/// Private procedures with the names, which have no statement and no parameter.
fn empties(names: &[&str]) -> String {
    let procedures = (names.iter()).map(|name| format!("Private Sub {name}()\nEnd Sub\n"));
    procedures.collect()
}

#[test]
fn a_name_invoked_bare_refers_to_the_procedure_whether_it_takes_arguments_or_not() {
    let main = "Sub Main()\n\
                \x20   ByStatement\n\
                \x20   Call ByCall\n\
                \x20   ByArguments 1, 2\n\
                \x20   x = ByFunction(3)\n\
                \x20   y = ByValue\n\
                \x20   SetTimer 0, 0, 1000, AddressOf ByAddress\n\
                End Sub\n";
    let others = [
        "ByStatement",
        "ByCall",
        "ByArguments",
        "ByFunction",
        "ByValue",
        "ByAddress",
        "Unreferred",
    ];
    let project = standard(&format!("{main}{}", empties(&others)));
    assert_eq!(project.unused(), ["0:Unreferred"]);
    assert_eq!(
        project.calls(0, "Main"),
        [
            "ByAddress",
            "ByArguments",
            "ByCall",
            "ByFunction",
            "ByStatement",
            "ByValue"
        ]
    );
}

#[test]
fn a_name_after_a_dot_or_used_as_a_member_refers_to_every_procedure_of_that_name() {
    // The name says nothing of where the procedure is: it may be in any module,
    // private or not, since the object may be of any class.
    let main = r#"
Sub Main()
    Dim o As Object
    o.ByMethod 1
    Module1.ByQualified 2
    Call o.ByCall
    x = o.ByProperty
    o.ByAssigned = 3
    With o
        .ByWith
    End With
    Me.ByMe
End Sub
"#;
    let hidden = empties(&["ByQualified", "ByCall"]);
    let class = r#"
Public Sub ByMethod(ByVal x As Long)
End Sub
Public Property Get ByProperty() As Long
End Property
Public Property Let ByAssigned(ByVal v As Long)
End Property
Public Sub ByWith()
End Sub
Public Sub ByMe()
End Sub
Public Sub Other()
End Sub
"#;
    let result = project(&[(Standard, main), (Standard, &hidden), (Class, class)]);
    assert_eq!(result.unused(), ["2:Other"]);
    assert_eq!(
        result.calls(0, "Main"),
        [
            "ByAssigned",
            "ByCall",
            "ByMe",
            "ByMethod",
            "ByProperty",
            "ByQualified",
            "ByWith"
        ]
    );
}

#[test]
fn a_bare_name_refers_to_the_procedure_of_its_own_module_before_any_other() {
    let caller = format!("Sub Main()\n    Helper\nEnd Sub\n{}", empties(&["Helper"]));
    let private = empties(&["Helper"]);
    let public = "Public Sub Helper(ByVal x As Long)\nEnd Sub\n";
    let result = project(&[
        (Standard, &caller),
        (Standard, &private),
        (Standard, public),
    ]);
    // The `Helper` next to `Main` is the one it calls: the private one of
    // another module is out of sight, and the public one is hidden by it.
    assert_eq!(result.unused(), ["1:Helper", "2:Helper"]);
    assert_eq!(result.calls(0, "Main"), ["Helper"]);
}

#[test]
fn a_bare_name_without_a_procedure_in_the_module_refers_to_public_procedures_of_standard_modules() {
    let caller = "Sub Main()\n    Helper\nEnd Sub\n";
    let private = empties(&["Helper"]);
    let public = "Public Sub Helper(ByVal x As Long)\nEnd Sub\n";
    let result = project(&[
        (Standard, caller),
        (Standard, &private),
        (Standard, public),
        (Class, public),
        (Document, public),
        (Form, public),
    ]);
    // Another module can call a public procedure of a standard module by its
    // name, and nothing else.
    assert_eq!(
        result.unused(),
        ["1:Helper", "3:Helper", "4:Helper", "5:Helper"]
    );
    assert_eq!(result.calls(0, "Main"), ["Helper"]);
}

#[test]
fn a_bare_name_in_a_class_refers_to_the_procedure_of_the_class_too() {
    let class = "Public Sub Run2()\n    Helper\nEnd Sub\nPrivate Sub Helper()\nEnd Sub\n";
    let standard_module = empties(&["Helper"]);
    let result = project(&[(Class, class), (Standard, &standard_module)]);
    assert_eq!(result.unused(), ["0:Run2", "1:Helper"]);
}

#[test]
fn a_string_given_to_a_call_that_starts_a_procedure_by_its_name_refers_to_it() {
    let main = r#"
Sub Main()
    Application.Run "ByRun"
    Application.Run "Module1.ByQualifiedRun", 1
    Application.Run "'Book1.xlsm'!Module1.ByWorkbookRun"
    CallByName obj, "ByCallByName", VbMethod
    Application.OnTime Now + TimeValue("00:00:01"), "ByOnTime"
    Application.OnKey "^h", "ByOnKey"
    btn.OnAction = "ByOnAction"
    MsgBox "ByMessage"
    x = "ByAssignment"
End Sub
"#;
    let others = [
        "ByRun",
        "ByQualifiedRun",
        "ByWorkbookRun",
        "ByCallByName",
        "ByOnTime",
        "ByOnKey",
        "ByOnAction",
        "ByMessage",
        "ByAssignment",
    ];
    let result = standard(&format!("{main}{}", empties(&others)));
    // A string that is given to anything else names no procedure.
    assert_eq!(result.unused(), ["0:ByMessage", "0:ByAssignment"]);
    assert_eq!(
        result.calls(0, "Main"),
        [
            "ByCallByName",
            "ByOnAction",
            "ByOnKey",
            "ByOnTime",
            "ByQualifiedRun",
            "ByRun",
            "ByWorkbookRun"
        ]
    );
}

#[test]
fn a_string_that_folds_to_a_name_refers_to_the_procedure() {
    let main = r#"
Private Const Macro As String = "ByConstant"
Sub Main()
    Application.Run Macro
    Application.Run "By" & "Concatenation"
    Application.Run "By" & ChrW(67) & "hr"
End Sub
"#;
    let others = ["ByConstant", "ByConcatenation", "ByChr", "ByNothing"];
    let result = standard(&format!("{main}{}", empties(&others)));
    assert_eq!(result.unused(), ["0:ByNothing"]);
}

#[test]
fn a_name_that_does_not_fold_refers_to_nothing() {
    let main = r#"
Sub Main()
    Dim name As String
    name = "Helper"
    Application.Run name
    Application.Run name & suffix
End Sub
"#;
    // A variable holds the name: the string that was assigned to it is in
    // another statement, which starts nothing.
    let result = standard(&format!("{main}{}", empties(&["Helper"])));
    assert_eq!(result.unused(), ["0:Helper"]);
}

#[test]
fn every_string_of_the_statement_that_starts_a_procedure_is_read_as_a_name() {
    // The graph does not know which argument of `Run` is the macro: it reads
    // them all, which errs on the side of a procedure being used.
    let main = "Sub Main()\n    Application.Run \"Other\", \"Argument\"\nEnd Sub\n";
    let result = standard(&format!("{main}{}", empties(&["Other", "Argument"])));
    assert_eq!(result.unused(), [] as [&str; 0]);
}

#[test]
fn the_literals_of_an_expression_that_does_not_fold_are_read_as_names_too() {
    // The facts list the literals of an expression they cannot fold one by one:
    // `Macro` is all that `"Macro" & n` gives, and the graph takes it for the
    // name of a procedure, which errs on the side of a procedure being used.
    let main = "Sub Main()\n    Application.Run \"Macro\" & n\nEnd Sub\n";
    let result = standard(&format!("{main}{}", empties(&["Macro", "MacroTwo"])));
    assert_eq!(result.unused(), ["0:MacroTwo"]);
    assert_eq!(result.calls(0, "Main"), ["Macro"]);
}

#[test]
fn a_string_that_is_not_the_name_of_a_procedure_refers_to_nothing() {
    let main = r#"
Sub Main()
    Application.Run "Helper()"
    Application.Run "My Helper"
    Application.Run "1Helper"
    Application.Run "-Helper"
    Application.Run "Module1."
    Application.Run ""
End Sub
"#;
    let result = standard(&format!("{main}{}", empties(&["Helper"])));
    assert_eq!(result.unused(), ["0:Helper"]);
}

#[test]
fn a_string_is_read_up_to_a_length_that_a_qualified_name_may_have() {
    // The workbook that qualifies a name may have a long path.
    let call = |length: usize| {
        let qualifier = "a".repeat(length.saturating_sub(".Helper".len()));
        let main = format!("Sub Main()\n    Application.Run \"{qualifier}.Helper\"\nEnd Sub\n");
        standard(&format!("{main}{}", empties(&["Helper"]))).unused()
    };
    assert_eq!(call(1024), [] as [&str; 0]);
    assert_eq!(call(1025), ["0:Helper"]);
}

#[test]
fn the_name_of_an_argument_is_no_reference() {
    let main = "Sub Main()\n    Fill Pattern:=1, Color:=2\n    Fill Color\nEnd Sub\n";
    let result = standard(&format!("{main}{}", empties(&["Pattern", "Color"])));
    // `Color` is also used as a value in the second statement.
    assert_eq!(result.unused(), ["0:Pattern"]);
}

#[test]
fn the_value_of_an_argument_that_has_its_name_is_a_bare_name() {
    // `Filename:=Filename` names the argument and passes the value of `Filename`.
    // The value is a bare name, which is the `Filename` of the module of the
    // statement or one that every module can call, and not the private one of
    // another module.
    let caller = "Sub Main()\n    Workbooks.Open Filename:=Filename\nEnd Sub\n";
    let private = "Private Function Filename() As String\nEnd Function\n";
    let result = project(&[(Standard, caller), (Standard, private)]);
    assert_eq!(result.unused(), ["1:Filename"]);
    assert_eq!(result.calls(0, "Main"), [] as [&str; 0]);

    let own = format!("{caller}{private}");
    let result = standard(&own);
    assert_eq!(result.unused(), [] as [&str; 0]);
    assert_eq!(result.calls(0, "Main"), ["Filename"]);

    let public = "Public Function Filename() As String\nEnd Function\n";
    let result = project(&[(Standard, caller), (Standard, public)]);
    assert_eq!(result.unused(), [] as [&str; 0]);
    assert_eq!(result.calls(0, "Main"), ["Filename"]);
}

#[test]
fn names_compare_in_any_letter_case_and_without_the_type_character() {
    let main = r#"
Sub Main()
    HELPER
    x = total$(1)
    GÉNÉRATION
End Sub
Private Sub helper()
End Sub
Private Function Total$(ByVal n As Long)
End Function
Private Sub Génération()
End Sub
"#;
    let result = standard(main);
    assert_eq!(result.unused(), [] as [&str; 0]);
    assert_eq!(result.calls(0, "Main"), ["Génération", "Total", "helper"]);
}

#[test]
fn the_calls_are_the_names_the_project_declares_sorted_and_once() {
    let main = r#"
Sub Main()
    zulu
    ALPHA
    alpha
    Mike
    beta
    x = Range("A1").Value
    Cells(1, 1).Select
End Sub
"#;
    let result = standard(&format!(
        "{main}{}",
        empties(&["Zulu", "Alpha", "Mike", "beta"])
    ));
    // The names of the project, as it spells them; capitals go before lower
    // case; `Range`, `Value` and the others are no procedure of the project.
    assert_eq!(result.calls(0, "Main"), ["Alpha", "Mike", "Zulu", "beta"]);
}

#[test]
fn a_name_that_modules_spell_differently_gets_the_smallest_spelling_in_any_order() {
    // Capitals go before lower case in byte order.
    let caller = "Sub Main()\n    o.Helper\nEnd Sub\n";
    let upper = empties(&["HELPER"]);
    let lower = empties(&["helper"]);
    for (first, second) in [(&upper, &lower), (&lower, &upper)] {
        let result = project(&[(Standard, first), (Standard, second), (Standard, caller)]);
        assert_eq!(result.calls(2, "Main"), ["HELPER"]);
    }
}

#[test]
fn a_procedure_that_refers_to_itself_calls_nothing_and_is_not_used_by_it() {
    let result = standard(
        r#"
Private Function Fact(ByVal n As Long) As Long
    If n <= 1 Then
        Fact = 1
    Else
        Fact = n * Fact(n - 1)
    End If
End Function
"#,
    );
    assert_eq!(result.unused(), ["0:Fact"]);
    assert_eq!(result.calls(0, "Fact"), [] as [&str; 0]);
}

#[test]
fn a_name_after_a_dot_reaches_the_procedure_of_another_module_with_the_name_of_the_caller() {
    // `Total` of the first module refers to itself by its bare name, which is
    // no call, and to `o.Total`, which may be the `Total` of the second module.
    let first = "Public Function Total() As Long\n    Total = o.Total + Total\nEnd Function\n";
    let second = "Public Function Total() As Long\nEnd Function\n";
    let result = project(&[(Standard, first), (Standard, second)]);
    assert_eq!(result.calls(0, "Total"), ["Total"]);
    assert_eq!(result.calls(1, "Total"), [] as [&str; 0]);

    // With no other `Total`, the same text calls nothing.
    let alone = project(&[(Standard, first)]);
    assert_eq!(alone.calls(0, "Total"), [] as [&str; 0]);
}

#[test]
fn procedures_that_call_each_other_use_each_other() {
    // The graph tells whether anything refers to a procedure, not whether
    // anything reaches it from an entry point.
    let result = standard(
        r#"
Private Sub Ping()
    Pong
End Sub
Private Sub Pong()
    Ping
End Sub
Private Sub Dead()
    Live
End Sub
Private Sub Live()
End Sub
"#,
    );
    assert_eq!(result.unused(), ["0:Dead"]);
    assert_eq!(result.calls(0, "Ping"), ["Pong"]);
    assert_eq!(result.calls(0, "Pong"), ["Ping"]);
    assert_eq!(result.calls(0, "Dead"), ["Live"]);
}

#[test]
fn the_procedures_of_a_module_that_share_a_name_are_one_group() {
    let property = r#"
Private mValue As Long
Public Property Get Value() As Long
    Value = mValue
End Property
Public Property Let Value(ByVal v As Long)
    mValue = v
End Property
"#;
    // Neither refers to the other, and the name they use for their value is
    // their own.
    let alone = project(&[(Class, property)]);
    assert_eq!(alone.unused(), ["0:Value", "0:Value"]);
    assert_eq!(alone.calls(0, "Value"), [] as [&str; 0]);
    // Another module that sets the property uses both.
    let used = project(&[
        (Class, property),
        (Standard, "Sub Main()\n    o.Value = 1\nEnd Sub\n"),
    ]);
    assert_eq!(used.unused(), [] as [&str; 0]);
    assert_eq!(used.calls(1, "Main"), ["Value"]);
}

#[test]
fn the_order_of_the_modules_does_not_change_what_the_graph_says() {
    let caller = "Sub Main()\n    Helper\n    o.Other\nEnd Sub\n";
    let callee =
        "Public Sub Helper(ByVal x As Long)\nEnd Sub\nPublic Sub Other(ByVal x)\nEnd Sub\n";
    for modules in [
        [(Standard, caller), (Standard, callee)],
        [(Standard, callee), (Standard, caller)],
    ] {
        let result = project(&modules);
        assert_eq!(result.unused(), [] as [&str; 0]);
        let main = modules.iter().position(|&(_, text)| text == caller);
        assert_eq!(
            result.calls(main.expect("the caller"), "Main"),
            ["Helper", "Other"]
        );
    }
}

#[test]
fn the_statements_of_two_procedures_may_alternate() {
    let text = r#"
Sub Main()
    First
    Second
End Sub
Sub Other()
    o.Third
    Fourth
End Sub
"#;
    let names = ["First", "Second", "Third", "Fourth"];
    let module = Module::read(Standard, &format!("{text}{}", empties(&names)));
    // The statements to read, as the procedure and the position among its own.
    let run = |order: &[(usize, usize)]| {
        let mut graph = CallGraph::new();
        let id = graph.add_module(Standard, &module.procedures, &[]);
        for &(procedure, at) in order {
            let (line, statement) = &module.statements[procedure][at];
            graph.read(id, procedure, *line, statement);
        }
        graph.resolve()
    };
    let lengths = [module.statements[0].len(), module.statements[1].len()];
    let mut sequential = Vec::new();
    let mut alternating = Vec::new();
    for (procedure, &length) in lengths.iter().enumerate() {
        sequential.extend((0..length).map(|at| (procedure, at)));
    }
    for at in 0..lengths[0].max(lengths[1]) {
        for (procedure, &length) in lengths.iter().enumerate() {
            if at < length {
                alternating.push((procedure, at));
            }
        }
    }
    assert_ne!(sequential, alternating);
    let expected = run(&sequential);
    assert_eq!(run(&alternating), expected);
    let main = &expected[0][0];
    assert_eq!(
        main.calls.as_deref(),
        Some(&["First".to_owned(), "Second".to_owned()][..])
    );
    let other = &expected[0][1];
    assert_eq!(
        other.calls.as_deref(),
        Some(&["Fourth".to_owned(), "Third".to_owned()][..])
    );
}

#[test]
fn a_module_or_a_procedure_that_the_graph_does_not_know_is_ignored() {
    let main = Module::read(Standard, "Sub Main()\n    Helper\nEnd Sub\n");
    let mut other = CallGraph::new();
    other.add_module(Standard, &main.procedures, &[]);
    let foreign = other.add_module(Standard, &main.procedures, &[]);
    let mut graph = CallGraph::new();
    let module = graph.add_module(Standard, &main.procedures, &[]);
    // The graph that numbered `foreign` has two modules, this one has one.
    let (line, statement) = &main.statements[0][1];
    graph.read(foreign, 0, *line, statement);
    graph.read(module, 1, *line, statement);
    graph.read(module, usize::MAX, *line, statement);
    let links = graph.resolve();
    assert_eq!(links.len(), 1);
    assert_eq!(links[0][0].calls, Some(Vec::new()));
}

#[test]
fn the_header_of_a_procedure_refers_to_nothing() {
    // The header names the procedure, its parameters and their types. The
    // `Helper` of another module is not used for it.
    let text = "Private Sub Helper(ByVal Other As Long)\nEnd Sub\n";
    let own = Module::read(Class, text);
    let mut graph = CallGraph::new();
    let module = graph.add_module(Class, &own.procedures, &[]);
    let (line, header) = &own.statements[0][0];
    assert_eq!(*line, own.procedures[0].lines.start);
    assert!(header.members.iter().any(|member| member.name == "Helper"));
    graph.read(module, 0, *line, header);

    let text = "Private Sub Helper()\nEnd Sub\nPrivate Sub Other()\nEnd Sub\n";
    graph.add_module(Class, &Module::read(Class, text).procedures, &[]);
    let unused: Vec<_> = (graph.resolve().iter().flatten())
        .map(|links| links.possibly_unused)
        .collect();
    assert_eq!(unused, [Some(true); 3]);
}

#[test]
fn a_graph_with_no_procedure_has_nothing_to_say() {
    assert!(CallGraph::new().resolve().is_empty());
    let mut graph = CallGraph::new();
    graph.add_module(Standard, &[], &[]);
    graph.add_module(Class, &[], &[]);
    assert_eq!(graph.resolve(), vec![Vec::new(), Vec::new()]);
}

#[test]
fn a_procedure_that_the_parser_could_not_name_is_unused() {
    let module = Module::read(Class, "Sub ()\nEnd Sub\nPrivate Sub Named()\nEnd Sub\n");
    assert_eq!(module.procedures[0].name, "");
    let mut graph = CallGraph::new();
    graph.add_module(Class, &module.procedures, &[]);
    let links = graph.resolve();
    let unused: Vec<_> = links[0].iter().map(|links| links.possibly_unused).collect();
    assert_eq!(unused, [Some(true), Some(true)]);
}

#[test]
fn a_module_that_was_not_read_keeps_its_place_and_leaves_the_calls_unknown() {
    let main = Module::read(Standard, "Sub Main()\n    Helper\nEnd Sub\n");
    let helper = Module::read(Standard, "Sub Helper()\nEnd Sub\n");
    let mut graph = CallGraph::new();
    main.add_to(&mut graph, &[]);
    // A module with no source, or one that the caller refuses.
    graph.skip_module();
    helper.add_to(&mut graph, &[]);

    let links = graph.resolve();

    let sizes: Vec<_> = links.iter().map(Vec::len).collect();
    assert_eq!(sizes, [1, 0, 1]);
    // What `Main` calls, and what calls `Helper`, may be in that module. The host
    // may run both, whatever was read.
    for links in links.iter().flatten() {
        assert_eq!(links.calls, None);
        assert_eq!(links.possibly_unused, None);
        assert!(links.is_entry_point);
    }
}

#[test]
fn the_statements_of_a_module_that_was_not_read_are_ignored() {
    let main = Module::read(Standard, "Sub Main()\n    Helper\nEnd Sub\n");
    let mut graph = CallGraph::new();
    let skipped = graph.skip_module();
    let (line, statement) = &main.statements[0][1];

    graph.read(skipped, 0, *line, statement);

    assert_eq!(graph.resolve(), vec![Vec::new()]);
}
