' Source: https://github.com/MicrosoftDocs/VBA-Docs/blob/6765796079a1552460769cb633068d387594dc99/excel/Concepts/Cells-and-Ranges/make-a-cell-blink.md
' Author: Microsoft Corporation (MicrosoftDocs/VBA-Docs)
' License: MIT
' Retrieved: 2026-10-17
' Family: 09-evenements
'
Option Explicit

Public NextBlink As Double
'The cell that you want to blink
Public Const BlinkCell As String = "Sheet1!B2"

'Start blinking
Private Sub StartBlinking()
    Application.Goto Range("A1"), 1
    'If the color is red, change the color and text to white
    If Range(BlinkCell).Interior.ColorIndex = 3 Then
        Range(BlinkCell).Interior.ColorIndex = 0
        Range(BlinkCell).Value = "White"
    'If the color is white, change the color and text to red
    Else
        Range(BlinkCell).Interior.ColorIndex = 3
        Range(BlinkCell).Value = "Red"
    End If
    'Wait one second before changing the color again
    NextBlink = Now + TimeSerial(0, 0, 1)
    Application.OnTime NextBlink, "StartBlinking", , True
End Sub

'Stop blkinking
Private Sub StopBlinking()
    'Set color to white
    Range(BlinkCell).Interior.ColorIndex = 0
    'Clear the value in the cell
    Range(BlinkCell).ClearContents
    On Error Resume Next
    Application.OnTime NextBlink, "StartBlinking", , False
    Err.Clear
End Sub
