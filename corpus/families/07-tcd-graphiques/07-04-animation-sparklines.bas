' Source: https://github.com/MicrosoftDocs/VBA-Docs/blob/6765796079a1552460769cb633068d387594dc99/excel/Concepts/Sparklines/animate-a-sparkline.md
' Author: Microsoft Corporation (MicrosoftDocs/VBA-Docs)
' License: MIT
' Retrieved: 2026-10-17
' Family: 07-tcd-graphiques
'
Sub SparkAnimation()

    ' The group of sparklines to animate
    Dim oSparkGroup As SparklineGroup
    'variables for the loop
    Dim i As Integer, j As Integer

    ' Set up the sparkline group variable
    Set oSparkGroup = Sheet1.Range("A2").SparklineGroups(1)

    ' Set the data source to the first year of data
    oSparkGroup.ModifySourceData "B2:M4"

    ' Loop through the data points for the subsequent two years
    For i = 1 To 24
        ' Move the reference for the sparkline group over one cell
        oSparkGroup.ModifySourceData Range(oSparkGroup.SourceData).Offset(, 1).Address

        ' Slow the animation
        j = 1
        Do
            j = j + 1: DoEvents
        Loop Until j = 4000
    Next i

End Sub
