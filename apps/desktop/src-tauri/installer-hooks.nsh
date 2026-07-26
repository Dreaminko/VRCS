; Keep upgrades in the directory selected by the existing installation.
; Tauri stores the canonical path in ${MANUPRODUCTKEY}; checking the
; uninstaller prevents a stale registry value from redirecting a fresh install.
!macro NSIS_HOOK_PREINSTALL
  ReadRegStr $R8 SHCTX "${MANUPRODUCTKEY}" ""
  StrCmp $R8 "" vrcs_upgrade_location_done
  IfFileExists "$R8\uninstall.exe" 0 vrcs_upgrade_location_done
  StrCpy $R9 "$INSTDIR"
  StrCpy $INSTDIR "$R8"
  SetOutPath $INSTDIR
  RMDir "$R9"

  vrcs_upgrade_location_done:
!macroend
