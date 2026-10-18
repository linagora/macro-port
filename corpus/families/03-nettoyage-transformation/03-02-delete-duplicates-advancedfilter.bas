' Source: https://github.com/MicrosoftDocs/VBA-Docs/blob/6765796079a1552460769cb633068d387594dc99/excel/Concepts/Cells-and-Ranges/delete-duplicate-entries-in-a-range.md
' Author: Tom Urtis (Microsoft VBA-Docs)
' License: MIT
' Retrieved: 2026-10-17
' Family: 03-nettoyage-transformation
'
Sub DeleteDuplicates()
    With Application
        ' Turn off screen updating to increase performance
        .ScreenUpdating = False
        Dim LastColumn As Integer
        LastColumn = Cells.Find(What:="*", After:=Range("A1"), SearchOrder:=xlByColumns, SearchDirection:=xlPrevious).Column + 1
        With Range("A1:A" & Cells(Rows.Count, 1).End(xlUp).Row)
            ' Use AdvanceFilter to filter unique values
            .AdvancedFilter Action:=xlFilterInPlace, Unique:=True
            .SpecialCells(xlCellTypeVisible).Offset(0, LastColumn - 1).Value = 1
            On Error Resume Next
            ActiveSheet.ShowAllData
            'Delete the blank rows
            Columns(LastColumn).SpecialCells(xlCellTypeBlanks).EntireRow.Delete
            Err.Clear
        End With
        Columns(LastColumn).Clear
        .ScreenUpdating = True
    End With
End Sub
