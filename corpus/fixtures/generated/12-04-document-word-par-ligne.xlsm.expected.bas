Attribute VB_Name = "M_12_04"
' You must pick Microsoft Word Object Library from Tools>References
' in the VB editor to execute Word commands.
Sub ControlWord()
    Dim appWD As Word.Application
    ' Create a new instance of Word and make it visible
    Set appWD = CreateObject("Word.Application.12")
    appWD.Visible = True

    ' Find the last row with data in the spreadsheet
    FinalRow = Range("A9999").End(xlUp).Row
    For i = 1 To FinalRow
        ' Copy the current row
        Worksheets("Sheet1").Rows(i).Copy
        ' Tell Word to create a new document
        appWD.Documents.Add
        ' Tell Word to paste the contents of the clipboard into the new document.
        appWD.Selection.Paste
        ' Save the new document with a sequential file name.
        appWD.ActiveDocument.SaveAs Filename:="File" & i
        ' Close the new Word document.
        appWD.ActiveDocument.Close
    Next i
    ' Close the Word application.
    appWD.Quit
End Sub
