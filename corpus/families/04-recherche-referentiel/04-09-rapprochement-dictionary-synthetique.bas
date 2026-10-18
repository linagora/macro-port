' Source: synthetique, corpus vba-migration-corpus (LINAGORA)
' Author: LINAGORA
' License: CC0-1.0
' Retrieved: 2026-10-17
' Family: 04-recherche-referentiel
' Synthetic: echantillon benin ecrit pour tester la detection, ne fait rien de nuisible
'
Option Explicit

' Rapprochement de deux listes par cle (matricule) avec Scripting.Dictionary
' Feuille "Paie" : matricule en A, montant en C ; feuille "RH" : matricule en A, nom en B
Public Sub RapprocherPaieEtRH()
    Dim dictRH As Object, wsPaie As Worksheet, wsRH As Worksheet, wsRes As Worksheet
    Dim derLigne As Long, i As Long, ligneRes As Long, cle As String

    Set dictRH = CreateObject("Scripting.Dictionary")
    dictRH.CompareMode = vbTextCompare
    Set wsPaie = ThisWorkbook.Worksheets("Paie")
    Set wsRH = ThisWorkbook.Worksheets("RH")

    derLigne = wsRH.Cells(wsRH.Rows.Count, 1).End(xlUp).Row
    For i = 2 To derLigne
        cle = Trim$(CStr(wsRH.Cells(i, 1).Value))
        If Len(cle) > 0 And Not dictRH.Exists(cle) Then dictRH.Add cle, i
    Next i

    Application.DisplayAlerts = False
    On Error Resume Next
    ThisWorkbook.Worksheets("Ecarts").Delete
    On Error GoTo 0
    Application.DisplayAlerts = True
    Set wsRes = ThisWorkbook.Worksheets.Add(After:=wsPaie)
    wsRes.Name = "Ecarts"
    wsRes.Range("A1:D1").Value = Array("Matricule", "Nom", "Montant", "Statut")
    ligneRes = 2

    derLigne = wsPaie.Cells(wsPaie.Rows.Count, 1).End(xlUp).Row
    For i = 2 To derLigne
        cle = Trim$(CStr(wsPaie.Cells(i, 1).Value))
        If dictRH.Exists(cle) Then
            dictRH.Remove cle
        Else
            wsRes.Cells(ligneRes, 1).Value = cle
            wsRes.Cells(ligneRes, 3).Value = wsPaie.Cells(i, 3).Value
            wsRes.Cells(ligneRes, 4).Value = "Paye sans fiche RH"
            ligneRes = ligneRes + 1
        End If
    Next i

    Dim k As Variant
    For Each k In dictRH.Keys
        wsRes.Cells(ligneRes, 1).Value = k
        wsRes.Cells(ligneRes, 2).Value = wsRH.Cells(dictRH(k), 2).Value
        wsRes.Cells(ligneRes, 4).Value = "Fiche RH sans paie"
        ligneRes = ligneRes + 1
    Next k

    wsRes.Columns("A:D").AutoFit
    MsgBox (ligneRes - 2) & " ecart(s) trouve(s).", vbInformation
End Sub
