!define DOMIOS_INSTALLER_HOOK_DIR "${__FILEDIR__}"

!macro DomiosMeshFirewall OPERATION
  nsExec::ExecToLog '"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$INSTDIR\installer\account-mesh-firewall.ps1" -Operation ${OPERATION} -ServerProgram "$INSTDIR\domios-server\domios-server.exe"'
  Pop $0
  ${If} $0 != 0
    DetailPrint "Account mesh firewall setup returned $0. Installation continues; Tailscale access may require permission."
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTINSTALL
  !insertmacro DomiosMeshFirewall Ensure
!macroend

!macro NSIS_HOOK_PREINSTALL
  InitPluginsDir
  File /oname=$PLUGINSDIR\installer-preflight.ps1 "${DOMIOS_INSTALLER_HOOK_DIR}\installer-preflight.ps1"
  File /oname=$PLUGINSDIR\installer-process-model.ps1 "${DOMIOS_INSTALLER_HOOK_DIR}\installer-process-model.ps1"
  nsExec::ExecToLog '"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$PLUGINSDIR\installer-preflight.ps1" -InstallRoot "$INSTDIR"'
  Pop $0
  ${If} $0 != 0
    MessageBox MB_OK|MB_ICONSTOP "Domios could not stop this installation safely. Close Domios and retry. No files were replaced." /SD IDOK
    Abort
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ${If} $UpdateMode <> 1
    !insertmacro DomiosMeshFirewall Remove
  ${EndIf}
!macroend

