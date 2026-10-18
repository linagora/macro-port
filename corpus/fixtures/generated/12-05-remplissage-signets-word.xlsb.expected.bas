Attribute VB_Name = "M_12_05"
' You must pick Microsoft Word Object Library from Tools>References
' in the Visual Basic editor to execute Word commands.

Option Explicit

Sub Add_Single_Values_Word()
Dim wdApp As Word.Application
Dim wdDoc As Word.Document
Dim wdRange1 As Word.Range
Dim wdRange2 As Word.Range
Dim wdRange3 As Word.Range

Dim wbBook As Workbook
Dim wsSheet As Worksheet

Dim vaData As Variant

Set wbBook = ThisWorkbook
Set wsSheet = wbBook.Worksheets("Sheet1")

vaData = wsSheet.Range("W_Data").Value

' Instantiate the Word Objects.
Set wdApp = New Word.Application
Set wdDoc = wdApp.Documents.Open(wbBook.Path & "\Test.docx")

With wdDoc
    Set wdRange1 = .Bookmarks("td1").Range
    Set wdRange2 = .Bookmarks("td2").Range
    Set wdRange3 = .Bookmarks("td3").Range
End With

' Write values to the bookmarks.
wdRange1.Text = vaData(1, 1)
wdRange2.Text = vaData(2, 1)
wdRange3.Text = vaData(3, 1)

With wdDoc
    .Save
    .Close
End With

wdApp.Quit

' Release the objects from memory.
Set wdRange1 = Nothing
Set wdRange2 = Nothing
Set wdRange3 = Nothing
Set wdDoc = Nothing
Set wdApp = Nothing

End Sub
