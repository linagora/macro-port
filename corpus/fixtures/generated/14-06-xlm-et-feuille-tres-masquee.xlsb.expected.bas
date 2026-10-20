Attribute VB_Name = "M_14_06"
Option Explicit

' Motif : macro XLM / Excel 4.0 appelee depuis VBA, et masquage de feuille "tres masquee"
Public Sub MasquerParametres()
    ThisWorkbook.Worksheets("Parametres").Visible = xlSheetVeryHidden
    Application.ExecuteExcel4Macro "MESSAGE(TRUE,""Calcul en cours"")"
    Application.ExecuteExcel4Macro "MESSAGE(FALSE)"
End Sub
