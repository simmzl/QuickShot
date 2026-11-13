Unicode true
RequestExecutionLevel user
ManifestDPIAware true
!include "MUI2.nsh"
!include "x64.nsh"

!ifndef VERSION
  !error "Provide VERSION, PAYLOAD and OUTPUT when invoking makensis"
!endif
!define PRODUCT_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\QuickShot"
!define RUN_KEY "Software\Microsoft\Windows\CurrentVersion\Run"

Name "QuickShot"
OutFile "${OUTPUT}"
InstallDir ""
Icon "..\assets\app-icon.ico"
UninstallIcon "..\assets\app-icon.ico"
ShowInstDetails show

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!define MUI_FINISHPAGE_RUN "$INSTDIR\QuickShot.exe"
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

Function .onInit
  ${IfNot} ${RunningX64}
    MessageBox MB_OK|MB_ICONSTOP "QuickShot requires 64-bit Windows." /SD IDOK
    Abort
  ${EndIf}
  SetRegView 64
  ; NSIS supplies $INSTDIR before .onInit when /D= is passed.
  ${If} $INSTDIR == ""
    ReadRegStr $INSTDIR HKCU "${PRODUCT_KEY}" "InstallLocation"
    ${If} $INSTDIR == ""
      StrCpy $INSTDIR "$LOCALAPPDATA\Programs\QuickShot"
    ${EndIf}
  ${EndIf}
FunctionEnd

Section "QuickShot"
  SetOutPath "$INSTDIR"
  ; Abort a silent upgrade if the tray app still owns the executable.
  ; Interactive installs offer Retry after the user quits from the tray.
  SetOverwrite on
  AllowSkipFiles off
  File "/oname=QuickShot.exe" "${PAYLOAD}"
  WriteUninstaller "$INSTDIR\Uninstall.exe"
  WriteRegStr HKCU "Software\Classes\quickshot-view" "" "URL:QuickShot screenshot"
  WriteRegStr HKCU "Software\Classes\quickshot-view" "URL Protocol" ""
  WriteRegStr HKCU "Software\Classes\quickshot-view\shell\open\command" "" '"$INSTDIR\QuickShot.exe" --view "%1"'
  CreateShortcut "$SMPROGRAMS\QuickShot.lnk" "$INSTDIR\QuickShot.exe"
  WriteRegStr HKCU "${PRODUCT_KEY}" "DisplayName" "QuickShot"
  WriteRegStr HKCU "${PRODUCT_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${PRODUCT_KEY}" "DisplayIcon" "$INSTDIR\QuickShot.exe"
  WriteRegStr HKCU "${PRODUCT_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${PRODUCT_KEY}" "UninstallString" '$\"$INSTDIR\Uninstall.exe$\"'
  WriteRegDWORD HKCU "${PRODUCT_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${PRODUCT_KEY}" "NoRepair" 1
  ReadRegStr $0 HKCU "${RUN_KEY}" "QuickShot"
  ${If} $0 != ""
    WriteRegStr HKCU "${RUN_KEY}" "QuickShot" '$\"$INSTDIR\QuickShot.exe$\"'
  ${EndIf}
SectionEnd

Section "Uninstall"
  SetRegView 64
  retry:
  ClearErrors
  Delete "$INSTDIR\QuickShot.exe"
  IfErrors 0 removed
  MessageBox MB_RETRYCANCEL|MB_ICONEXCLAMATION "Quit QuickShot from the tray, then retry." /SD IDCANCEL IDRETRY retry
  SetErrorLevel 1
  Abort
  removed:
  ReadRegStr $0 HKCU "${RUN_KEY}" "QuickShot"
  ${If} $0 == '$\"$INSTDIR\QuickShot.exe$\"'
    DeleteRegValue HKCU "${RUN_KEY}" "QuickShot"
  ${EndIf}
  ReadRegStr $0 HKCU "Software\Classes\quickshot-view\shell\open\command" ""
  ${If} $0 == '"$INSTDIR\QuickShot.exe" --view "%1"'
    DeleteRegKey HKCU "Software\Classes\quickshot-view"
    DeleteRegKey HKCU "Software\Classes\AppUserModelId\simmzl.QuickShot"
    DeleteRegKey HKCU "Software\QuickShot\NotificationFiles"
  ${EndIf}
  DeleteRegKey HKCU "${PRODUCT_KEY}"
  Delete "$SMPROGRAMS\QuickShot.lnk"
  Delete "$INSTDIR\Uninstall.exe"
  RMDir "$INSTDIR"
SectionEnd
