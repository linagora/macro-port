' Source: https://github.com/Alrowithy/vba-code/blob/703962956c17d25ed102c4f2771d078cd219c110/Utilities/Utilities.bas
' Author: Marius Dragan (repo Alrowithy/vba-code)
' License: MIT
' Retrieved: 2026-10-17
' Family: 08-fonctions-personnalisees
'
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
