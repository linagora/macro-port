' Source: synthetique, corpus vba-migration-corpus (LINAGORA)
' Author: LINAGORA
' License: CC0-1.0
' Retrieved: 2026-10-17
' Family: 14-patterns-securite
' Synthetic: echantillon benin ecrit pour tester la detection, ne fait rien de nuisible
'
Option Explicit

' Motif : macro XLM / Excel 4.0 appelee depuis VBA, et masquage de feuille "tres masquee"
Public Sub MasquerParametres()
    ThisWorkbook.Worksheets("Parametres").Visible = xlSheetVeryHidden
    Application.ExecuteExcel4Macro "MESSAGE(TRUE,""Calcul en cours"")"
    Application.ExecuteExcel4Macro "MESSAGE(FALSE)"
End Sub
