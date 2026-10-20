Attribute VB_Name = "M_12_01"
Sub Sample()
   'Setting up the Excel variables.
   Dim olApp As Object
   Dim olMailItm As Object
   Dim iCounter As Integer
   Dim Dest As Variant
   Dim SDest As String

   'Create the Outlook application and the empty email.
   Set olApp = CreateObject("Outlook.Application")
   Set olMailItm = olApp.CreateItem(0)

   'Using the email, add multiple recipients, using a list of addresses in column A.
   With olMailItm
       SDest = ""
       For iCounter = 1 To WorksheetFunction.CountA(Columns(1))
           If SDest = "" Then
               SDest = Cells(iCounter, 1).Value
           Else
               SDest = SDest & ";" & Cells(iCounter, 1).Value
           End If
       Next iCounter

    'Do additional formatting on the BCC and Subject lines, add the body text from the spreadsheet, and send.
       .BCC = SDest
       .Subject = "FYI"
       .Body = ActiveSheet.TextBoxes(1).Text
       .Send
   End With

   'Clean up the Outlook application.
   Set olMailItm = Nothing
   Set olApp = Nothing
End Sub
