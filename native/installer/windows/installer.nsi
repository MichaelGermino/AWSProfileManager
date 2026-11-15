; AWS Profile Manager 2.0 — Windows installer (plan §6.5).
;
; Built by native/tools/build-installer.ps1, which stages the files and passes:
;   /DVERSION=2.0.0  /DVERSION_NUMERIC=2.0.0.0  /DAPP_DIR=<staging dir>  /DOUT_FILE=<setup exe>
;
; It takes over 1.x's identity exactly, so an update is seamless in both directions:
;   - per-user install into the same folder (%LOCALAPPDATA%\Programs\aws-profile-manager), same exe
;     name ("AWS Profile Manager.exe"), so the login item (HKCU Run "electron.app.AWS Profile Manager")
;     and every pinned shortcut keep pointing at the right file;
;   - same uninstall entry (electron-builder's GUID for appId com.awsprofilemanager.app) and the same
;     install key with KeepShortcuts=true, so the 1.x installer can later take the app back
;     (rollback, §7.4): it runs this uninstaller with "/S /KEEP_APP_DATA --updated [--keep-shortcuts]
;     _?=<dir>" and installs 1.x over it;
;   - shortcuts with the same names; the app gives them 1.x's AppUserModelID on start.
;
; Command line (the same flags electron-builder installers take):
;   /S                silent (both updaters)
;   --updated         an update: shortcuts are left as they are (1.x's electron-updater passes it)
;   --force-run       start the app when done, also when silent
;   --no-desktop-shortcut   a fresh install creates no desktop shortcut
;
; Before installing, a 1.x (Electron) install is removed with its own uninstaller in update mode:
; /KEEP_APP_DATA and --updated, never --delete-app-data. App data (%APPDATA%\aws-profile-manager),
; the keychain and ~/.aws are never touched by this installer or by either uninstaller.

Unicode true
ManifestDPIAware true
SetCompressor /SOLID lzma
RequestExecutionLevel user

!ifndef VERSION
  !error "VERSION is required"
!endif
!ifndef VERSION_NUMERIC
  !error "VERSION_NUMERIC is required (x.y.z.w)"
!endif
!ifndef APP_DIR
  !error "APP_DIR (the staged files) is required"
!endif
!ifndef OUT_FILE
  !error "OUT_FILE is required"
!endif

!define PRODUCT_NAME "AWS Profile Manager"
!define APP_ID "com.awsprofilemanager.app"
; electron-builder: UUID v5 of the appId — the 1.x uninstall entry and install key.
!define APP_GUID "d557df58-8eda-5532-9fc4-6897818bafbc"
!define EXE_NAME "AWS Profile Manager.exe"
!define UNINSTALLER_NAME "Uninstall AWS Profile Manager.exe"
!define UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_GUID}"
!define INSTALL_KEY "Software\${APP_GUID}"
!define RUN_KEY "Software\Microsoft\Windows\CurrentVersion\Run"
!define RUN_VALUE "electron.app.AWS Profile Manager"

!include "MUI2.nsh"
!include "LogicLib.nsh"
!include "FileFunc.nsh"
!include "WordFunc.nsh"

Name "${PRODUCT_NAME}"
OutFile "${OUT_FILE}"
InstallDir "$LOCALAPPDATA\Programs\aws-profile-manager"
BrandingText "${PRODUCT_NAME} ${VERSION}"
ShowInstDetails nevershow
ShowUnInstDetails nevershow
AutoCloseWindow true

VIProductVersion "${VERSION_NUMERIC}"
VIFileVersion "${VERSION_NUMERIC}"
VIAddVersionKey /LANG=1033 "ProductName" "${PRODUCT_NAME}"
VIAddVersionKey /LANG=1033 "FileDescription" "${PRODUCT_NAME} Setup"
VIAddVersionKey /LANG=1033 "CompanyName" "${PRODUCT_NAME}"
VIAddVersionKey /LANG=1033 "LegalCopyright" "Copyright © 2026 ${PRODUCT_NAME}"
VIAddVersionKey /LANG=1033 "FileVersion" "${VERSION}"
VIAddVersionKey /LANG=1033 "ProductVersion" "${VERSION}"

!define MUI_ICON "${APP_DIR}\resources\icon.ico"
!define MUI_UNICON "${APP_DIR}\resources\icon.ico"
; One-click, like 1.x: no wizard pages, only progress.
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

Var isUpdated
Var forceRun
Var noDesktop
Var keepShortcuts
Var freshInstall
Var hadDesktopLink
Var oldDir

; ---- shared helpers (installer and uninstaller) ----------------------------------------------

; Is "AWS Profile Manager.exe" running for this user? Sets $0 to "1" or "0".
!macro IS_APP_RUNNING
  ReadEnvStr $1 "USERNAME"
  nsExec::ExecToStack '"$SYSDIR\cmd.exe" /C ""$SYSDIR\tasklist.exe" /NH /FI "IMAGENAME eq ${EXE_NAME}" /FI "USERNAME eq $1" | "$SYSDIR\find.exe" /I "${EXE_NAME}""'
  Pop $0 ; exit code of find: 0 = found
  Pop $2 ; output (unused)
  ${If} $0 == "0"
    StrCpy $0 "1"
  ${Else}
    StrCpy $0 "0"
  ${EndIf}
!macroend

; Close the app before files are replaced. During an update the old app is already quitting, so it
; gets a few seconds first; then it is ended. Aborts if it is still running after that.
!macro CLOSE_APP_FUNCTION PREFIX
Function ${PREFIX}CloseApp
  !insertmacro IS_APP_RUNNING
  ${If} $0 == "0"
    Return
  ${EndIf}
  ${If} $isUpdated != "1"
  ${AndIfNot} ${Silent}
    MessageBox MB_OKCANCEL|MB_ICONINFORMATION "${PRODUCT_NAME} is running. It will be closed to continue." /SD IDOK IDOK +2
    Abort
  ${EndIf}
  StrCpy $R1 0
  ${Do}
    Sleep 500
    IntOp $R1 $R1 + 1
    !insertmacro IS_APP_RUNNING
    ${If} $0 == "0"
      Return
    ${EndIf}
    ${If} $R1 == 6
    ${OrIf} $R1 == 14
      ReadEnvStr $1 "USERNAME"
      nsExec::Exec '"$SYSDIR\taskkill.exe" /F /T /FI "IMAGENAME eq ${EXE_NAME}" /FI "USERNAME eq $1"'
      Pop $0
    ${EndIf}
  ${LoopUntil} $R1 >= 24
  MessageBox MB_OK|MB_ICONSTOP "${PRODUCT_NAME} could not be closed. Close it from the tray icon and run the installer again." /SD IDOK
  SetErrorLevel 2
  Quit
FunctionEnd
!macroend
!insertmacro CLOSE_APP_FUNCTION ""
!insertmacro CLOSE_APP_FUNCTION "un."

; ---- installer -----------------------------------------------------------------------------------

Function .onInit
  ${GetParameters} $R0
  StrCpy $isUpdated "0"
  ClearErrors
  ${GetOptions} $R0 "--updated" $R1
  ${IfNot} ${Errors}
    StrCpy $isUpdated "1"
  ${EndIf}
  StrCpy $forceRun "0"
  ClearErrors
  ${GetOptions} $R0 "--force-run" $R1
  ${IfNot} ${Errors}
    StrCpy $forceRun "1"
  ${EndIf}
  StrCpy $noDesktop "0"
  ClearErrors
  ${GetOptions} $R0 "--no-desktop-shortcut" $R1
  ${IfNot} ${Errors}
    StrCpy $noDesktop "1"
  ${EndIf}

  ; An existing install (1.x or 2.x) decides the folder, so every path that points at the exe stays valid.
  ReadRegStr $oldDir HKCU "${INSTALL_KEY}" "InstallLocation"
  ${If} $oldDir == ""
    ReadRegStr $R2 HKCU "${UNINSTALL_KEY}" "UninstallString"
    ${If} $R2 != ""
      ; "C:\...\Uninstall AWS Profile Manager.exe" /currentuser -> its folder
      StrCpy $R3 $R2 1
      ${If} $R3 == '"'
        StrCpy $R2 $R2 "" 1
        ${WordFind} $R2 '"' "+1" $R2
      ${EndIf}
      ${GetParent} $R2 $oldDir
    ${EndIf}
  ${EndIf}
  ${If} $oldDir != ""
  ${AndIf} ${FileExists} "$oldDir\*.*"
    StrCpy $INSTDIR $oldDir
    StrCpy $freshInstall "0"
  ${Else}
    StrCpy $oldDir ""
    StrCpy $freshInstall "1"
  ${EndIf}
FunctionEnd

; Remove a 1.x (Electron) install with its own uninstaller, keeping app data. Same call as
; electron-builder's installers make for an older version of themselves.
Function UninstallElectron
  ${If} $oldDir == ""
    Return
  ${EndIf}
  ${IfNot} ${FileExists} "$oldDir\resources\app.asar"
    Return ; not an Electron install (2.x, or nothing)
  ${EndIf}
  ${IfNot} ${FileExists} "$oldDir\${UNINSTALLER_NAME}"
    ; No uninstaller: leave the old files alone; the new files are written next to them below and
    ; the Electron runtime files are removed one by one.
    Call RemoveElectronFiles
    Return
  ${EndIf}
  StrCpy $R4 "/currentuser --updated"
  ReadRegStr $R5 HKCU "${INSTALL_KEY}" "KeepShortcuts"
  ${If} $R5 == "true"
    ; The 1.x uninstaller supports it: its shortcuts (same names, target and AppUserModelID) stay.
    StrCpy $R4 "$R4 --keep-shortcuts"
  ${EndIf}
  InitPluginsDir
  CopyFiles /SILENT "$oldDir\${UNINSTALLER_NAME}" "$PLUGINSDIR\old-uninstaller.exe"
  StrCpy $R6 0
  ${Do}
    IntOp $R6 $R6 + 1
    ClearErrors
    ExecWait '"$PLUGINSDIR\old-uninstaller.exe" /S /KEEP_APP_DATA $R4 _?=$oldDir' $R7
    ${If} ${Errors}
      ; Could not run the copy (policy): run it in place.
      ClearErrors
      ExecWait '"$oldDir\${UNINSTALLER_NAME}" /S /KEEP_APP_DATA $R4 _?=$oldDir' $R7
    ${EndIf}
    ${If} $R7 == "0"
      Return
    ${EndIf}
    Sleep 1000
    Call CloseApp
  ${LoopUntil} $R6 >= 5
  ; 1.x is still in place and untouched (its uninstaller restores its files when it fails).
  MessageBox MB_OK|MB_ICONSTOP "The previous version of ${PRODUCT_NAME} could not be removed (code $R7). Nothing was changed. Restart Windows and run the installer again." /SD IDOK
  SetErrorLevel 3
  Quit
FunctionEnd

; Electron runtime files left by a 1.x install whose uninstaller is missing. Only known names.
Function RemoveElectronFiles
  RMDir /r "$oldDir\locales"
  Delete "$oldDir\resources\app.asar"
  Delete "$oldDir\resources\app-update.yml"
  RMDir /r "$oldDir\resources\app.asar.unpacked"
  Delete "$oldDir\${EXE_NAME}"
  Delete "$oldDir\chrome_100_percent.pak"
  Delete "$oldDir\chrome_200_percent.pak"
  Delete "$oldDir\d3dcompiler_47.dll"
  Delete "$oldDir\dxcompiler.dll"
  Delete "$oldDir\dxil.dll"
  Delete "$oldDir\ffmpeg.dll"
  Delete "$oldDir\icudtl.dat"
  Delete "$oldDir\libEGL.dll"
  Delete "$oldDir\libGLESv2.dll"
  Delete "$oldDir\LICENSE.electron.txt"
  Delete "$oldDir\LICENSES.chromium.html"
  Delete "$oldDir\resources.pak"
  Delete "$oldDir\snapshot_blob.bin"
  Delete "$oldDir\v8_context_snapshot.bin"
  Delete "$oldDir\vk_swiftshader.dll"
  Delete "$oldDir\vk_swiftshader_icd.json"
  Delete "$oldDir\vulkan-1.dll"
FunctionEnd

Section "Install"
  SetShellVarContext current
  Call CloseApp

  ; Remembered before 1.x's uninstaller runs (without --keep-shortcuts it removes the links).
  StrCpy $hadDesktopLink "0"
  ${If} ${FileExists} "$DESKTOP\${PRODUCT_NAME}.lnk"
    StrCpy $hadDesktopLink "1"
  ${EndIf}

  Call UninstallElectron

  SetOutPath "$INSTDIR"
  ; A 2.x install: replace the bundled resources as a whole, so no stale file survives.
  ${If} ${FileExists} "$INSTDIR\${EXE_NAME}"
    RMDir /r "$INSTDIR\resources"
  ${EndIf}
  File /r "${APP_DIR}\*.*"
  WriteUninstaller "$INSTDIR\${UNINSTALLER_NAME}"

  ; The same entries 1.x wrote (Apps & features, and the install key the 1.x installer reads).
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayName" "${PRODUCT_NAME} ${VERSION}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "UninstallString" '"$INSTDIR\${UNINSTALLER_NAME}" /currentuser'
  WriteRegStr HKCU "${UNINSTALL_KEY}" "QuietUninstallString" '"$INSTDIR\${UNINSTALLER_NAME}" /currentuser /S'
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayIcon" "$INSTDIR\${EXE_NAME},0"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoRepair" 1
  ${GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
  IntFmt $0 "0x%08X" $0
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "EstimatedSize" "$0"
  WriteRegStr HKCU "${INSTALL_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${INSTALL_KEY}" "KeepShortcuts" "true"
  WriteRegStr HKCU "${INSTALL_KEY}" "ShortcutName" "${PRODUCT_NAME}"

  ; Shortcuts. Existing ones (kept from 1.x or an earlier 2.x) stay untouched: same target, and
  ; they carry 1.x's AppUserModelID, so pinned copies keep working. Missing ones are created; the
  ; app gives them the AppUserModelID (${APP_ID}) when it starts.
  ${IfNot} ${FileExists} "$SMPROGRAMS\${PRODUCT_NAME}.lnk"
    CreateShortCut "$SMPROGRAMS\${PRODUCT_NAME}.lnk" "$INSTDIR\${EXE_NAME}" "" "$INSTDIR\${EXE_NAME}" 0
  ${EndIf}
  ${IfNot} ${FileExists} "$DESKTOP\${PRODUCT_NAME}.lnk"
    ${If} $hadDesktopLink == "1"
    ${OrIf} $freshInstall == "1"
      ${If} $noDesktop != "1"
      ${OrIf} $hadDesktopLink == "1"
        CreateShortCut "$DESKTOP\${PRODUCT_NAME}.lnk" "$INSTDIR\${EXE_NAME}" "" "$INSTDIR\${EXE_NAME}" 0
      ${EndIf}
    ${EndIf}
  ${EndIf}
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, i 0, i 0)'

  ; The login item is left as it is: same value name and same exe path as 1.x, and the app
  ; rewrites it from its setting on every start.

  ${If} $forceRun == "1"
  ${OrIfNot} ${Silent}
    Exec '"$INSTDIR\${EXE_NAME}"'
  ${EndIf}
SectionEnd

; ---- uninstaller ---------------------------------------------------------------------------------

Function un.onInit
  ${GetParameters} $R0
  StrCpy $isUpdated "0"
  ClearErrors
  ${GetOptions} $R0 "--updated" $R1
  ${IfNot} ${Errors}
    StrCpy $isUpdated "1"
  ${EndIf}
  StrCpy $keepShortcuts "0"
  ClearErrors
  ${GetOptions} $R0 "--keep-shortcuts" $R1
  ${IfNot} ${Errors}
    StrCpy $keepShortcuts "1"
  ${EndIf}
FunctionEnd

Section "Uninstall"
  SetShellVarContext current
  Call un.CloseApp

  ; Only the files this installer put there; the folder itself goes only if nothing else is left.
  ${If} ${FileExists} "$INSTDIR\${EXE_NAME}"
    RMDir /r "$INSTDIR\resources"
  ${EndIf}
  Delete "$INSTDIR\${EXE_NAME}"
  Delete "$INSTDIR\THIRD_PARTY_NOTICES.txt"
  Delete "$INSTDIR\${UNINSTALLER_NAME}"
  RMDir "$INSTDIR"

  ${If} $keepShortcuts != "1"
    Delete "$SMPROGRAMS\${PRODUCT_NAME}.lnk"
    Delete "$DESKTOP\${PRODUCT_NAME}.lnk"
    System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, i 0, i 0)'
  ${EndIf}

  ; A real uninstall also removes the login item, if it is ours. An update (the 1.x installer taking
  ; over) keeps it: 1.x uses the same entry for the same exe path.
  ${If} $isUpdated != "1"
    ReadRegStr $0 HKCU "${RUN_KEY}" "${RUN_VALUE}"
    ${If} $0 == '"$INSTDIR\${EXE_NAME}"'
      DeleteRegValue HKCU "${RUN_KEY}" "${RUN_VALUE}"
    ${EndIf}
  ${EndIf}

  DeleteRegKey HKCU "${UNINSTALL_KEY}"
  DeleteRegKey HKCU "${INSTALL_KEY}"
  ; App data (%APPDATA%\aws-profile-manager), keychain entries and ~/.aws are kept, as in 1.x
  ; (deleteAppDataOnUninstall was never set).
SectionEnd
