Attribute VB_Name = "M_07_02"
Dim cnnConn As ADODB.Connection
Dim rstRecordset As ADODB.Recordset
Dim cmdCommand As ADODB.Command

' Open the connection.
Set cnnConn = New ADODB.Connection
With cnnConn
 .ConnectionString = _
 "Provider=Microsoft.Jet.OLEDB.4.0"
 .Open "C:\perfdate\record.mdb"
End With

' Set the command text.
Set cmdCommand = New ADODB.Command
Set cmdCommand.ActiveConnection = cnnConn
With cmdCommand
 .CommandText = "Select Speed, Pressure, Time From DynoRun"
 .CommandType = adCmdText
 .Execute
End With

' Open the recordset.
Set rstRecordset = New ADODB.Recordset
Set rstRecordset.ActiveConnection = cnnConn
rstRecordset.Open cmdCommand

' Create PivotTable cache and report.
Set objPivotCache = ActiveWorkbook.PivotCaches.Add( _
 SourceType:=xlExternal)
Set objPivotCache.Recordset = rstRecordset

ActiveSheet.PivotTables.Add _
 PivotCache:=objPivotCache, _
 TableDestination:=Range("A3"), _
 TableName:="Performance"

With ActiveSheet.PivotTables("Performance")
 .SmallGrid = False
 With .PivotFields("Pressure")
 .Orientation = xlRowField
 .Position = 1
 End With
 With .PivotFields("Speed")
 .Orientation = xlColumnField
 .Position = 1
 End With
 With .PivotFields("Time")
 .Orientation = xlDataField
 .Position = 1
 End With
End With

' Close the connections and clean up.
cnnConn.Close
Set cmdCommand = Nothing
Set rstRecordset = Nothing
Set cnnConn = Nothing
