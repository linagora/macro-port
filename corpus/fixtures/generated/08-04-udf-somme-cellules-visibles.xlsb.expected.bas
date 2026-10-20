Attribute VB_Name = "M_08_04"
Private Function SumOnlyVisible(r As Range) As Double
    Dim rCell As Range

    Application.Volatile

    Dim cell As Range

    For Each rCell In r.Cells
        With rCell
            If Not .Rows.Hidden And Not .Columns.Hidden Then SumOnlyVisible = SumOnlyVisible + .value
        End With
    Next
End Function
