Attribute VB_Name = "M_01_03"
Sub PercentToggle()
' PercentToggle Macro
' Shortcut Key: E

Dim Dash As Boolean
Dim OneWith As String
Dim TwoWith As String
'Dim ThreeWith As String
Dim OneWithout As String
Dim TwoWithout As String
'Dim ThreeWithout As String

OneWith = "#,##0%_);(#,##0%);--_%_);@_)"
TwoWith = "#,##0.0%_);(#,##0.0%);--_%_);@_)"
'ThreeWith = "#,##0.00%_);(#,##0.00%);--_%_);@_%_)"

OneWithout = "#,##0%_);(#,##0%);#,##0%_);@_)"
TwoWithout = "#,##0.0%_);(#,##0.0%);#,##0.0%_);@_)"
'ThreeWithout = "#,##0.00%_);(#,##0.00%);#,##0.00%_);@_%_)"

If ThisWorkbook.Sheets(1).Range("DashToggle").Value = 1 Then
    Dash = True
Else
    Dash = False
End If

If Dash = True Then
    If Selection.NumberFormat = TwoWith Or Selection.NumberFormat = TwoWithout Then
        Selection.NumberFormat = OneWith
    'ElseIf Selection.NumberFormat = OneWith Or Selection.NumberFormat = OneWithout Then
    '    Selection.NumberFormat = ThreeWith
    'ElseIf Selection.NumberFormat = ThreeWith Or Selection.NumberFormat = ThreeWithout Then
    '    Selection.NumberFormat = TwoWith
    Else: Selection.NumberFormat = TwoWith
    End If

End If

If Dash = False Then
    If Selection.NumberFormat = TwoWith Or Selection.NumberFormat = TwoWithout Then
        Selection.NumberFormat = OneWithout
    'ElseIf Selection.NumberFormat = OneWith Or Selection.NumberFormat = OneWithout Then
    '    Selection.NumberFormat = ThreeWithout
    'ElseIf Selection.NumberFormat = ThreeWith Or Selection.NumberFormat = ThreeWithout Then
    '    Selection.NumberFormat = TwoWithout
    Else: Selection.NumberFormat = TwoWithout
    End If

End If

End Sub
