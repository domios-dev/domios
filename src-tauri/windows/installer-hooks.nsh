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

!macro NSIS_HOOK_PREUNINSTALL
  ${If} $UpdateMode <> 1
    !insertmacro DomiosMeshFirewall Remove
  ${EndIf}
!macroend
