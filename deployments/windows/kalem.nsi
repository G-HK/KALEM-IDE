; Kalem IDE installer (NSIS 3). Built by package.ps1:
;
;   makensis /DVERSION=0.1.0 /DNUMVERSION=0.1.0 /DARCH=x64 /DBINDIR=... /DLANGDIR=... /DROOTDIR=... /DOUTFILE=... kalem.nsi
;
; Per-machine install into Program Files, `languages\` beside the exe, an optional "add to PATH"
; "Add to PATH" page (ticked by default) and a normal uninstaller.
; Silent install: setup.exe /S   (add /NOPATH to skip the PATH change, /D=C:\dir last to change
; the folder). Silent uninstall: "%ProgramFiles%\Kalem IDE\uninstall.exe" /S

Unicode true
!ifndef VERSION
  !error "pass /DVERSION=x.y.z"
!endif
!ifndef NUMVERSION
  !error "pass /DNUMVERSION=x.y.z (numeric part of the version)"
!endif
!ifndef OUTFILE
  !define OUTFILE "kalem-setup.exe"
!endif

!include "MUI2.nsh"
!include "LogicLib.nsh"
!include "WinMessages.nsh"
!include "x64.nsh"
!include "nsDialogs.nsh"
!include "FileFunc.nsh"
!insertmacro GetSize
!insertmacro GetParameters
!insertmacro GetOptions

Name "Kalem IDE"
OutFile "${OUTFILE}"
InstallDir "$PROGRAMFILES64\Kalem IDE"
InstallDirRegKey HKLM "Software\Kalem IDE" "InstallDir"
RequestExecutionLevel admin
SetCompressor /SOLID lzma
BrandingText "Kalem IDE ${VERSION}"

VIProductVersion "${NUMVERSION}.0"
VIAddVersionKey "ProductName" "Kalem IDE"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "FileDescription" "Kalem IDE installer"
VIAddVersionKey "LegalCopyright" "MIT OR Apache-2.0"

!define UNINSTKEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\Kalem IDE"
!define ENVKEY "SYSTEM\CurrentControlSet\Control\Session Manager\Environment"

Var AddPath
Var PathCheckbox

!define MUI_ABORTWARNING
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_LICENSE "${ROOTDIR}\LICENSE-MIT"
!insertmacro MUI_PAGE_DIRECTORY
Page custom PathPageCreate PathPageLeave
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

Function .onInit
  SetRegView 64
  ; PATH is added unless the user unticks it or passes /NOPATH (silent installs).
  StrCpy $AddPath 1
  ${GetParameters} $R0
  ClearErrors
  ${GetOptions} $R0 "/NOPATH" $R1
  ${IfNot} ${Errors}
    StrCpy $AddPath 0
  ${EndIf}
FunctionEnd

Function PathPageCreate
  !insertmacro MUI_HEADER_TEXT "Add Kalem to PATH" "Run Kalem from any terminal"
  nsDialogs::Create 1018
  Pop $0
  ${If} $0 == error
    Abort
  ${EndIf}
  ${NSD_CreateLabel} 0 0 100% 40u "Adding the Kalem folder to the system PATH lets you type 'kalem' in any terminal (PowerShell, Command Prompt, the terminal in your editor) without the full path.$\r$\n$\r$\nOpen a new terminal after installing for it to take effect. The uninstaller removes the entry again."
  Pop $0
  ${NSD_CreateCheckbox} 0 52u 100% 12u "Add Kalem to the system PATH (recommended)"
  Pop $PathCheckbox
  ${If} $AddPath == 1
    ${NSD_Check} $PathCheckbox
  ${EndIf}
  nsDialogs::Show
FunctionEnd

Function PathPageLeave
  ${NSD_GetState} $PathCheckbox $AddPath
FunctionEnd

Function un.onInit
  SetRegView 64
FunctionEnd

Section "Kalem IDE (required)" SecMain
  SectionIn RO
  SetOutPath "$INSTDIR"
  File "${BINDIR}\kalem.exe"
  File "${ROOTDIR}\LICENSE-MIT"
  File "${ROOTDIR}\LICENSE-APACHE"
  File "${ROOTDIR}\README.md"
  SetOutPath "$INSTDIR\languages"
  File "${LANGDIR}\*.toml"

  WriteUninstaller "$INSTDIR\uninstall.exe"
  WriteRegStr HKLM "Software\Kalem IDE" "InstallDir" "$INSTDIR"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\App Paths\kalem.exe" "" "$INSTDIR\kalem.exe"

  WriteRegStr HKLM "${UNINSTKEY}" "DisplayName" "Kalem IDE"
  WriteRegStr HKLM "${UNINSTKEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKLM "${UNINSTKEY}" "Publisher" "Kalem IDE contributors"
  WriteRegStr HKLM "${UNINSTKEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKLM "${UNINSTKEY}" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegStr HKLM "${UNINSTKEY}" "QuietUninstallString" '"$INSTDIR\uninstall.exe" /S'
  WriteRegStr HKLM "${UNINSTKEY}" "URLInfoAbout" "https://github.com/OWNER/kalem-ide"
  WriteRegDWORD HKLM "${UNINSTKEY}" "NoModify" 1
  WriteRegDWORD HKLM "${UNINSTKEY}" "NoRepair" 1
  ${GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
  IntFmt $0 "0x%08X" $0
  WriteRegDWORD HKLM "${UNINSTKEY}" "EstimatedSize" "$0"
SectionEnd

; PATH is edited through PowerShell on purpose: NSIS strings are capped (1024 chars in the
; standard build), which would truncate a long PATH. The registry value keeps its REG_EXPAND_SZ
; type so entries such as %SystemRoot% keep working.
Section "-Add Kalem to PATH" SecPath
  ${If} $AddPath != 1
    DetailPrint "Skipping PATH (not selected)."
    Return
  ${EndIf}
  nsExec::ExecToLog `powershell.exe -NoProfile -ExecutionPolicy Bypass -Command "$$k=[Microsoft.Win32.Registry]::LocalMachine.OpenSubKey('${ENVKEY}',$$true); $$p=$$k.GetValue('Path','','DoNotExpandEnvironmentNames'); $$d='$INSTDIR'.TrimEnd('\'); if((($$p -split ';') | ForEach-Object { $$_.TrimEnd('\') }) -notcontains $$d) { $$n=(@($$p.TrimEnd(';'),$$d) | Where-Object { $$_ }) -join ';'; $$k.SetValue('Path',$$n,'ExpandString') }; $$k.Close()"`
  Pop $0
  ${If} $0 != 0
    DetailPrint "Could not add Kalem to PATH (exit code $0). Add $INSTDIR to PATH manually."
  ${Else}
    DetailPrint "Added $INSTDIR to the system PATH. Open a new terminal to use 'kalem'."
  ${EndIf}
  SendMessage ${HWND_BROADCAST} ${WM_WININICHANGE} 0 "STR:Environment" /TIMEOUT=5000
SectionEnd

Section "Uninstall"
  nsExec::ExecToLog `powershell.exe -NoProfile -ExecutionPolicy Bypass -Command "$$k=[Microsoft.Win32.Registry]::LocalMachine.OpenSubKey('${ENVKEY}',$$true); $$p=$$k.GetValue('Path','','DoNotExpandEnvironmentNames'); $$d='$INSTDIR'.TrimEnd('\'); $$n=(($$p -split ';') | Where-Object { $$_ -and ($$_.TrimEnd('\') -ne $$d) }) -join ';'; $$k.SetValue('Path',$$n,'ExpandString'); $$k.Close()"`
  Pop $0
  SendMessage ${HWND_BROADCAST} ${WM_WININICHANGE} 0 "STR:Environment" /TIMEOUT=5000

  Delete "$INSTDIR\kalem.exe"
  Delete "$INSTDIR\LICENSE-MIT"
  Delete "$INSTDIR\LICENSE-APACHE"
  Delete "$INSTDIR\README.md"
  Delete "$INSTDIR\languages\*.toml"
  RMDir "$INSTDIR\languages"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"

  DeleteRegKey HKLM "${UNINSTKEY}"
  DeleteRegKey HKLM "Software\Microsoft\Windows\CurrentVersion\App Paths\kalem.exe"
  DeleteRegKey HKLM "Software\Kalem IDE"
SectionEnd
