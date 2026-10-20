Attribute VB_Name = "M_01_02"
Sub TN_GL_hold()
'
' updated Macro
'
Dim hideRng As Range
Dim i As Long
Dim lastrow As Long

  ' Sheet2 => define the sheet you are using
With Sheet2
' sheet2 = TNs
    'count how many rows are using column B
    lastrow = .Cells(.Rows.Count, "B").End(xlUp).Row
'Unhide rows
    .Cells.EntireRow.Hidden = False
'Lets say my table starts by 6 and ends in lastrow value
    For i = 6 To lastrow
'Condition in this case if columns W OR Y OR AA for all rows check if the value is not "on Hold"
        If Not (.Range("W" & i) = "On hold" Or .Range("Y" & i) = "On Hold" Or .Range("AA" & i) = "On hold") Then
  'if hide range is nohting then set hide range => first record
            If hideRng Is Nothing Then
              Set hideRng = .Range("B" & i) ' first record not on hold
            Else
      ' else append range to first record
              Set hideRng = Union(.Range("B" & i), hideRng) ' append records not on hold
            End If
        End If
    Next
  ' if there are records in hide range then hide them
    If Not hideRng Is Nothing Then hideRng.EntireRow.Hidden = True
    ' and that's it, you have filtered all rows without a ON hold value in columns W or Y or AA
    On Error Resume Next

    On Error GoTo 0
End With
End Sub
