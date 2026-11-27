; Vox desktop client installer (NSIS 3, per-user, no administrator rights).
;
; Build on Windows after `cargo build --release -p vox-client`:
;   makensis /DVERSION=1.2.3 /DNUMVERSION=1.2.3 /DOUTDIR=C:\build\dist bins\vox-client\package\windows\vox.nsi
; OUTDIR must be an absolute path: makensis resolves a relative one from this script's folder.
; VERSION is the text shown to people (it may be "nightly"). NUMVERSION is the numeric x.y.z
; that Windows reads for the file version, so it must be digits and dots only.
; Produces <OUTDIR>\Vox-<VERSION>-windows-x64-setup.exe.
;
; The game installs into %LOCALAPPDATA%\Programs\Vox. Your own levels and your controls
; (levels\, controls.json) live in %APPDATA%\Vox. The uninstaller never touches %APPDATA%\Vox,
; so a reinstall or an update keeps them.

Unicode true
!ifndef VERSION
  !define VERSION "nightly"
!endif
!ifndef NUMVERSION
  !define NUMVERSION "0.0.0"
!endif
!ifndef SRC
  !define SRC "..\..\..\.."
!endif
!ifndef BIN
  !define BIN "${SRC}\target\release\vox-client.exe"
!endif
!ifndef OUTDIR
  !define OUTDIR "."
!endif
!define APP "Vox"
!define EXE "vox-client.exe"
!define UNINST_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\Vox"

Name "${APP}"
OutFile "${OUTDIR}\Vox-${VERSION}-windows-x64-setup.exe"
; Per-user install: no UAC prompt, and every registry key is under HKCU.
RequestExecutionLevel user
InstallDir "$LOCALAPPDATA\Programs\${APP}"
InstallDirRegKey HKCU "Software\${APP}" "InstallDir"
SetCompressor /SOLID lzma
!if /FileExists "${SRC}\assets\icon.ico"
  Icon "${SRC}\assets\icon.ico"
  UninstallIcon "${SRC}\assets\icon.ico"
!endif

!include "MUI2.nsh"
!define MUI_ABORTWARNING
!define MUI_FINISHPAGE_RUN "$INSTDIR\${EXE}"
!define MUI_FINISHPAGE_RUN_TEXT "Start ${APP} now"
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_COMPONENTS
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

VIProductVersion "${NUMVERSION}.0"
VIAddVersionKey "ProductName" "${APP}"
VIAddVersionKey "FileDescription" "${APP} installer"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "LegalCopyright" ""

Section "${APP} (required)" SecMain
  SectionIn RO
  SetShellVarContext current
  SetOutPath "$INSTDIR"
  File /oname=${EXE} "${BIN}"
  File /oname=README.md "${SRC}\docs\help\getting-started.md"
  SetOutPath "$INSTDIR\assets"
  File /r "${SRC}\assets\*.*"
  SetOutPath "$INSTDIR\data"
  File /r "${SRC}\data\*.*"
  SetOutPath "$INSTDIR"

  WriteRegStr HKCU "Software\${APP}" "InstallDir" "$INSTDIR"
  WriteRegStr HKCU "${UNINST_KEY}" "DisplayName" "${APP}"
  WriteRegStr HKCU "${UNINST_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINST_KEY}" "Publisher" "vox.fail"
  WriteRegStr HKCU "${UNINST_KEY}" "DisplayIcon" "$INSTDIR\assets\icon.ico"
  WriteRegStr HKCU "${UNINST_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINST_KEY}" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegDWORD HKCU "${UNINST_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINST_KEY}" "NoRepair" 1
  WriteUninstaller "$INSTDIR\uninstall.exe"

  ; Shortcuts start in the install folder, so the client finds assets\ and data\ next to it.
  CreateDirectory "$SMPROGRAMS\${APP}"
  CreateShortcut "$SMPROGRAMS\${APP}\${APP}.lnk" "$INSTDIR\${EXE}" "" "$INSTDIR\assets\icon.ico" 0
  CreateShortcut "$SMPROGRAMS\${APP}\Uninstall ${APP}.lnk" "$INSTDIR\uninstall.exe"
SectionEnd

Section "Desktop shortcut" SecDesktop
  SetShellVarContext current
  SetOutPath "$INSTDIR"
  CreateShortcut "$DESKTOP\${APP}.lnk" "$INSTDIR\${EXE}" "" "$INSTDIR\assets\icon.ico" 0
SectionEnd

LangString DESC_Main ${LANG_ENGLISH} "The Vox client, its characters, props, stage sets and world data."
LangString DESC_Desktop ${LANG_ENGLISH} "A shortcut on your desktop."
!insertmacro MUI_FUNCTION_DESCRIPTION_BEGIN
  !insertmacro MUI_DESCRIPTION_TEXT ${SecMain} $(DESC_Main)
  !insertmacro MUI_DESCRIPTION_TEXT ${SecDesktop} $(DESC_Desktop)
!insertmacro MUI_FUNCTION_DESCRIPTION_END

Section "Uninstall"
  SetShellVarContext current
  ; Leave the install folder before deleting it.
  SetOutPath "$TEMP"
  Delete "$DESKTOP\${APP}.lnk"
  RMDir /r "$SMPROGRAMS\${APP}"
  RMDir /r "$INSTDIR\assets"
  RMDir /r "$INSTDIR\data"
  Delete "$INSTDIR\${EXE}"
  Delete "$INSTDIR\README.md"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"
  DeleteRegKey HKCU "${UNINST_KEY}"
  DeleteRegKey HKCU "Software\${APP}"
  ; %APPDATA%\Vox (your own levels and your controls) is left alone on purpose: the player's
  ; own files are never removed by the uninstaller.
SectionEnd
