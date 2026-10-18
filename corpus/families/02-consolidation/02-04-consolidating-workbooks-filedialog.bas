' Source: https://github.com/Marius-Dragan/VBA-CODE/blob/703962956c17d25ed102c4f2771d078cd219c110/WB%20&%20WS/ConsolidatingWB.bas
' Author: Marius Dragan
' License: MIT
' Retrieved: 2026-10-17
' Family: 02-consolidation
'
Attribute VB_Name = "ConsolidatingWB"
Option Explicit
Sub ConsolidatingMultipleWb()

Dim CurrentWB As Workbook
Dim ws As Worksheet
'Dim MasterWS As Worksheet
Dim Sheet As Worksheet
Dim IndvFiles As FileDialog
Dim FileIDx As Long
Dim LRow1 As Long
Dim LRow2 As Long
Dim CopyRange As Range
Dim i As Integer
Dim x As Integer
Dim F_Name As String

Set ws = ActiveSheet
'Set MasterWS = ThisWorkbook.Sheets(1)
Set IndvFiles = Application.FileDialog(msoFileDialogOpen)

    With IndvFiles
        .AllowMultiSelect = True
        .Title = "Multi-select target data files:"
        .ButtonName = ""
        .Filters.Clear
        '.Filters.Add ".xlsx files", "*.xlsx"
        If Not .Show Then
            Set IndvFiles = Nothing
            'MsgBox "Please select folder and try again.", vbInformation
            Exit Sub
        End If
    End With

    Application.DisplayAlerts = False
    Application.ScreenUpdating = False

    For FileIDx = 1 To IndvFiles.SelectedItems.Count

        Set CurrentWB = Workbooks.Open(IndvFiles.SelectedItems(FileIDx))

        For Each Sheet In CurrentWB.Sheets
            If LRow1 = 0 Then
                LRow1 = ws.Range("A" & ws.Rows.Count).End(xlUp).Row
            Else
                LRow1 = ws.Range("A" & ws.Rows.Count).End(xlUp).Row + 1
            End If

            LRow2 = ws.Range("A" & ws.Rows.Count).End(xlUp).Row

            'Set CopyRange = CurrentWB.ActiveSheet.Range("A2:z" & LRow2)
            Set CopyRange = CurrentWB.ActiveSheet.UsedRange
            CopyRange.Copy
            ws.Range("A" & LRow1).PasteSpecial Paste:=xlPasteValues, operation:=xlNone, skipblanks:=False, Transpose:=False
            Application.CutCopyMode = False
        Next Sheet
        CurrentWB.Close False
    Next FileIDx

Application.DisplayAlerts = True
Application.ScreenUpdating = True

MsgBox "Process completed!", vbInformation, Title:="Consolidating WB"

End Sub
