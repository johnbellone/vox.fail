; Block Avatar installer (NSIS 3).
; Build on Windows after `cargo build --release`:
;   makensis /DVERSION=0.1.0 installer\block-avatar.nsi
; Produces installer\Block-Avatar-Setup-<version>.exe.
; Your own files (settings, clips, logs, questions, gags.txt, sounds) live in
; %APPDATA%\Block Avatar and are kept when you uninstall.

Unicode true
!ifndef VERSION
  !define VERSION "0.1.0"
!endif
!ifndef SRC
  !define SRC "..\..\.."
!endif
!define APP "Block Avatar"
!define EXE "block_avatar.exe"
!define UNINST_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\BlockAvatar"

Name "${APP}"
OutFile "Block-Avatar-Setup-${VERSION}.exe"
InstallDir "$PROGRAMFILES64\${APP}"
InstallDirRegKey HKLM "Software\${APP}" "InstallDir"
RequestExecutionLevel admin
SetCompressor /SOLID lzma
!if /FileExists "${SRC}\assets\icon.ico"
  Icon "${SRC}\assets\icon.ico"
  UninstallIcon "${SRC}\assets\icon.ico"
!endif

!include "MUI2.nsh"
!define MUI_ABORTWARNING
!define MUI_FINISHPAGE_RUN "$INSTDIR\${EXE}"
!define MUI_FINISHPAGE_RUN_TEXT "Start ${APP} now (opens the setup page)"
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_COMPONENTS
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

VIProductVersion "${VERSION}.0"
VIAddVersionKey "ProductName" "${APP}"
VIAddVersionKey "FileDescription" "${APP} installer"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "LegalCopyright" ""

Section "${APP} (required)" SecMain
  SectionIn RO
  SetOutPath "$INSTDIR"
  File "${SRC}\target\release\${EXE}"
  File /oname=README.md "${SRC}\docs\avatar\manual.md"
  SetOutPath "$INSTDIR\assets"
  File /r "${SRC}\assets\*.*"
  SetOutPath "$INSTDIR"

  WriteRegStr HKLM "Software\${APP}" "InstallDir" "$INSTDIR"
  WriteRegStr HKLM "${UNINST_KEY}" "DisplayName" "${APP}"
  WriteRegStr HKLM "${UNINST_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKLM "${UNINST_KEY}" "Publisher" "${APP}"
  WriteRegStr HKLM "${UNINST_KEY}" "DisplayIcon" "$INSTDIR\${EXE}"
  WriteRegStr HKLM "${UNINST_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKLM "${UNINST_KEY}" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegDWORD HKLM "${UNINST_KEY}" "NoModify" 1
  WriteRegDWORD HKLM "${UNINST_KEY}" "NoRepair" 1
  WriteUninstaller "$INSTDIR\uninstall.exe"

  ; the app looks for assets\ next to itself, so the shortcut starts there
  CreateDirectory "$SMPROGRAMS\${APP}"
  CreateShortcut "$SMPROGRAMS\${APP}\${APP}.lnk" "$INSTDIR\${EXE}" "" "$INSTDIR\${EXE}" 0
  CreateShortcut "$SMPROGRAMS\${APP}\Your files (clips, logs, settings).lnk" "$APPDATA\${APP}"
  CreateShortcut "$SMPROGRAMS\${APP}\Uninstall ${APP}.lnk" "$INSTDIR\uninstall.exe"
SectionEnd

Section "Desktop shortcut" SecDesktop
  CreateShortcut "$DESKTOP\${APP}.lnk" "$INSTDIR\${EXE}" "" "$INSTDIR\${EXE}" 0
SectionEnd

Section "Allow the viewer question page through the firewall" SecFirewall
  ; only needed when viewers reach the ask page directly (not through a tunnel)
  nsExec::Exec 'netsh advfirewall firewall add rule name="${APP} viewer questions" dir=in action=allow protocol=TCP localport=7778 program="$INSTDIR\${EXE}"'
SectionEnd

LangString DESC_Main ${LANG_ENGLISH} "The app, characters, props and stage sets."
LangString DESC_Desktop ${LANG_ENGLISH} "A shortcut on your desktop."
LangString DESC_Firewall ${LANG_ENGLISH} "Lets viewers reach the question page on port 7778. Skip this if you use a tunnel or don't take viewer questions."
!insertmacro MUI_FUNCTION_DESCRIPTION_BEGIN
  !insertmacro MUI_DESCRIPTION_TEXT ${SecMain} $(DESC_Main)
  !insertmacro MUI_DESCRIPTION_TEXT ${SecDesktop} $(DESC_Desktop)
  !insertmacro MUI_DESCRIPTION_TEXT ${SecFirewall} $(DESC_Firewall)
!insertmacro MUI_FUNCTION_DESCRIPTION_END
Section "Uninstall"
  nsExec::Exec 'netsh advfirewall firewall delete rule name="${APP} viewer questions"'
  Delete "$DESKTOP\${APP}.lnk"
  RMDir /r "$SMPROGRAMS\${APP}"
  RMDir /r "$INSTDIR\assets"
  Delete "$INSTDIR\${EXE}"
  Delete "$INSTDIR\README.md"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"
  DeleteRegKey HKLM "${UNINST_KEY}"
  DeleteRegKey HKLM "Software\${APP}"
  ; %APPDATA%\Block Avatar (your clips, settings and logs) is left alone on purpose
SectionEnd
