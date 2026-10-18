Attribute VB_Name = "M_04_04"
Sub RunCompare()
Dim s1 As String
Dim s2 As String
Dim ee As Integer

'Asking the user to input the two sheets to compare
s1 = InputBox("Enter the most recent sheet name")
If s1 = "" Then Exit Sub
s2 = InputBox("Enter the prior sheet name")
If s2 = "" Then Exit Sub
' This is the unique primary key that ties the two workbooks
ee = InputBox("What Column # is the Employee ID on?" & vbNewLine & "Input the Column Number, not Letter!")
If ee = 0 Then Exit Sub

'sample of what values s1, s2, and ee are
's1 = "6_9_2015"
's2 = "6_8_2015"
'ee = 1

'Compare the two sheets using the sub below
Call compareSheets(s2, s1, ee)

End Sub

Sub compareSheets(sht_old As String, sht_new As String, ee_col As Integer)

'count of the # of differences in the row
Dim row_diffs As Integer

' Last Row # so Excel knows when to stop
Dim lastRow As Integer
' Last Column # so Excel knows when to stop
Dim lastCol As Integer
' Current Row
Dim curr_row As Integer
' Current Column
Dim curr_col As Integer
' Use this to find the column number to look up in the old sheet


Dim ref_range As Range
Dim lookup_col As Integer
Dim ref_Col As Integer

Dim new_info As Range
Dim old_info As Range

Set new_info = ActiveWorkbook.Sheets(sht_new).UsedRange
Set old_info = ActiveWorkbook.Sheets(sht_old).UsedRange

' These two lines find tells excel what is the last row and column #.
lastRow = new_info.Rows.Count
lastCol = new_info.Columns.Count
ref_Col = old_info.Columns.Count

Set ref_range = old_info.Range(ActiveWorkbook.Sheets(sht_old).Cells(2, 1), _
ActiveWorkbook.Sheets(sht_old).Cells(2, ref_Col))

' starts at row 3, change if necessary
For curr_row = 3 To lastRow

    row_diffs = 0

    For curr_col = 1 To lastCol

    On Error Resume Next
'   Test case to make sure the correct row and column is being selected
'   MsgBox (new_info.Cells(2, curr_col).Value)

'   Could be more efficient to implement a native match function
    lookup_col = WorksheetFunction.Match(new_info.Cells(2, curr_col), ref_range, 0)

'    MsgBox (lookup_col)

    If Not WorksheetFunction.Index(old_info.Columns(lookup_col), _
    WorksheetFunction.Match(new_info.Cells(curr_row, ee_col), old_info.Columns(ee_col), 0)) _
    = new_info.Cells(curr_row, curr_col) Then _

    new_info.Cells(curr_row, curr_col).Interior.Color = vbYellow
    row_diffs = row_diffs + 1

    End If

    lookup_col = 1

    Next curr_col

    ActiveWorkbook.Sheets(sht_new).Cells(curr_row, lastCol + 1).Value = row_diffs

Next curr_row

ActiveWorkbook.Sheets(sht_new).Select

End Sub
