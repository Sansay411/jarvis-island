; Jarvis's own uninstall window (installer/src/uninstall.rs) replaces the plain
; NSIS one in «Apps & features». It is built before `tauri build`; without it
; the package keeps the NSIS uninstaller as before.
;
; These run on every install and update, so the registration survives
; auto-updates. Updates call `UninstallString` with /UPDATE /P _?=<folder>:
; the window hands such calls to uninstall.exe unchanged.

!define JARVIS_UNINSTALLER "${__FILEDIR__}\..\..\installer\target\release\jarvis-uninstall.exe"

!macro NSIS_HOOK_POSTINSTALL
  !if /FileExists "${JARVIS_UNINSTALLER}"
    File "/oname=$INSTDIR\jarvis-uninstall.exe" "${JARVIS_UNINSTALLER}"
    WriteRegStr SHCTX "${UNINSTKEY}" "UninstallString" "$\"$INSTDIR\jarvis-uninstall.exe$\""
    WriteRegStr SHCTX "${UNINSTKEY}" "QuietUninstallString" "$\"$INSTDIR\uninstall.exe$\" /S"
  !endif
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $UpdateMode <> 1
    Delete "$INSTDIR\jarvis-uninstall.exe"
    RMDir "$INSTDIR"
  ${EndIf}
!macroend
