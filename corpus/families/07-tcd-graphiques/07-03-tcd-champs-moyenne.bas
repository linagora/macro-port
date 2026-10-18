' Source: https://github.com/byronwall/bUTL/blob/be303d89ea61c7326a381b91596fc85ce60901f4/src/code/Usability.bas
' Author: Byron Wall (byronwall/bUTL)
' License: MIT
' Retrieved: 2026-10-17
' Family: 07-tcd-graphiques
'
Public Sub PivotSetAllFields()
    '---------------------------------------------------------------------------------------
    ' Procedure : PivotSetAllFields
    ' Author    : @byronwall
    ' Date      : 2015 07 24
    ' Purpose   : Sets all fields in a PivotTable to use a certain calculation type
    ' Flag      : new-feature
    '---------------------------------------------------------------------------------------
    '
    Dim targetTable As PivotTable
    Dim targetSheet As Worksheet

    Set targetSheet = ActiveSheet

    'this information is a bit unclear to me
    MsgBox "This defaults to the average for every Pivot table on the sheet.  Edit code for other result."
    On Error Resume Next
    For Each targetTable In targetSheet.PivotTables
        Dim targetField As PivotField
        For Each targetField In targetTable.DataFields
            targetField.Function = xlAverage
        Next targetField
    Next targetTable

End Sub
