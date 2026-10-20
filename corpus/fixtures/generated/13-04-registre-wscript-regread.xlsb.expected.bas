Attribute VB_Name = "M_13_04"
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
