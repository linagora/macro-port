' Source: https://github.com/Aquafortis/vba-sub-snippets/blob/58f65955ff084471ab8a4c3b541e528672d577f4/Excel/MergeAllSheets.vb
' Author: Aquafortis
' License: MPL-2.0
' Retrieved: 2026-10-17
' Family: 02-consolidation
'
Sub MergeAllSheets()
    ' Merge all sheets into one
    ' Keep top row of first sheet only
    Dim S As Integer
    Dim cRng As Range
    Sheets.Add(Before:=Sheets(1)).Name = "Merged"
    Sheets(2).Range("A1").EntireRow.Copy _
    Sheets(1).Range("A1")
    For S = 2 To Sheets.Count
        Sheets(S).Activate
        Set cRng = ActiveSheet.UsedRange
        cRng.Offset(1, 0).Resize(cRng.Rows.Count - 1, _
        cRng.Columns.Count).Copy _
        Sheets(1).Range("A65536").End(xlUp)(2)
    Next
    Sheets("Merged").Activate
End Sub
