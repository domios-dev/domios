!define DOMIOS_INSTALLER_HOOK_DIR "${__FILEDIR__}"

!macro DomiosInstallerEvidence STAGE
  Push $9
  ClearErrors
  FileOpen $9 "$TEMP\Domios-installer.log" a
  ${IfNot} ${Errors}
    FileWrite $9 "stage=${STAGE} version=${VERSION} installer=$EXEPATH root=$INSTDIR$\r$\n"
    FileClose $9
  ${EndIf}
  ClearErrors
  Pop $9
!macroend

!macro DomiosMeshFirewall OPERATION
  nsExec::ExecToLog '"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$INSTDIR\installer\account-mesh-firewall.ps1" -Operation ${OPERATION} -ServerProgram "$INSTDIR\domios-server\domios-server.exe"'
  Pop $0
  ${If} $0 != 0
    DetailPrint "Account mesh firewall setup returned $0. Installation continues; Tailscale access may require permission."
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTINSTALL
  !insertmacro DomiosInstallerEvidence "postinstall"
!if "${BUNDLEID}" != "com.domios.dev.updater"
  !insertmacro DomiosMeshFirewall Ensure
!endif
!macroend

!macro NSIS_HOOK_PREINSTALL
  !insertmacro DomiosInstallerEvidence "preinstall"
  InitPluginsDir
  File /oname=$PLUGINSDIR\installer-preflight.ps1 "${DOMIOS_INSTALLER_HOOK_DIR}\installer-preflight.ps1"
  File /oname=$PLUGINSDIR\installer-process-model.ps1 "${DOMIOS_INSTALLER_HOOK_DIR}\installer-process-model.ps1"
  File /oname=$PLUGINSDIR\installer-preflight-flow.ps1 "${DOMIOS_INSTALLER_HOOK_DIR}\installer-preflight-flow.ps1"
  File /oname=$PLUGINSDIR\installer-process-native.cs "${DOMIOS_INSTALLER_HOOK_DIR}\installer-process-native.cs"
  File /oname=$PLUGINSDIR\installer-process-capture.ps1 "${DOMIOS_INSTALLER_HOOK_DIR}\installer-process-capture.ps1"
  domios_preflight_retry:
!if "${BUNDLEID}" == "com.domios.dev.updater"
  !if "${MAINBINARYNAME}" != "domios-dev-app"
    !error "Isolated updater requires the dedicated DEV executable"
  !endif
  nsExec::ExecToStack '"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$PLUGINSDIR\installer-preflight.ps1" -InstallRoot "$INSTDIR" -Development -TargetVersion "${VERSION}"'
!else
  nsExec::ExecToStack '"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$PLUGINSDIR\installer-preflight.ps1" -InstallRoot "$INSTDIR" -TargetVersion "${VERSION}"'
!endif
  Pop $0
  Pop $1
  ${If} $0 != 0
    !insertmacro DomiosInstallerEvidence "preflight_refused"
    DetailPrint "$1"
    MessageBox MB_RETRYCANCEL|MB_ICONEXCLAMATION "$1$\r$\n$\r$\nClose the named holder, then choose Retry to check again. Cancel leaves the installation unchanged." /SD IDCANCEL IDRETRY domios_preflight_retry
    Abort
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREUNINSTALL
!if "${BUNDLEID}" != "com.domios.dev.updater"
  ${If} $UpdateMode <> 1
    !insertmacro DomiosMeshFirewall Remove
  ${EndIf}
!endif
!macroend

