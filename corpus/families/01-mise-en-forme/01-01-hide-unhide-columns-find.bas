' Source: https://github.com/MicrosoftDocs/VBA-Docs/blob/6765796079a1552460769cb633068d387594dc99/excel/Concepts/Cells-and-Ranges/hide-and-unhide-columns.md
' Author: Dennis Wallentin (Microsoft VBA-Docs)
' License: MIT
' Retrieved: 2026-10-17
' Family: 01-mise-en-forme
'
Sub Hide_Columns()

    'Excel objects.
    Dim m_wbBook As Workbook
    Dim m_wsSheet As Worksheet
    Dim m_rnCheck As Range
    Dim m_rnFind As Range
    Dim m_stAddress As String

    'Initialize the Excel objects.
    Set m_wbBook = ThisWorkbook
    Set m_wsSheet = m_wbBook.Worksheets("Sheet1")

    'Search the four columns for any constants.
    Set m_rnCheck = m_wsSheet.Range("A1:D1").SpecialCells(xlCellTypeConstants)

    'Retrieve all columns that contain an X. If there is at least one, begin the DO/WHILE loop.
    With m_rnCheck
        Set m_rnFind = .Find(What:="X")
        If Not m_rnFind Is Nothing Then
            m_stAddress = m_rnFind.Address

            'Hide the column, and then find the next X.
            Do
                m_rnFind.EntireColumn.Hidden = True
                Set m_rnFind = .FindNext(m_rnFind)
            Loop While Not m_rnFind Is Nothing And m_rnFind.Address <> m_stAddress
        End If
    End With

End Sub

Sub Unhide_Columns()
    'Excel objects.
    Dim m_wbBook As Workbook
    Dim m_wsSheet As Worksheet
    Dim m_rnCheck As Range
    Dim m_rnFind As Range
    Dim m_stAddress As String

    'Initialize the Excel objects.
    Set m_wbBook = ThisWorkbook
    Set m_wsSheet = m_wbBook.Worksheets("Sheet1")

    'Search the four columns for any constants.
    Set m_rnCheck = m_wsSheet.Range("A1:D1").SpecialCells(xlCellTypeConstants)

    'Retrieve all columns that contain X. If there is at least one, begin the DO/WHILE loop.
    With m_rnCheck
        Set m_rnFind = .Find(What:="X", LookIn:=xlFormulas)
        If Not m_rnFind Is Nothing Then
            m_stAddress = m_rnFind.Address

            'Unhide the column, and then find the next X.
            Do
                m_rnFind.EntireColumn.Hidden = False
                Set m_rnFind = .FindNext(m_rnFind)
            Loop While Not m_rnFind Is Nothing And m_rnFind.Address <> m_stAddress
        End If
    End With

End Sub
