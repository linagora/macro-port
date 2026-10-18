' Source: synthetique, corpus vba-migration-corpus (LINAGORA)
' Author: LINAGORA
' License: CC0-1.0
' Retrieved: 2026-10-17
' Family: 14-patterns-securite
' Synthetic: echantillon benin ecrit pour tester la detection, ne fait rien de nuisible
'
Option Explicit

' Motif : telechargement HTTP puis ecriture sur disque dans %TEMP% (ADODB.Stream)
' URL volontairement inoffensive
Public Sub TelechargerReferentiel()
    Dim http As Object, stm As Object, chemin As String
    chemin = Environ("TEMP") & "\referentiel.csv"
    Set http = CreateObject("MSXML2.XMLHTTP")
    http.Open "GET", "https://example.com/referentiel.csv", False
    http.send
    If http.Status = 200 Then
        Set stm = CreateObject("ADODB.Stream")
        stm.Type = 1
        stm.Open
        stm.Write http.responseBody
        stm.SaveToFile chemin, 2
        stm.Close
        Workbooks.OpenText Filename:=chemin, DataType:=xlDelimited, Semicolon:=True
    End If
End Sub
