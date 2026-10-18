' Source: https://github.com/MicrosoftDocs/VBA-Docs/blob/6765796079a1552460769cb633068d387594dc99/excel/Concepts/Cells-and-Ranges/highlight-the-active-cell-row-or-column.md
' Author: Microsoft Corporation (MicrosoftDocs/VBA-Docs)
' License: MIT
' Retrieved: 2026-10-17
' Family: 09-evenements
'
Private Sub Worksheet_SelectionChange(ByVal Target As Range)
    If Target.Cells.Count > 1 Then Exit Sub
    Application.ScreenUpdating = False
    ' Clear the color of all the cells
    Cells.Interior.ColorIndex = 0
    With Target
        ' Highlight the entire row and column that contain the active cell
        .EntireRow.Interior.ColorIndex = 8
        .EntireColumn.Interior.ColorIndex = 8
    End With
    Application.ScreenUpdating = True
End Sub
