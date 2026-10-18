' Source: https://github.com/MicrosoftDocs/VBA-Docs/blob/6765796079a1552460769cb633068d387594dc99/api/Excel.Range.Consolidate.md
' Author: Microsoft (VBA-Docs)
' License: MIT
' Retrieved: 2026-10-17
' Family: 02-consolidation
'
Worksheets("Sheet1").Range("A1").Consolidate _
 Sources:=Array("Sheet2!R1C1:R37C6", "Sheet3!R1C1:R37C6"), _
 Function:=xlSum
