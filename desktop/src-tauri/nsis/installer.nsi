; Plenum NSIS installer hooks
; Adds firewall rules automatically during installation so the user never
; needs to touch PowerShell or admin prompts manually.

!macro customInstall
  ; Allow Plenum through Windows Firewall (UDP discovery + TCP transfer)
  nsExec::ExecToLog 'netsh advfirewall firewall add rule name="Plenum (UDP Discovery)" dir=in action=allow protocol=UDP localport=41820 program="$INSTDIR\desktop.exe" enable=yes'
  nsExec::ExecToLog 'netsh advfirewall firewall add rule name="Plenum (TCP Transfer)" dir=in action=allow program="$INSTDIR\desktop.exe" enable=yes'
!macroend

!macro customUnInstall
  ; Clean up firewall rules on uninstall
  nsExec::ExecToLog 'netsh advfirewall firewall delete rule name="Plenum (UDP Discovery)"'
  nsExec::ExecToLog 'netsh advfirewall firewall delete rule name="Plenum (TCP Transfer)"'
!macroend
