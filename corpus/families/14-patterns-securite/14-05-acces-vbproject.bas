' Source: synthetique, corpus vba-migration-corpus (LINAGORA)
' Author: LINAGORA
' License: CC0-1.0
' Retrieved: 2026-10-17
' Family: 14-patterns-securite
' Synthetic: echantillon benin ecrit pour tester la detection, ne fait rien de nuisible
'
Option Explicit

' Motif : acces au modele objet du projet VBA (necessite "Faire confiance a l'acces au modele objet du projet VBA")
' Usage legitime frequent : export des modules pour versionnement
Public Sub ExporterModules()
    Dim comp As Object, dossier As String, ext As String
    dossier = ThisWorkbook.Path & "\src\"
    If Dir(dossier, vbDirectory) = "" Then MkDir dossier
    For Each comp In ThisWorkbook.VBProject.VBComponents
        Select Case comp.Type
            Case 1: ext = ".bas"
            Case 2, 100: ext = ".cls"
            Case 3: ext = ".frm"
        End Select
        comp.Export dossier & comp.Name & ext
    Next comp
End Sub

' Variante a risque : ajout de code a la volee
Public Sub InjecterProcedure()
    ThisWorkbook.VBProject.VBComponents("Module1").CodeModule.AddFromString _
        "Sub Bonjour()" & vbCrLf & "MsgBox ""Bonjour""" & vbCrLf & "End Sub"
End Sub
