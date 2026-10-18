' Source: https://github.com/MicrosoftDocs/VBA-Docs/blob/6765796079a1552460769cb633068d387594dc99/excel/Concepts/Cells-and-Ranges/fill-a-value-down-into-blank-cells-in-a-column.md
' Author: Tom Urtis (Microsoft VBA-Docs)
' License: MIT
' Retrieved: 2026-10-17
' Family: 03-nettoyage-transformation
'
Sub FillCellsFromAbove()
    ' Turn off screen updating to improve performance
    Application.ScreenUpdating = False
    On Error Resume Next
    ' Look in column A
    With Columns(1)
        ' For blank cells, set them to equal the cell above
        .SpecialCells(xlCellTypeBlanks).Formula = "=R[-1]C"
        'Convert the formula to a value
        .Value = .Value
    End With
    Err.Clear
    Application.ScreenUpdating = True
End Sub
