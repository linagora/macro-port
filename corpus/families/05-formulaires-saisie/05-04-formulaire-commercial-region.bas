' Source: https://github.com/MicrosoftDocs/VBA-Docs/blob/6765796079a1552460769cb633068d387594dc99/excel/Concepts/Controls-DialogBoxes-Forms/using-control-values-while-code-is-running.md
' Author: Microsoft Corporation (MicrosoftDocs/VBA-Docs)
' License: MIT
' Retrieved: 2026-10-17
' Family: 05-formulaires-saisie
'
' Code in module to declare public variables.
Public strRegion As String
Public intSalesPersonID As Integer
Public blnCancelled As Boolean

' Code in form.
Private Sub cmdCancel_Click()
 Module1.blnCancelled = True
 Unload Me
End Sub

Private Sub cmdOK_Click()
 ' Save data.
 intSalesPersonID = txtSalesPersonID.Text
 strRegion = lstRegions.List(lstRegions.ListIndex)
 Module1.blnCancelled = False
 Unload Me
End Sub

Private Sub UserForm_Initialize()
 Module1.blnCancelled = True
End Sub

' Code in module to display form.
Sub LaunchSalesPersonForm()
 frmSalesPeople.Show
 If blnCancelled = True Then
 MsgBox "Operation Cancelled!", vbExclamation
 Else
 MsgBox "The Salesperson's ID is: " &
 intSalesPersonID & _
 "The Region is: " & strRegion
 End If
End Sub
