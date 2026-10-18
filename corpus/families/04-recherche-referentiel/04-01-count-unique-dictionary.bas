' Source: https://github.com/byronwall/bUTL/blob/be303d89ea61c7326a381b91596fc85ce60901f4/src/code/Testing.bas
' Author: Byron Wall (@byronwall)
' License: MIT
' Retrieved: 2026-10-17
' Family: 04-recherche-referentiel
'
Public Sub CountUnique()
    '---------------------------------------------------------------------------------------
    ' Procedure : CountUnique
    ' Author    : @byronwall
    ' Date      : 2016 01 27
    ' Purpose   : counts the number of unique values in a Range
    '---------------------------------------------------------------------------------------
    '

    Dim rng_data As Range

    Set rng_data = GetInputOrSelection("select the range to count unique")
    Set rng_data = Intersect(rng_data, rng_data.Parent.UsedRange)

    Dim dict_vals As New Dictionary

    Dim rng_val As Range

    For Each rng_val In rng_data
        If Not dict_vals.Exists(rng_val.Value) Then
            dict_vals.Add rng_val.Value, 1
        End If
    Next

    MsgBox "items: " & dict_vals.count

End Sub
