// SPDX-License-Identifier: Apache-2.0
//! The structural predicate `udf_candidate` (SPEC section 7): which functions
//! qualify, and one test for each thing that a function must not do.

// clippy.toml excuses `expect` in the `#[test]` functions only, not in helpers.
#![allow(clippy::expect_used)]

use macroport_core::rules::ModuleType;
use macroport_core::vba::{is_udf_candidate, parse, split_lines};

/// Whether each procedure of `text`, in order, is a UDF candidate when the
/// module has this type.
fn candidates(module_type: ModuleType, text: &str) -> Vec<bool> {
    let lines = split_lines(text).expect("lines");
    (parse(&lines).procedures.iter())
        .map(|procedure| is_udf_candidate(module_type, procedure, &lines))
        .collect()
}

/// Whether a public function of a standard module with these statements is a
/// UDF candidate.
fn is_candidate(statements: &str) -> bool {
    let text = format!("Public Function F(r As Range)\n{statements}\nEnd Function\n");
    let found = candidates(ModuleType::Standard, &text);
    assert_eq!(found.len(), 1, "{text}");
    found.first().copied().expect("a procedure")
}

/// Fails for the first body that is not a UDF candidate.
fn assert_candidates(bodies: &[&str]) {
    for statements in bodies {
        assert!(
            is_candidate(statements),
            "should be a candidate: {statements:?}"
        );
    }
}

/// Fails for the first body that is a UDF candidate.
fn assert_not_candidates(bodies: &[&str]) {
    for statements in bodies {
        assert!(
            !is_candidate(statements),
            "should not be one: {statements:?}"
        );
    }
}

// Which procedures can be candidates.

#[test]
fn a_public_or_default_function_of_a_standard_module_is_a_candidate() {
    for text in [
        "Function F()\nEnd Function",
        "Public Function F()\nEnd Function",
        "Public Static Function F(ByVal x As Long) As Long\nF = x\nEnd Function",
        // The names of the parameters are not statements.
        "Function F(Value As Long, Optional Font As Long = 3)\nEnd Function",
    ] {
        assert_eq!(candidates(ModuleType::Standard, text), [true], "{text}");
    }
}

#[test]
fn only_a_public_or_default_function_can_be_one() {
    for text in [
        "Private Function F()\nEnd Function",
        "Friend Function F()\nEnd Function",
        "Sub S()\nEnd Sub",
        "Public Sub S()\nEnd Sub",
        "Property Get P()\nEnd Property",
        "Property Let P(v)\nEnd Property",
        "Property Set P(v)\nEnd Property",
    ] {
        assert_eq!(candidates(ModuleType::Standard, text), [false], "{text}");
    }
}

#[test]
fn only_a_function_of_a_standard_module_can_be_one() {
    let text = "Public Function F()\nF = 1\nEnd Function";
    for module_type in [ModuleType::Class, ModuleType::Document, ModuleType::Form] {
        assert_eq!(candidates(module_type, text), [false], "{module_type:?}");
    }
}

#[test]
fn each_function_is_judged_on_its_own_lines() {
    let text = "\
Public Function A(r As Range)
    A = r.Value
End Function
Public Function B(r As Range)
    r.Value = 1
End Function
Public Sub C()
End Sub
Public Function D()
    D = 1
End Function
";
    assert_eq!(
        candidates(ModuleType::Standard, text),
        [true, false, false, true]
    );
}

#[test]
fn a_function_that_is_not_closed_is_judged_on_the_lines_it_has() {
    let clean = "Public Function F()\nF = 1\n";
    assert_eq!(candidates(ModuleType::Standard, clean), [true]);
    let dirty = "Public Function F(r As Range)\nr.Select\n";
    assert_eq!(candidates(ModuleType::Standard, dirty), [false]);
}

// The methods.

#[test]
fn a_method_that_changes_the_workbook_is_a_side_effect() {
    assert_not_candidates(&[
        "r.Select",
        "Range(\"A1\").Select",
        "Selection.Copy Destination:=Range(\"B1\")",
        "ws.Activate",
        "r.Delete",
        "r.Insert Shift:=xlDown",
        "r.ClearContents",
        "r.Parent.Parent.Activate",
        "x = r.Delete",
        "Call r.Select",
        "If a Then r.Select",
        "With r\n.Select\nEnd With",
        // In whatever case.
        "r.CLEARCONTENTS",
        "r.select",
    ]);
}

#[test]
fn a_name_that_is_not_such_a_method_is_no_side_effect() {
    assert_candidates(&[
        // `Select Case` is a statement, not the method.
        "Select Case r.Row\nCase 1\nx = 2\nEnd Select",
        // Another name that starts or ends like one.
        "x = r.Selected",
        "x = r.CopyFromRecordset",
        "x = r.Deleted",
        // A name that is no member, and the name of an argument.
        "Deleted = 1",
        "Copy = 1",
        "Delete x",
        "Foo Delete:=True",
        // A name after a `!` is the item of a collection, not a method.
        "rs!Delete",
        "x = rs!Delete",
        // Text.
        "x = \"r.Select\"",
        "' r.Select",
        "Rem r.Select",
        "x = 1 ' r.Delete",
    ]);
}

// `MsgBox`.

#[test]
fn a_message_box_is_a_side_effect() {
    assert_not_candidates(&[
        "MsgBox \"hi\"",
        "MsgBox(\"hi\")",
        "x = MsgBox(\"a\", vbYesNo)",
        "Call MsgBox(\"a\")",
        "If a Then MsgBox \"a\"",
        // After a `.`, and in whatever case.
        "VBA.MsgBox \"a\"",
        "msgbox \"a\"",
    ]);
}

#[test]
fn a_name_that_is_not_msgbox_is_no_side_effect() {
    assert_candidates(&[
        "MyMsgBox \"a\"",
        "x = MsgBoxResult",
        "Foo MsgBox:=1",
        "x = \"MsgBox\"",
        "' MsgBox \"a\"",
    ]);
}

// The assignments.

#[test]
fn an_assignment_to_a_listed_member_is_a_side_effect() {
    assert_not_candidates(&[
        "r.Value = 1",
        "Range(\"A1\").Value = 1",
        "Cells(1, 1).Value = 1",
        "Worksheets(\"a\").Range(\"A1:B2\").Value = x",
        "r.Formula = \"=1\"",
        "r.Interior.Color = 3",
        "r.Font.Bold = True",
        "r.Cells(1, 1).Font.Size = 8",
        "Let r.Value = 1",
        // In whatever case, with arguments, or in brackets.
        "r.value = 1",
        "r.Value(xlRangeValueDefault) = x",
        "r.[Value] = 1",
        // On the line that continues the target.
        "Range(\"A1\") _\n    .Value = 1",
    ]);
}

#[test]
fn an_assignment_in_a_single_line_if_is_a_side_effect() {
    assert_not_candidates(&[
        "If a Then r.Value = 1",
        "If a Then x = 1 Else r.Value = 2",
        "If a Then Else r.Value = 1",
        "If a Then\nx = 1\nElseIf b Then r.Value = 2\nEnd If",
    ]);
}

#[test]
fn an_assignment_after_a_colon_or_in_a_case_is_a_side_effect() {
    assert_not_candidates(&[
        "x = 1: r.Value = 2",
        "Select Case x\nCase 1: r.Value = 1\nEnd Select",
        "Select Case x\nCase 1\ny = 1\nCase Else\nr.Value = 2\nEnd Select",
    ]);
}

#[test]
fn a_read_a_comparison_and_an_assignment_to_another_target_are_no_side_effect() {
    assert_candidates(&[
        // Reads.
        "x = r.Value",
        "x = Range(\"A1\").Formula",
        "y = r.Font.Bold",
        "F = r.Interior.Color",
        // Comparisons.
        "If r.Value = 1 Then x = 2",
        "If r.Value = 1 And r.Font.Bold = True Then x = 1",
        "Do While r.Value <> 0\nLoop",
        "While r.Value = 1\nWend",
        "Select Case r.Value\nEnd Select",
        "Debug.Print r.Value = 1",
        "Foo r.Value = 1",
        // Assignments to variables, to the function and to other members.
        "x = 1",
        "F = 1",
        "Set s = r",
        "r.NumberFormat = \"0\"",
        "Application.ScreenUpdating = False",
        "Mid(s, 1, 2) = \"ab\"",
        // A variable that is named like a member, and an item of a collection.
        "Value = 3",
        "Font = 1",
        "rs!Value = 1",
    ]);
}

#[test]
fn a_dot_after_a_space_does_not_continue_the_target() {
    // It starts the first argument of a call, which is a comparison.
    assert_candidates(&[
        "list.Add .Value = 1",
        "Foo .Font.Bold = True",
        "With r\nlist.Add .Value = 1\nEnd With",
    ]);
}

#[test]
fn the_arguments_of_a_target_are_not_its_members() {
    assert_candidates(&[
        "a(r.Value) = 1",
        "Cells(r.Value, 1).Name = \"x\"",
        "Cells(r.Font.Size, 1).Name = \"x\"",
    ]);
}

#[test]
fn a_listed_name_in_a_string_or_a_comment_is_no_side_effect() {
    assert_candidates(&[
        "x = \"r.Value = 1\"",
        "' r.Value = 1",
        "Rem r.Value = 1",
        "x = 1 ' r.Value = 2",
        // A `Rem` after `Then` or `Else` is a comment as well.
        "If a Then Rem r.Value = 1",
        "If a Then x = 1 Else Rem r.Value = 2",
    ]);
}

#[test]
fn what_the_spec_does_not_list_is_no_side_effect() {
    // The list is closed: these write to the workbook, and are still candidates.
    assert_candidates(&[
        "Cells(1, 1) = 1",
        "r.Value2 = 1",
        "r.FormulaR1C1 = \"x\"",
        "ThisWorkbook.Names.Add Name:=\"x\", RefersTo:=\"=A1\"",
        // A font kept in a variable.
        "Set f = r.Font\nf.Bold = True",
        "Set f = r.Font\nWith f\n.Bold = True\nEnd With",
    ]);
}

// The `With` blocks.

#[test]
fn an_assignment_in_a_with_to_a_listed_member_is_a_side_effect() {
    assert_not_candidates(&[
        "With r\n.Value = 1\nEnd With",
        "With r\n.Font.Bold = True\nEnd With",
        // The target of the block is the listed member.
        "With r.Font\n.Bold = True\nEnd With",
        "With r.Interior\n.Color = 3\nEnd With",
        "With Worksheets(1).Range(\"A1\").Interior\n.Pattern = xlSolid\nEnd With",
        // A block inside it, relative to it.
        "With r\nWith .Font\n.Bold = True\nEnd With\nEnd With",
        "With r.Font\nWith .Sub\n.Bold = True\nEnd With\nEnd With",
    ]);
}

#[test]
fn a_with_block_ends_at_its_end_with() {
    assert_candidates(&[
        "With r\nx = .Value\n.NumberFormat = \"0\"\nEnd With",
        "With r.Font\nx = .Bold\nEnd With",
        // After the block, the target is no longer the font.
        "With r.Font\nEnd With\ns.Bold = True",
        "With r.Font\nEnd With\n.Bold = True",
        // A block inside it that is not relative to it.
        "With r.Font\nWith s\n.Bold = True\nEnd With\nEnd With",
        // The arguments of the target are not its members.
        "With Cells(r.Font.Size, 1)\n.Name = \"x\"\nEnd With",
        // A block that is not opened, and one that is not closed.
        "End With\nEnd With\nx = 1",
        "With r.Font\nx = .Bold",
        // An item of a collection, in a block.
        "With rs\n!Value = 1\nEnd With",
    ]);
}
