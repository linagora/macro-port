VERSION 1.0 CLASS
BEGIN
  MultiUse = -1  'True
END
Attribute VB_Name = "Sheet1"
Attribute VB_GlobalNameSpace = False
Attribute VB_Creatable = False
Attribute VB_PredeclaredId = True
Attribute VB_Exposed = True
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

