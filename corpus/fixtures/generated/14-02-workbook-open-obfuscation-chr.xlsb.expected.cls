VERSION 1.0 CLASS
BEGIN
  MultiUse = -1  'True
END
Attribute VB_Name = "ThisWorkbook"
Attribute VB_GlobalNameSpace = False
Attribute VB_Creatable = False
Attribute VB_PredeclaredId = True
Attribute VB_Exposed = True
' Module ThisWorkbook
Option Explicit

' Motif : Workbook_Open + WScript.Shell avec chaine construite par Chr() et concatenation
Private Sub Workbook_Open()
    Dim o As Object, c As String
    c = Chr(110) & Chr(111) & "te" & "pad" & Chr(46) & StrReverse("exe")
    Set o = CreateObject("WScr" & "ipt.Sh" & "ell")
    o.Run c, 1, False
End Sub

