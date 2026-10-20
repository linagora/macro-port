Attribute VB_Name = "M_14_04"
Option Explicit

' Motif : identifiants en clair dans le code (chaine de connexion fictive)
Private Const CNX As String = "Provider=SQLOLEDB;Data Source=srv-finance01;Initial Catalog=COMPTA;User ID=sa;Password=Fictif2026!;"

Public Sub ChargerEcritures()
    Dim cn As Object, rs As Object
    Set cn = CreateObject("ADODB.Connection")
    cn.Open CNX
    Set rs = cn.Execute("SELECT compte, libelle, montant FROM ecritures WHERE exercice = 2026")
    ThisWorkbook.Worksheets("Ecritures").Range("A2").CopyFromRecordset rs
    rs.Close: cn.Close
End Sub
