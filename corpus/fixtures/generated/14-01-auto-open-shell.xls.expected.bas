Attribute VB_Name = "M_14_01"
Option Explicit

' Motif : execution automatique a l'ouverture + lancement d'un programme externe
Sub Auto_Open()
    Dim r As Double
    r = Shell("notepad.exe", vbNormalFocus)
End Sub
