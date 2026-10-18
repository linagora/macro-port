' Source: https://github.com/MicrosoftDocs/VBA-Docs/blob/6765796079a1552460769cb633068d387594dc99/Language/Reference/User-Interface-Help/declare-statement.md
' Author: Microsoft
' License: MIT
' Retrieved: 2026-10-17
' Family: 13-api-windows-activex
'
' In Microsoft Windows (16-bit):
Declare Sub MessageBeep Lib "User" (ByVal N As Integer)
' Assume SomeBeep is an alias for the procedure name.
Declare Sub MessageBeep Lib "User" Alias "SomeBeep"(ByVal N As Integer)
' Use an ordinal in the Alias clause to call GetWinFlags.
Declare Function GetWinFlags Lib "Kernel" Alias "#132"()As Long

' In 32-bit Microsoft Windows systems, specify the library USER32.DLL,
' rather than USER.DLL. Use conditional compilation to write
' code that can run on either Win32 or Win16.
#If Win32 Then
    Declare Sub MessageBeep Lib "User32" (ByVal N As Long)
#Else
    Declare Sub MessageBeep Lib "User" (ByVal N As Integer)
#End If


' 64-bit Declare statement example:
Declare PtrSafe Function GetActiveWindow Lib "User32" () As LongPtr

' Conditional Compilation Example
#If Vba7 Then
     ' Code is running in  32-bit or 64-bit VBA7.
     #If Win64 Then
          ' Code is running in 64-bit VBA7.
     #Else
          ' Code is not running in 64-bit VBA7.
     #End If
#Else
     ' Code is NOT running in 32-bit or 64-bit VBA7.
#End If
