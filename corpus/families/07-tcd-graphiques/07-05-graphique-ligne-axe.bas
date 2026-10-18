' Source: https://github.com/MicrosoftDocs/VBA-Docs/blob/6765796079a1552460769cb633068d387594dc99/api/Excel.Axis.CrossesAt.md
' Author: Microsoft Corporation (MicrosoftDocs/VBA-Docs)
' License: MIT
' Retrieved: 2026-10-17
' Family: 07-tcd-graphiques
'
Sub Chart()

 ' Create a sample source of data.
 Range("A1") = "2"
 Range("A2") = "4"
 Range("A3") = "6"
 Range("A4") = "3"

 ' Create a chart based on the sample source of data.
 Charts.Add

 With ActiveChart
 .ChartType = xlLineMarkersStacked
 .SetSourceData Source:=Sheets("Sheet1").Range("A1:A4"), PlotBy:= xlColumns
 .Location Where:=xlLocationAsObject, Name:="Sheet1"
 End With

 ' Set the category axis to cross the value axis at value 3.
 ActiveChart.Axes(xlValue).Select
 Selection.CrossesAt = 3

End Sub
