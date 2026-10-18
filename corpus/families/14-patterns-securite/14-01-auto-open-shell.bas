' Source: synthetique, corpus vba-migration-corpus (LINAGORA)
' Author: LINAGORA
' License: CC0-1.0
' Retrieved: 2026-10-17
' Family: 14-patterns-securite
' Synthetic: echantillon benin ecrit pour tester la detection, ne fait rien de nuisible
'
Option Explicit

' Motif : execution automatique a l'ouverture + lancement d'un programme externe
Sub Auto_Open()
    Dim r As Double
    r = Shell("notepad.exe", vbNormalFocus)
End Sub
