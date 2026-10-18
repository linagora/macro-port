' Source: https://github.com/MicrosoftDocs/VBA-Docs/blob/6765796079a1552460769cb633068d387594dc99/excel/Concepts/Controls-DialogBoxes-Forms/add-a-unique-list-of-values-to-a-combo-box.md
' Author: Microsoft Corporation (MicrosoftDocs/VBA-Docs)
' License: MIT
' Retrieved: 2026-10-17
' Family: 05-formulaires-saisie
'
Sub Populate_Combobox_Worksheet_Collection()
    'The Excel workbook and worksheets that contain the data, as well as the range placed on that data
    Dim wbBook As Workbook
    Dim wsSheet As Worksheet
    Dim rnData As Range

    Dim vaData As Variant               'the list, stored in a variant

    Dim ncData As New VBA.Collection    'the list, stored in a collection

    Dim lnCount As Long                 'the count used in the On Error Resume Next loop.

    Dim vaItem As Variant               'a variant representing the type of items in ncData

    'Instantiate the Excel objects.
    Set wbBook = ThisWorkbook
    Set wsSheet = wbBook.Worksheets("Sheet2")

    'Using Sheet2,retrieve the range of the list in Column A.
    With wsSheet
        Set rnData = .Range(.Range("A2"), .Range("A100").End(xlUp))
    End With

    'Place the list values into vaData.
    vaData = rnData.Value

    'Place the list values from vaData into the VBA.Collection.
    On Error Resume Next
        For lnCount = 1 To UBound(vaData)
        ncData.Add vaData(lnCount, 1), CStr(vaData(lnCount, 1))
    Next lnCount
    On Error GoTo 0

    'Clear the combo box (in case you ran the macro before),
    'and then add each unique variant item from ncData to the combo box.
    With wsSheet.OLEObjects("ComboBox1").Object
        .Clear
        For Each vaItem In ncData
            .AddItem ncData(vaItem)
        Next vaItem
    End With

End Sub
