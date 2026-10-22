// SPDX-License-Identifier: Apache-2.0
//! The procedures that the host starts and those that nothing calls (SPEC M2
//! item 5 and section 7, ADR-0012): the front end tells the call graph which
//! procedures the host starts, by the predicates of the rule engine, and reads
//! `is_entry_point` and `possibly_unused` back from it.

use macroport_core::report::{FileReport, ProcedureReport};
use macroport_core::rules::ModuleType;
use macroport_core::vba::MAX_LINES;

use crate::support::{codes, procedures, report_of_modules, source};

/// What the report says of a procedure: its name, `is_entry_point`,
/// `is_event_handler` and `possibly_unused`.
type Flags<'a> = (&'a str, Option<bool>, Option<bool>, Option<bool>);

/// The report of the modules `(name, type, text)`, in that order.
fn report_of(modules: &[(&str, ModuleType, &str)]) -> FileReport {
    let modules = (modules.iter())
        .map(|&(name, module_type, text)| source(name, module_type, text))
        .collect();
    report_of_modules(modules)
}

/// The flags of every procedure of `file`, in order.
fn flags(file: &FileReport) -> Vec<Flags<'_>> {
    fn of(procedure: &ProcedureReport) -> Flags<'_> {
        (
            procedure.name.as_str(),
            procedure.is_entry_point,
            procedure.is_event_handler,
            procedure.possibly_unused,
        )
    }
    procedures(file).map(of).collect()
}

/// Whether the procedure called `name` has the trait `possibly_unused`.
fn has_the_trait(file: &FileReport, name: &str) -> bool {
    let found = procedures(file).find(|procedure| procedure.name == name);
    let traits = found.and_then(|procedure| procedure.traits.as_ref());
    traits.is_some_and(|traits| traits.iter().any(|id| id == "possibly_unused"))
}

#[test]
fn the_events_of_a_document_and_the_auto_macros_are_started_by_the_host() {
    let document = "\
Sub Workbook_Open()
    Setup
End Sub

Private Sub Setup()
End Sub

Private Sub Spare()
End Sub
";
    let standard = "Sub Auto_Open()\nEnd Sub\n";

    let file = report_of(&[
        ("ThisWorkbook", ModuleType::Document, document),
        ("Startup", ModuleType::Standard, standard),
    ]);

    // `Workbook_Open` is an event handler and an entry point. `Setup` is called
    // from it. `Spare` is called from nowhere. `Auto_Open` is an entry point and
    // no event handler: SPEC section 7 gives `is_event_handler` to the
    // `event_handler` predicate only.
    let expected = [
        ("Workbook_Open", Some(true), Some(true), Some(false)),
        ("Setup", Some(false), Some(false), Some(false)),
        ("Spare", Some(false), Some(false), Some(true)),
        ("Auto_Open", Some(true), Some(false), Some(false)),
    ];
    assert_eq!(flags(&file), expected);
    assert_eq!(codes(&file), Vec::<&str>::new());
}

#[test]
fn a_procedure_that_nothing_calls_is_possibly_unused_and_has_the_trait() {
    let text = "\
Public Sub Start()
    Used
End Sub

Private Sub Used()
End Sub

Private Sub Unused()
End Sub
";

    let file = report_of(&[("Module1", ModuleType::Standard, text)]);

    let expected = [
        ("Start", Some(true), Some(false), Some(false)),
        ("Used", Some(false), Some(false), Some(false)),
        ("Unused", Some(false), Some(false), Some(true)),
    ];
    assert_eq!(flags(&file), expected);
    // The rule engine reads the predicate back from the graph: the trait is on
    // the procedure that nothing calls and on no other.
    assert!(has_the_trait(&file, "Unused"));
    assert!(!has_the_trait(&file, "Used"));
    assert!(!has_the_trait(&file, "Start"));
}

#[test]
fn a_public_procedure_of_a_standard_module_is_an_entry_point_when_a_button_can_run_it() {
    let text = "\
Public Sub Button()
End Sub

Public Sub WithParameter(n As Long)
End Sub

Public Function Udf(n As Long) As Long
End Function

Private Sub Hidden()
End Sub
";
    let class = "Public Sub Method()\nEnd Sub\n";

    let file = report_of(&[
        ("Module1", ModuleType::Standard, text),
        ("Class1", ModuleType::Class, class),
    ]);

    // A button runs a procedure with no parameter, a cell calls a function. A
    // public procedure of a class is a method and is run by none of them.
    let expected = [
        ("Button", Some(true), Some(false), Some(false)),
        ("WithParameter", Some(false), Some(false), Some(true)),
        ("Udf", Some(true), Some(false), Some(false)),
        ("Hidden", Some(false), Some(false), Some(true)),
        ("Method", Some(false), Some(false), Some(true)),
    ];
    assert_eq!(flags(&file), expected);
}

#[test]
fn a_handler_of_a_variable_with_events_is_started_by_the_host_in_a_class_module() {
    let class = "\
Private WithEvents App As Application

Private Sub App_NewWorkbook(ByVal Wb As Workbook)
End Sub

Private Sub App_Gone()
End Sub

Private Sub Spare()
End Sub
";

    let file = report_of(&[("Watcher", ModuleType::Class, class)]);

    // `App_NewWorkbook` and `App_Gone` are named after the variable, which the
    // module declares with events.
    let expected = [
        ("App_NewWorkbook", Some(true), Some(true), Some(false)),
        ("App_Gone", Some(true), Some(true), Some(false)),
        ("Spare", Some(false), Some(false), Some(true)),
    ];
    assert_eq!(flags(&file), expected);
}

#[test]
fn a_procedure_that_may_handle_the_event_of_a_control_is_left_out_unless_a_call_shows_it_is_used() {
    let form = "\
Private Sub UserForm_Initialize()
    Fill
End Sub

Private Sub Button1_Click()
    Fill
End Sub

Private Sub Fill()
    Load_Data
End Sub

Private Sub Load_Data()
End Sub

Private Sub Spare()
End Sub

Private Sub Spare_Part()
End Sub
";

    let file = report_of(&[("UserForm1", ModuleType::Form, form)]);

    // The designer is not read, so the controls of the form are not known:
    // `Button1_Click` may handle the click of a button called `Button1`, which
    // the engine cannot match, and so may `Spare_Part`. Nothing can tell whether
    // the host starts them, so the fields are left out for these two and not made
    // up. `Load_Data` has the same form but `Fill` calls it, and what is called
    // is used whoever starts it: `possibly_unused` is false, whereas it is not
    // known if it is an entry point. `Spare` has no underscore and is unused.
    let expected = [
        ("UserForm_Initialize", Some(true), Some(true), Some(false)),
        ("Button1_Click", None, None, None),
        ("Fill", Some(false), Some(false), Some(false)),
        ("Load_Data", None, None, Some(false)),
        ("Spare", Some(false), Some(false), Some(true)),
        ("Spare_Part", None, None, None),
    ];
    assert_eq!(flags(&file), expected);
    // The trait follows `possibly_unused`, and the calls are known all the same.
    assert!(has_the_trait(&file, "Spare"));
    assert!(!has_the_trait(&file, "Spare_Part") && !has_the_trait(&file, "Load_Data"));
    let called = procedures(&file).find(|procedure| procedure.name == "Button1_Click");
    assert_eq!(
        called.and_then(|procedure| procedure.calls.clone()),
        Some(vec!["Fill".to_owned()])
    );
}

#[test]
fn a_standard_module_has_no_events_and_its_workbook_open_is_an_ordinary_procedure() {
    let text = "Private Sub Workbook_Open()\nEnd Sub\n";

    let file = report_of(&[("Module1", ModuleType::Standard, text)]);

    // SPEC section 7: the events of the workbook count in a document module only.
    let expected = [("Workbook_Open", Some(false), Some(false), Some(true))];
    assert_eq!(flags(&file), expected);
}

#[test]
fn a_module_that_cannot_be_read_leaves_possibly_unused_out_and_keeps_the_entry_points() {
    let big = "a\n".repeat(MAX_LINES as usize + 1);
    let first = "Sub A()\n    B\nEnd Sub\n";
    let last = "Public Sub B(n As Long)\nEnd Sub\n\nPrivate Sub C()\nEnd Sub\n";
    let document = "Sub Workbook_Open()\nEnd Sub\n";

    let whole = report_of(&[
        ("First", ModuleType::Standard, first),
        ("Last", ModuleType::Standard, last),
        ("ThisWorkbook", ModuleType::Document, document),
    ]);
    let partial = report_of(&[
        ("First", ModuleType::Standard, first),
        ("Big", ModuleType::Standard, &big),
        ("Last", ModuleType::Standard, last),
        ("ThisWorkbook", ModuleType::Document, document),
    ]);

    // Read in full, `C` is called from nowhere. With `Big` refused, whether it
    // is called is not known, though it is known that `A` and `Workbook_Open` are
    // started, and the trait follows `possibly_unused`: a missing value is no
    // reason for it.
    let expected = [
        ("A", Some(true), Some(false), Some(false)),
        ("B", Some(false), Some(false), Some(false)),
        ("C", Some(false), Some(false), Some(true)),
        ("Workbook_Open", Some(true), Some(true), Some(false)),
    ];
    assert_eq!(flags(&whole), expected);
    assert!(has_the_trait(&whole, "C"));
    let expected = [
        ("A", Some(true), Some(false), None),
        ("B", Some(false), Some(false), None),
        ("C", Some(false), Some(false), None),
        ("Workbook_Open", Some(true), Some(true), None),
    ];
    assert_eq!(flags(&partial), expected);
    assert_eq!(codes(&partial), ["vba.too_many_lines"]);
    assert!(!has_the_trait(&partial, "C"));
}

#[test]
fn a_module_that_the_room_of_the_engine_refuses_leaves_possibly_unused_out_too() {
    // A statement of half a million names takes more than the rule engine may
    // read: the module is refused for that, not for its lines.
    let dense = format!("x = {}a\n", "a.".repeat(250_000));
    let first = "Sub A()\n    B\nEnd Sub\n";
    let last = "Public Sub B(n As Long)\nEnd Sub\n\nPrivate Sub C()\nEnd Sub\n";

    let file = report_of(&[
        ("First", ModuleType::Standard, first),
        ("Dense", ModuleType::Standard, &dense),
        ("Last", ModuleType::Standard, last),
    ]);

    // The module may hold a procedure that calls `C`: as for a module that cannot
    // be read at all, the graph judges none and the entry points stay.
    let expected = [
        ("A", Some(true), Some(false), None),
        ("B", Some(false), Some(false), None),
        ("C", Some(false), Some(false), None),
    ];
    assert_eq!(flags(&file), expected);
    assert_eq!(codes(&file), ["vba.too_much_code"]);
    assert!(!has_the_trait(&file, "C"));
}
