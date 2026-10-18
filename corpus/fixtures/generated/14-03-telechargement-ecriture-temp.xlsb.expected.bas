Attribute VB_Name = "M_14_03"
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
