Attribute VB_Name = "M_12_07"
Option Explicit

' Export d'un graphique et d'une plage vers une nouvelle presentation PowerPoint (liaison tardive)
Public Sub ExporterVersPowerPoint()
    Dim ppApp As Object, ppPres As Object, ppSlide As Object
    Dim ws As Worksheet, co As ChartObject

    Set ws = ThisWorkbook.Worksheets("Synthese")
    On Error Resume Next
    Set ppApp = GetObject(, "PowerPoint.Application")
    On Error GoTo 0
    If ppApp Is Nothing Then Set ppApp = CreateObject("PowerPoint.Application")
    ppApp.Visible = True
    Set ppPres = ppApp.Presentations.Add

    ' Diapositive 1 : tableau de synthese colle en image
    Set ppSlide = ppPres.Slides.Add(1, 11) ' ppLayoutTitleOnly
    ppSlide.Shapes(1).TextFrame.TextRange.Text = "Synthese " & Format(Date, "mmmm yyyy")
    ws.Range("A1:F15").CopyPicture Appearance:=xlScreen, Format:=xlPicture
    ppSlide.Shapes.Paste

    ' Une diapositive par graphique
    For Each co In ws.ChartObjects
        Set ppSlide = ppPres.Slides.Add(ppPres.Slides.Count + 1, 11)
        ppSlide.Shapes(1).TextFrame.TextRange.Text = co.Chart.ChartTitle.Text
        co.Chart.ChartArea.Copy
        ppSlide.Shapes.PasteSpecial DataType:=2 ' ppPasteEnhancedMetafile
    Next co

    ppPres.SaveAs ThisWorkbook.Path & Application.PathSeparator & "Synthese.pptx"
End Sub
