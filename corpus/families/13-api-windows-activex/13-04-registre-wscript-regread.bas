' Source: https://github.com/Alrowithy/vba-code/blob/703962956c17d25ed102c4f2771d078cd219c110/Utilities/ExtraOptions.bas#L143-L157
' Author: Marius Dragan
' License: MIT
' Retrieved: 2026-10-17
' Family: 13-api-windows-activex
'
Function RegKeyExists(key)
  Dim oShell, entry
  On Error Resume Next

  Set oShell = CreateObject("WScript.Shell")
  entry = oShell.RegRead(key)
  If Err.Number = 0 Then
    Err.Clear
    RegKeyExists = True
  Else
    Err.Clear
    RegKeyExists = False
  End If
  On Error GoTo 0
End Function
