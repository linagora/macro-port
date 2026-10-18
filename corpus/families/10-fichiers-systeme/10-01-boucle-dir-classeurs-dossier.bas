' Source: https://github.com/Alrowithy/vba-code/blob/703962956c17d25ed102c4f2771d078cd219c110/Utilities/SmartUtilities.bas#L125-L146
' Author: Marius Dragan
' License: MIT
' Retrieved: 2026-10-17
' Family: 10-fichiers-systeme
'
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
