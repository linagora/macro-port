' Source: https://github.com/MicrosoftDocs/VBA-Docs/blob/6765796079a1552460769cb633068d387594dc99/excel/Concepts/Workbooks-and-Worksheets/add-a-table-of-contents-to-a-workbook.md
' Author: Microsoft Corporation (MicrosoftDocs/VBA-Docs)
' License: MIT
' Retrieved: 2026-10-17
' Family: 06-generation-rapports
'
Option Explicit
Sub Create_TOC()
Dim wbBook As Workbook
Dim wsActive As Worksheet
Dim wsSheet As Worksheet
Dim lnRow As Long
Dim lnPages As Long
Dim lnCount As Long
Set wbBook = ActiveWorkbook
With Application
    .DisplayAlerts = False
    .ScreenUpdating = False
End With
'If the TOC sheet already exist delete it and add a new
'worksheet.
On Error Resume Next
With wbBook
    .Worksheets("TOC").Delete
    .Worksheets.Add Before:=.Worksheets(1)
End With
On Error GoTo 0
Set wsActive = wbBook.ActiveSheet
With wsActive
    .Name = "TOC"
    With .Range("A1:B1")
        .Value = VBA.Array("Table of Contents", "Sheet # - # of Pages")
        .Font.Bold = True
    End With
End With
lnRow = 2
lnCount = 1
'Iterate through the worksheets in the workbook and create
'sheetnames, add hyperlink and count & write the running number
'of pages to be printed for each sheet on the TOC sheet.
For Each wsSheet In wbBook.Worksheets
    If wsSheet.Name <> wsActive.Name Then
        wsSheet.Activate
        With wsActive
            .Hyperlinks.Add .Cells(lnRow, 1), "", _
            SubAddress:="'" & wsSheet.Name & "'!A1", _
            TextToDisplay:=wsSheet.Name
            lnPages = wsSheet.PageSetup.Pages().Count
            .Cells(lnRow, 2).Value = "'" & lnCount & "-" & lnPages
        End With
        lnRow = lnRow + 1
        lnCount = lnCount + 1
    End If
Next wsSheet
wsActive.Activate
wsActive.Columns("A:B").EntireColumn.AutoFit
With Application
    .DisplayAlerts = True
    .ScreenUpdating = True
End With
End Sub
