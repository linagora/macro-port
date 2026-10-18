Attribute VB_Name = "M_07_04"
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
