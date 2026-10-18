Attribute VB_Name = "M_12_03"
Sub Import_Contacts()

    'Outlook objects.
    Dim olApp As Outlook.Application
    Dim olNamespace As Outlook.Namespace
    Dim olFolder As Outlook.MAPIFolder
    Dim olConItems As Outlook.Items
    Dim olItem As Object

    'Excel objects.
    Dim wbBook As Workbook
    Dim wsSheet As Worksheet

    'Location in the imported contact list.
    Dim lnContactCount As Long

    Dim strDummy As String

    'Turn off screen updating.
    Application.ScreenUpdating = False

    'Initialize the Excel objects.
    Set wbBook = ThisWorkbook
    Set wsSheet = wbBook.Worksheets(1)

    'Format the target worksheet.
    With wsSheet
        .Range("A1").CurrentRegion.Clear
        .Cells(1, 1).Value = "Company / Private Person"
        .Cells(1, 2).Value = "Street Address"
        .Cells(1, 3).Value = "Postal Code"
        .Cells(1, 4).Value = "City"
        .Cells(1, 5).Value = "Contact Person"
        .Cells(1, 6).Value = "Email"
        With .Range("A1:F1")
            .Font.Bold = True
            .Font.ColorIndex = 10
            .Font.Size = 11
        End With
    End With

    wsSheet.Activate

    'Initialize the Outlook variables with the MAPI namespace and the default Outlook folder of the current user.
    Set olApp = New Outlook.Application
    Set olNamespace = olApp.GetNamespace("MAPI")
    Set olFolder = olNamespace.GetDefaultFolder(10)
    Set olConItems = olFolder.Items

    'Row number to place the new information on; starts at 2 to avoid overwriting the header
    lnContactCount = 2

    'For each contact: if it is a business contact, write out the business info in the Excel worksheet;
    'otherwise, write out the personal info.
    For Each olItem In olConItems
        If TypeName(olItem) = "ContactItem" Then
            With olItem
                If InStr(olItem.CompanyName, strDummy) > 0 Then
                    Cells(lnContactCount, 1).Value = .CompanyName
                    Cells(lnContactCount, 2).Value = .BusinessAddressStreet
                    Cells(lnContactCount, 3).Value = .BusinessAddressPostalCode
                    Cells(lnContactCount, 4).Value = .BusinessAddressCity
                    Cells(lnContactCount, 5).Value = .FullName
                    Cells(lnContactCount, 6).Value = .Email1Address
                Else
                    Cells(lnContactCount, 1) = .FullName
                    Cells(lnContactCount, 2) = .HomeAddressStreet
                    Cells(lnContactCount, 3) = .HomeAddressPostalCode
                    Cells(lnContactCount, 4) = .HomeAddressCity
                    Cells(lnContactCount, 5) = .FullName
                    Cells(lnContactCount, 6) = .Email1Address
                End If
                wsSheet.Hyperlinks.Add Anchor:=Cells(lnContactCount, 6), _
                                       Address:="mailto:" & Cells(lnContactCount, 6).Value, _
                                       TextToDisplay:=Cells(lnContactCount, 6).Value
            End With
            lnContactCount = lnContactCount + 1
        End If
    Next olItem

    'Null out the variables.
    Set olItem = Nothing
    Set olConItems = Nothing
    Set olFolder = Nothing
    Set olNamespace = Nothing
    Set olApp = Nothing

    'Sort the rows alphabetically using the CompanyName or FullName as appropriate, and then autofit.
    With wsSheet
        .Range("A2", Cells(2, 6).End(xlDown)).Sort key1:=Range("A2"), order1:=xlAscending
        .Range("A:F").EntireColumn.AutoFit
    End With

    'Turn screen updating back on.
    Application.ScreenUpdating = True

    MsgBox "The list has successfully been created!", vbInformation

End Sub
