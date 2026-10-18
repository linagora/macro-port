' Source: https://github.com/MicrosoftDocs/VBA-Docs/blob/6765796079a1552460769cb633068d387594dc99/api/Excel.Workbook.BeforeSave.md
' Author: Microsoft Corporation (MicrosoftDocs/VBA-Docs)
' License: MIT
' Retrieved: 2026-10-17
' Family: 09-evenements
'
Private Sub Workbook_BeforeSave(ByVal SaveAsUI As Boolean, Cancel As Boolean)
   'If the six specified cells don't contain data, then display a message box with an error
   'and cancel the attempt to save.
   If WorksheetFunction.CountA(Worksheets("Sheet1").Range("D5,D7,D9,D11,D13,D15")) < 6 Then
      MsgBox "Workbook will not be saved unless" & vbCrLf & _
      "All required fields have been filled in!"
      Cancel = True
   End If
End Sub
