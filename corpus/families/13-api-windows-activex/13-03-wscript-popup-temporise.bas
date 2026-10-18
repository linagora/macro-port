' Source: https://github.com/MicrosoftDocs/VBA-Docs/blob/6765796079a1552460769cb633068d387594dc99/excel/Concepts/Controls-DialogBoxes-Forms/automatically-dismiss-a-message-box.md
' Author: Tom Urtis (Microsoft Docs)
' License: MIT
' Retrieved: 2026-10-17
' Family: 13-api-windows-activex
'
Sub MessageBoxTimer()
    Dim AckTime As Integer, InfoBox As Object
    Set InfoBox = CreateObject("WScript.Shell")
    'Set the message box to close after 10 seconds
    AckTime = 10
    Select Case InfoBox.Popup("Click OK (this window closes automatically after 10 seconds).", _
    AckTime, "This is your Message Box", 0)
        Case 1, -1
            Exit Sub
    End Select
End Sub
