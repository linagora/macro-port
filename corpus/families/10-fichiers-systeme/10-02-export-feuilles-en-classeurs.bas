' Source: https://github.com/Alrowithy/vba-code/blob/703962956c17d25ed102c4f2771d078cd219c110/WB%20%26%20WS/CopyEachWSToNewWB.bas
' Author: Marius Dragan
' License: MIT
' Retrieved: 2026-10-17
' Family: 10-fichiers-systeme
'
Attribute VB_Name = "CopyEachWSToNewWB"
Option Explicit
Sub CopyEachWSToNewWB()

Dim xPath As String
Dim xWs As Worksheet
Dim questionBoxPopUp As VbMsgBoxResult

 questionBoxPopUp = MsgBox("Are you sure you want to copy each worksheets as a new workbook in the current folder?", vbQuestion + vbYesNo + vbDefaultButton1, "Copy Worksheets?")
    If questionBoxPopUp = vbNo Then Exit Sub

On Error GoTo ErrorHandler
xPath = Application.ActiveWorkbook.path

Application.ScreenUpdating = False
Application.DisplayAlerts = False

For Each xWs In ActiveWorkbook.Sheets
    xWs.Copy
    Application.ActiveWorkbook.SaveAs fileName:=xPath & "\" & xWs.Name & ".xlsx"
    Application.ActiveWorkbook.Close False
Next xWs

Application.DisplayAlerts = True
Application.ScreenUpdating = True

MsgBox "Process completed!", vbInformation

 Exit Sub '<--- exit here if no error occured
ErrorHandler:
Application.DisplayAlerts = True
    Application.ScreenUpdating = True
    Debug.Print Err.Number; Err.Description
    MsgBox "Sorry, an error occured." & vbNewLine & vbNewLine & "Please print screen with the error message together with step by step commands that triggered the error to the developer in order to fix it." & vbNewLine & vbCrLf & Err.Number & " " & Err.Description, vbCritical, "Error!"

End Sub
