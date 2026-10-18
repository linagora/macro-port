' Source: synthetique, corpus vba-migration-corpus (LINAGORA)
' Author: LINAGORA
' License: CC0-1.0
' Retrieved: 2026-10-17
' Family: 14-patterns-securite
' Synthetic: echantillon benin ecrit pour tester la detection, ne fait rien de nuisible
'
' Module ThisWorkbook
Option Explicit

' Motif : Workbook_Open + WScript.Shell avec chaine construite par Chr() et concatenation
Private Sub Workbook_Open()
    Dim o As Object, c As String
    c = Chr(110) & Chr(111) & "te" & "pad" & Chr(46) & StrReverse("exe")
    Set o = CreateObject("WScr" & "ipt.Sh" & "ell")
    o.Run c, 1, False
End Sub
