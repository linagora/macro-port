' Source: https://github.com/MicrosoftDocs/VBA-Docs/blob/6765796079a1552460769cb633068d387594dc99/Language/Reference/User-Interface-Help/sendkeys-statement.md
' Author: Microsoft
' License: MIT
' Retrieved: 2026-10-17
' Family: 13-api-windows-activex
'
Dim ReturnValue, I
ReturnValue = Shell("CALC.EXE", 1)    ' Run Calculator.
AppActivate ReturnValue     ' Activate the Calculator.
For I = 1 To 100    ' Set up counting loop.
    SendKeys I & "{+}", True    ' Send keystrokes to Calculator
Next I    ' to add each value of I.
SendKeys "=", True    ' Get grand total.
SendKeys "%{F4}", True    ' Send ALT+F4 to close Calculator.
