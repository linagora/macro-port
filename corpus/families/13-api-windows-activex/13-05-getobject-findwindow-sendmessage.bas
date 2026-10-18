' Source: https://github.com/MicrosoftDocs/VBA-Docs/blob/6765796079a1552460769cb633068d387594dc99/Language/Reference/User-Interface-Help/getobject-function.md
' Author: Microsoft
' License: MIT
' Retrieved: 2026-10-17
' Family: 13-api-windows-activex
'
' Declare necessary API routines:
Declare Function FindWindow Lib "user32" Alias _
"FindWindowA" (ByVal lpClassName as String, _
                    ByVal lpWindowName As Long) As Long

Declare Function SendMessage Lib "user32" Alias _
"SendMessageA" (ByVal hWnd as Long,ByVal wMsg as Long, _
                    ByVal wParam as Long, _
                    ByVal lParam As Long) As Long

Sub GetExcel()
    Dim MyXL As Object    ' Variable to hold reference
                                ' to Microsoft Excel.
    Dim ExcelWasNotRunning As Boolean    ' Flag for final release.

' Test to see if there is a copy of Microsoft Excel already running.
    On Error Resume Next    ' Defer error trapping.
' Getobject function called without the first argument returns a
' reference to an instance of the application. If the application isn't
' running, an error occurs.
    Set MyXL = Getobject(, "Excel.Application")
    If Err.Number <> 0 Then ExcelWasNotRunning = True
    Err.Clear    ' Clear Err object in case error occurred.

' Check for Microsoft Excel. If Microsoft Excel is running,
' enter it into the Running Object table.
    DetectExcel

' Set the object variable to reference the file you want to see.
    Set MyXL = Getobject("c:\vb4\MYTEST.XLS")

' Show Microsoft Excel through its Application property. Then
' show the actual window containing the file using the Windows
' collection of the MyXL object reference.
    MyXL.Application.Visible = True
    MyXL.Parent.Windows(1).Visible = True
     Do manipulations of your  file here.
    ' ...
' If this copy of Microsoft Excel was not running when you
' started, close it using the Application property's Quit method.
' Note that when you try to quit Microsoft Excel, the
' title bar blinks and a message is displayed asking if you
' want to save any loaded files.
    If ExcelWasNotRunning = True Then
        MyXL.Application.Quit
    End IF

    Set MyXL = Nothing    ' Release reference to the
                                ' application and spreadsheet.
End Sub

Sub DetectExcel()
' Procedure dectects a running Excel and registers it.
    Const WM_USER = 1024
    Dim hWnd As Long
' If Excel is running this API call returns its handle.
    hWnd = FindWindow("XLMAIN", 0)
    If hWnd = 0 Then    ' 0 means Excel not running.
        Exit Sub
    Else
    ' Excel is running so use the SendMessage API
    ' function to enter it in the Running Object Table.
        SendMessage hWnd, WM_USER + 18, 0, 0
    End If
End Sub
