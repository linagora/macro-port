Attribute VB_Name = "M_02_01"
Worksheets("Sheet1").Range("A1").Consolidate _
 Sources:=Array("Sheet2!R1C1:R37C6", "Sheet3!R1C1:R37C6"), _
 Function:=xlSum
