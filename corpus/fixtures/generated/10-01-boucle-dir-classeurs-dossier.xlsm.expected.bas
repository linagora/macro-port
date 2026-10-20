Attribute VB_Name = "M_10_01"
Private Sub LoopThroughFiles()

    Dim folderPath As String
    Dim filename As String
    Dim WB As Workbook

    folderPath = "C:\Users\str-brompton.smc.uk\Desktop\New folder\" 'change to suit

    If Right(folderPath, 1) <> "\" Then folderPath = folderPath + "\"

    filename = Dir(folderPath & "*.xls")
    Do While filename <> ""
    Debug.Print (filename)
      Application.ScreenUpdating = False
        Set WB = Workbooks.Open(folderPath & filename)

        'Call a subroutine here to operate on the just-opened workbook
        'Call Edit121ReportWithAutoPrint
        WB.Close False
        filename = Dir
    Loop
  Application.ScreenUpdating = True
