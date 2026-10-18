' Source: https://github.com/guillermodean/Excel_Macros/blob/25729a05742bdd8f1721782c2e2c659dc933c9bd/WhenACellChangesValue.cls
' Author: Guillermo Deán
' License: MIT
' Retrieved: 2026-10-17
' Family: 09-evenements
'
'this code should be written in the main sheet not in a module

Private Sub Worksheet_Change(ByVal Target As Range)

' Auto Date

Dim Cell As Range
For Each Cell In Target
' I define beolw the column where I want to look for the change
    If Cell.Column = Range("AC:AC").Column Then
    'If the value of the cell is Accepted or rejected then i change the color and other cells values
        If Cell.Value = "Accepted" Or Cell.Value = "Rejected" Then
            If Cells(Cell.Row, "AD").Value = "" Then
                Cells(Cell.Row, "AD").Value = Now
            Else
                Cells(Cell.Row, "AC").Interior.ColorIndex = 27
                Cells(Cell.Row, "AD").Interior.ColorIndex = 27
                Cells(Cell.Row, "AD").Value = Now
            End If
            End If
        Else
        End If
    Else
    End If
Next Cell
End Sub
