' Source: https://github.com/Marius-Dragan/VBA-CODE/blob/703962956c17d25ed102c4f2771d078cd219c110/ManipulateData/SpotCheck.bas
' Author: Marius Dragan
' License: MIT
' Retrieved: 2026-10-17
' Family: 04-recherche-referentiel
'
Sub ComparingFrozenReportWithConsReport()
'--> Need to add dependency SmartUtlilities

    'Source list
    Dim allOnHandList As Range
    Dim allOnHandListResult As Range
    Dim allOnHandListSkuCell As Range
    Dim allOnHandListHeaderRowsCount As Integer
    Dim allOnHandListHeaderRowNum As Integer
    Dim foundMatchingallOnHandListSku As Range

    'Comparing against list
    Dim consList As Range
    Dim consListCellRow As Long
    Dim consListHeaderRowNum As Integer

    'Output variables
    Dim firstFoundMatchingAddress As String
    Dim countSkus As Long

    'Variable to hold if match found on the comparing against list
    Dim allOnHandListSkuCriteria As Variant

     On Error GoTo ErrorHandler

    '---> this method needs all data to be visible in order to loop through all cells
    SmartUtilities.ResetFilters

    '---> Allows users to select the ranges in case the table columns will change in the future
    Set allOnHandList = Application.InputBox("Select your all on hand list list range including header:", Default:="'" & ActiveSheet.Name & "'!", Type:=8)
        If Not allOnHandList Is Nothing Then
            If allOnHandList.Columns.Count = 1 Then
                Else
                 MsgBox "Multiple columns selected! Please pick only one column in the all on hand list sheet and retry.", vbCritical
                Exit Sub
            End If
        End If

     Set allOnHandListResult = Application.InputBox("Select the column header cell in the all on hand list where to write the result:", Default:="'" & ActiveSheet.Name & "'!", Type:=8)
        If Not allOnHandListResult Is Nothing Then
            If allOnHandListResult.Rows.Count = 1 Then
                Else
                 MsgBox "Multiple cells selected! Please pick only the header cell in the all on hand list sheet and retry.", vbCritical
                Exit Sub
            End If
        End If


      Set consList = Application.InputBox("Select your consignment list range including header:", Default:="'" & ActiveSheet.Name & "'!", Type:=8)
        If Not consList Is Nothing Then
            If consList.Columns.Count = 1 Then
                Else
                 MsgBox "Multiple columns selected! Please pick only one column in the consignment sheet and retry.", vbCritical
                Exit Sub
            End If
        End If

    allOnHandListHeaderRowsCount = allOnHandListResult.Row
    allOnHandListHeaderRowNum = allOnHandListResult.Row - 1

    consListHeaderRowNum = consList.Row - 1

        Application.ScreenUpdating = False

     If consList.End(xlToRight).value <> "Matching skus" Then
        consList.End(xlToRight).Offset(0, 1).value = "Matching skus"
     End If

    '---> Allows users to compare the consignment list to the inventory list in order to find matches
    For Each allOnHandListSkuCell In allOnHandList
        allOnHandListSkuCriteria = Trim(allOnHandListSkuCell.value) 'Using trim to delete the extra space from the data otherwhise it will throw an error

        With consList
            Set foundMatchingallOnHandListSku = .Find(What:=allOnHandListSkuCriteria, After:=.Cells(1, 1), LookIn:=xlValues, _
                                                LookAt:=xlWhole, SearchOrder:=xlByRows, SearchDirection:=xlNext, MatchCase:=True, _
                                                SearchFormat:=False) 'finds a match
        End With

    If foundMatchingallOnHandListSku Is Nothing Then
        If allOnHandListSkuCell.Row = allOnHandListHeaderRowsCount Then
            If allOnHandListResult.Cells(allOnHandListSkuCell.Row - allOnHandListHeaderRowNum).value = vbNullString Then
                allOnHandListResult.Cells(allOnHandListSkuCell.Row - allOnHandListHeaderRowNum).value = "Open Consignment List"
                allOnHandListResult.Font.FontStyle = "Bold"
            End If
        End If

      Else

            consListCellRow = foundMatchingallOnHandListSku.Row
            firstFoundMatchingAddress = foundMatchingallOnHandListSku.Address

            Do '---> Looping through all instances of a value and write result

                With consList
                    If allOnHandListResult.Cells(allOnHandListSkuCell.Row - allOnHandListHeaderRowNum).value = vbNullString Then

                         allOnHandListResult.Cells(allOnHandListSkuCell.Row - allOnHandListHeaderRowNum).value = .Cells(consListCellRow - consListHeaderRowNum, 6).value & " on cons " _
                                                                                        & .Cells(consListCellRow - consListHeaderRowNum, -7).value _
                                                                                        & " (" _
                                                                                        & .Cells(consListCellRow - consListHeaderRowNum, -6).value _
                                                                                        & ") " _
                                                                                        & .Cells(consListCellRow - consListHeaderRowNum, -5).value
                    Else

                         allOnHandListResult.Cells(allOnHandListSkuCell.Row - allOnHandListHeaderRowNum).value = allOnHandListResult.Cells(allOnHandListSkuCell.Row - allOnHandListHeaderRowNum).value _
                                                                                        & " // " _
                                                                                        & .Cells(consListCellRow - consListHeaderRowNum, 6).value & " on cons " _
                                                                                        & .Cells(consListCellRow - consListHeaderRowNum, -7).value _
                                                                                        & " (" _
                                                                                        & .Cells(consListCellRow - consListHeaderRowNum, -6).value _
                                                                                        & ") " _
                                                                                       & .Cells(consListCellRow - consListHeaderRowNum, -5).value

                    End If

                    countSkus = countSkus + 1

                    If foundMatchingallOnHandListSku.End(xlToRight) <> "Found" Then
                        foundMatchingallOnHandListSku.End(xlToRight).Offset(0, 1).value = "Found"
                    End If

                    Set foundMatchingallOnHandListSku = .FindNext(foundMatchingallOnHandListSku)

                End With

            Loop While Not foundMatchingallOnHandListSku Is Nothing And foundMatchingallOnHandListSku.Address <> firstFoundMatchingAddress

    End If

    Next allOnHandListSkuCell

        Application.ScreenUpdating = True

        MsgBox "Process completed!" & vbNewLine & vbNewLine & "Found " & countSkus & " skus in the consignment list." & vbCrLf, vbInformation, "Comparing lists"

Exit Sub
ErrorHandler:
        Application.ScreenUpdating = True
        Select Case Err.Number
                Case 424
                Exit Sub
                Case 0
                Exit Sub

                Case Else
                Debug.Print Err.Number, Err.Description
                MsgBox Err.Number, Err.Source, Err.Description, Err.HelpFile, Err.HelpContext

        End Select

End Sub
